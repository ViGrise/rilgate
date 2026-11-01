mod budget;
mod geo;
mod lookups;
mod network;
mod numeric;
mod text;

pub(crate) use budget::{Budget, Scope};
use numeric::Bounds;
use text::{Field, Text};

use crate::geoip::{GeoDatabase, GeoDatabases};
use crate::host_load::{HOST_LOAD, HostLoad};
use crate::ip_sets::{IpRanges, IpSet, Membership};
use crate::lookups::Lookups;
use crate::policy::schema::{Condition, LoadWindow};
use crate::{AsnSet, PolicySets};
use anyhow::{Result, ensure};
use http::{HeaderMap, Method};
use std::cell::Cell;
use std::net::IpAddr;
use std::sync::Arc;

pub struct Request<'a> {
  pub headers: &'a HeaderMap,
  pub trusted_peer: bool,
  pub path: &'a str,
  pub host: &'a str,
  pub method: &'a Method,
  pub client_ip: Option<IpAddr>,
  pub lookups: &'a Lookups,
}

pub(crate) const MAX_DNSBL_ZONES: usize = 8;
const MAX_ZONE_BYTES: usize = 120;
const MAX_REVERSE_DNS_DOMAINS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Truth {
  True,
  False,
  Unknown,
}

impl Truth {
  fn not(self) -> Self {
    match self {
      Self::True => Self::False,
      Self::False => Self::True,
      Self::Unknown => Self::Unknown,
    }
  }

  /// True when any value is True, else Unknown when any is Unknown, else False.
  pub(crate) fn any(values: impl IntoIterator<Item = Truth>) -> Truth {
    let mut unknown = false;
    for value in values {
      match value {
        Truth::True => return Truth::True,
        Truth::Unknown => unknown = true,
        Truth::False => {}
      }
    }
    if unknown {
      Truth::Unknown
    } else {
      Truth::False
    }
  }

  /// False when any value is False, else Unknown when any is Unknown, else True.
  pub(crate) fn all(values: impl IntoIterator<Item = Truth>) -> Truth {
    let mut unknown = false;
    for value in values {
      match value {
        Truth::False => return Truth::False,
        Truth::Unknown => unknown = true,
        Truth::True => {}
      }
    }
    if unknown { Truth::Unknown } else { Truth::True }
  }

  pub(crate) fn from_bool(value: bool) -> Truth {
    if value { Truth::True } else { Truth::False }
  }
}
impl From<Membership> for Truth {
  fn from(value: Membership) -> Self {
    match value {
      Membership::Member => Self::True,
      Membership::NotMember => Self::False,
      Membership::Unknown => Self::Unknown,
    }
  }
}

#[derive(Debug)]
pub(crate) enum Compiled {
  All(Vec<Self>),
  Any(Vec<Self>),
  Not(Box<Self>),
  Text(Field, Text),
  Ip(IpRanges),
  Sets(Vec<Arc<IpSet>>),
  Score(Bounds<i32>),
  Rate(usize, Bounds<u32>),
  Load(LoadWindow, Bounds<f64>),
  Country(Arc<GeoDatabase>, Vec<[u8; 2]>),
  Asn(Arc<GeoDatabase>, Vec<u32>),
  AsnSets(Arc<GeoDatabase>, Vec<Arc<AsnSet>>),
  Dnsbl(String),
  Honeypot(bool),
  ReverseDns(Vec<String>),
}

impl Compiled {
  pub(crate) fn compile(
    condition: &Condition,
    path: &str,
    depth: usize,
    budget: &mut Budget,
    sets: &PolicySets,
    geoip: &GeoDatabases,
  ) -> Result<Self> {
    budget.consume(depth, path)?;
    let mut nodes = Vec::new();
    for (values, any) in [(&condition.all, false), (&condition.any, true)] {
      if let Some(values) = values {
        let label = if any { "any" } else { "all" };
        ensure!(
          !values.is_empty(),
          "{path}.{label}: must contain at least one condition"
        );
        let children = values
          .iter()
          .enumerate()
          .map(|(index, c)| {
            Self::compile(
              c,
              &format!("{path}.{label}[{index}]"),
              depth + 1,
              budget,
              sets,
              geoip,
            )
          })
          .collect::<Result<Vec<_>>>()?;
        nodes.push(if any {
          Self::Any(children)
        } else {
          Self::All(children)
        });
      }
    }
    if let Some(value) = &condition.not {
      nodes.push(Self::Not(Box::new(Self::compile(
        value,
        &format!("{path}.not"),
        depth + 1,
        budget,
        sets,
        geoip,
      )?)));
    }
    text::compile_fields(condition, path, depth, budget, &mut nodes)?;
    if let Some(value) = &condition.client_ip {
      nodes.push(network::compile_ip(value, path, depth, sets, budget)?);
    }
    if let Some(value) = &condition.score {
      nodes.push(numeric::compile_score(value, path, depth, budget)?);
    }
    if let Some(value) = &condition.rate {
      nodes.push(numeric::compile_rate(value, path, depth, budget)?);
    }
    if let Some(value) = &condition.load {
      nodes.push(numeric::compile_load(value, path, depth, budget)?);
    }
    if let Some(codes) = &condition.country {
      nodes.push(geo::compile_country(codes, path, depth, budget, geoip)?);
    }
    if let Some(numbers) = &condition.asn {
      nodes.push(geo::compile_asn(numbers, path, depth, budget, sets, geoip)?);
    }
    if let Some(zone) = &condition.dnsbl {
      nodes.push(lookups::compile_dnsbl(zone, path, depth, budget)?);
    }
    if let Some(flagged) = condition.honeypot {
      nodes.push(lookups::compile_honeypot(flagged, path, depth, budget)?);
    }
    ensure!(
      !nodes.is_empty(),
      "{path}: condition must contain at least one matcher"
    );
    Ok(Self::All(nodes))
  }

  /// Evaluates the condition. `needs_lookups` is set when a lookup condition
  /// has no result in `request.lookups`.
  pub(crate) fn matches(
    &self,
    request: &Request<'_>,
    score: i32,
    load: &HostLoad,
    needs_lookups: &Cell<bool>,
  ) -> Truth {
    match self {
      Self::All(nodes) => Truth::all(
        nodes
          .iter()
          .map(|node| node.matches(request, score, load, needs_lookups)),
      ),
      Self::Any(nodes) => Truth::any(
        nodes
          .iter()
          .map(|node| node.matches(request, score, load, needs_lookups)),
      ),
      Self::Not(node) => node.matches(request, score, load, needs_lookups).not(),
      Self::Text(field, test) => text::match_field(field, test, request),
      Self::Ip(ranges) => network::match_ip(ranges, request),
      Self::Sets(sets) => network::match_sets(sets, request),
      Self::Score(bounds) => Truth::from_bool(bounds.contains(score)),
      Self::Rate(index, bounds) => numeric::match_rate(*index, bounds, request),
      Self::Load(window, bounds) => numeric::match_load(*window, bounds, load),
      Self::Country(database, codes) => geo::match_country(database, codes, request),
      Self::Asn(database, numbers) => geo::match_asn(database, numbers, request),
      Self::AsnSets(database, sets) => geo::match_asn_sets(database, sets, request),
      Self::Dnsbl(zone) => lookups::match_dnsbl(zone, request, needs_lookups),
      Self::Honeypot(expected) => lookups::match_honeypot(*expected, request),
      Self::ReverseDns(domains) => lookups::match_reverse_dns(domains, request, needs_lookups),
    }
  }
}

#[derive(Debug)]
pub struct CompiledCondition(Compiled);
impl CompiledCondition {
  pub fn compile(
    condition: &Condition,
    path: &str,
    sets: &PolicySets,
    geoip: &GeoDatabases,
  ) -> Result<Self> {
    let mut budget = Budget::for_request();
    Compiled::compile(condition, path, 0, &mut budget, sets, geoip).map(Self)
  }

  /// Selects requests whose condition evaluates to true with the current IP sets.
  pub fn matches(&self, request: &Request<'_>) -> bool {
    self.0.matches(request, 0, &HOST_LOAD, &Cell::new(false)) == Truth::True
  }
}
