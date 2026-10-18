use crate::ip_sets::{IpRanges, IpSet, IpSets, MAX_SETS_PER_MATCHER, Membership};
use crate::policy::schema::{Condition, NumberMatch, StringMatch};
use anyhow::{Context, Result, ensure};
use http::{HeaderMap, HeaderName, Method};
use regex::Regex;
use std::net::IpAddr;
use std::sync::Arc;

pub struct Request<'a> {
  pub headers: &'a HeaderMap,
  pub path: &'a str,
  pub method: &'a Method,
  pub client_ip: Option<IpAddr>,
}

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
}

#[derive(Debug)]
pub(crate) enum Compiled {
  All(Vec<Self>),
  Any(Vec<Self>),
  Not(Box<Self>),
  Text(Field, Text),
  Ip(IpRanges),
  Sets(Vec<Arc<IpSet>>),
  Score(NumberMatch),
}
#[derive(Debug)]
pub(crate) enum Field {
  Path,
  Method,
  UserAgent,
  Header(HeaderName),
}
#[derive(Debug)]
pub(crate) struct Text {
  source: StringMatch,
  regex: Option<Regex>,
}
#[derive(Default)]
pub(crate) struct Budget {
  nodes: usize,
  bytes: usize,
}
impl Budget {
  fn consume(&mut self, depth: usize, path: &str) -> Result<()> {
    self.nodes += 1;
    ensure!(depth <= 16, "{path}: condition depth must not exceed 16");
    ensure!(
      self.nodes <= 512,
      "{path}: policy must not exceed 512 condition nodes"
    );
    Ok(())
  }
}
impl Compiled {
  pub(crate) fn compile(
    condition: &Condition,
    path: &str,
    depth: usize,
    budget: &mut Budget,
    sets: &IpSets,
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
      )?)));
    }
    let mut fields = vec![
      (Field::Path, &condition.path),
      (Field::Method, &condition.method),
      (Field::UserAgent, &condition.user_agent),
    ];
    for (field, source) in fields.drain(..) {
      if let Some(source) = source {
        budget.consume(depth, path)?;
        let label = match &field {
          Field::Path => "path",
          Field::Method => "method",
          Field::UserAgent => "user_agent",
          Field::Header(_) => unreachable!(),
        };
        let test = Text::compile(source, budget).with_context(|| format!("{path}.{label}"))?;
        nodes.push(Self::Text(field, test));
      }
    }
    for (name, source) in &condition.headers {
      budget.consume(depth, path)?;
      let header = HeaderName::from_bytes(name.as_bytes())
        .with_context(|| format!("{path}.headers: expected an HTTP header name"))?;
      let test =
        Text::compile(source, budget).with_context(|| format!("{path}.headers.{header}"))?;
      nodes.push(Self::Text(Field::Header(header), test));
    }
    if let Some(value) = &condition.client_ip {
      budget.consume(depth, path)?;
      nodes.push(Self::compile_ip(value, path, sets)?);
    }
    if let Some(value) = &condition.score {
      budget.consume(depth, path)?;
      ensure!(
        value.eq.is_some()
          || value.gt.is_some()
          || value.gte.is_some()
          || value.lt.is_some()
          || value.lte.is_some(),
        "{path}.score: expected a numeric comparison"
      );
      let lower = [
        Some(i32::MIN as i64),
        value.eq.map(i64::from),
        value.gte.map(i64::from),
        value.gt.map(|v| i64::from(v) + 1),
      ]
      .into_iter()
      .flatten()
      .max()
      .unwrap();
      let upper = [
        Some(i32::MAX as i64),
        value.eq.map(i64::from),
        value.lte.map(i64::from),
        value.lt.map(|v| i64::from(v) - 1),
      ]
      .into_iter()
      .flatten()
      .min()
      .unwrap();
      ensure!(
        lower <= upper,
        "{path}.score: comparisons describe an empty range"
      );
      nodes.push(Self::Score(value.clone()));
    }
    ensure!(
      !nodes.is_empty(),
      "{path}: condition must contain at least one matcher"
    );
    Ok(Self::All(nodes))
  }

  fn compile_ip(value: &crate::policy::schema::IpMatch, path: &str, sets: &IpSets) -> Result<Self> {
    ensure!(
      (value.cidr.is_some() as usize + value.in_sets.is_some() as usize) == 1,
      "{path}: client_ip: expected exactly one of cidr, in_sets"
    );
    if let Some(cidr) = &value.cidr {
      let network = cidr
        .parse()
        .with_context(|| format!("{path}.client_ip.cidr: expected an IPv4 or IPv6 network"))?;
      return Ok(Self::Ip(IpRanges::from_networks([network])));
    }
    let names = value.in_sets.as_ref().expect("one of cidr, in_sets");
    ensure!(
      !names.is_empty() && names.len() <= MAX_SETS_PER_MATCHER,
      "{path}.client_ip.in_sets: in_sets requires 1 to {MAX_SETS_PER_MATCHER} set names"
    );
    let mut resolved = Vec::with_capacity(names.len());
    for name in names {
      let set = sets
        .get(name)
        .with_context(|| format!("{path}.client_ip.in_sets: unknown ip set {name}"))?;
      resolved.push(set.clone());
    }
    Ok(Self::Sets(resolved))
  }

  pub(crate) fn matches(&self, request: &Request<'_>, score: i32) -> Truth {
    match self {
      Self::All(nodes) => {
        let mut unknown = false;
        for node in nodes {
          match node.matches(request, score) {
            Truth::False => return Truth::False,
            Truth::Unknown => unknown = true,
            Truth::True => {}
          }
        }
        if unknown { Truth::Unknown } else { Truth::True }
      }
      Self::Any(nodes) => {
        let mut unknown = false;
        for node in nodes {
          match node.matches(request, score) {
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
      Self::Not(node) => node.matches(request, score).not(),
      Self::Text(field, test) => {
        let value = match field {
          Field::Path => Some(request.path),
          Field::Method => Some(request.method.as_str()),
          Field::UserAgent => Some(
            request
              .headers
              .get(http::header::USER_AGENT)
              .and_then(|v| v.to_str().ok())
              .unwrap_or_default(),
          ),
          Field::Header(name) => {
            return truth(
              request
                .headers
                .get_all(name)
                .iter()
                .any(|v| v.to_str().is_ok_and(|v| test.matches(v))),
            );
          }
        };
        truth(value.is_some_and(|v| test.matches(v)))
      }
      Self::Ip(ranges) => truth(request.client_ip.is_some_and(|ip| ranges.contains(ip))),
      Self::Sets(sets) => {
        let Some(ip) = request.client_ip else {
          return Truth::False;
        };
        let mut unknown = false;
        for set in sets {
          match set.membership(ip) {
            Membership::Member => return Truth::True,
            Membership::Unknown => unknown = true,
            Membership::NotMember => {}
          }
        }
        if unknown {
          Truth::Unknown
        } else {
          Truth::False
        }
      }
      Self::Score(test) => truth(
        test.eq.is_none_or(|v| score == v)
          && test.gt.is_none_or(|v| score > v)
          && test.gte.is_none_or(|v| score >= v)
          && test.lt.is_none_or(|v| score < v)
          && test.lte.is_none_or(|v| score <= v),
      ),
    }
  }
}

fn truth(value: bool) -> Truth {
  if value { Truth::True } else { Truth::False }
}
impl Text {
  fn compile(source: &StringMatch, budget: &mut Budget) -> Result<Self> {
    let count = [
      source.exact.is_some(),
      source.prefix.is_some(),
      source.contains.is_some(),
      source.values.is_some(),
      source.regex.is_some(),
    ]
    .into_iter()
    .filter(|v| *v)
    .count();
    ensure!(
      count == 1,
      "string matcher requires exactly one of exact, prefix, contains, in, regex"
    );
    if let Some(values) = &source.values {
      ensure!(
        !values.is_empty() && values.len() <= 64,
        "in requires 1 to 64 values"
      );
    }
    for text in source
      .exact
      .iter()
      .chain(source.prefix.iter())
      .chain(source.contains.iter())
      .chain(source.regex.iter())
      .chain(source.values.iter().flatten())
    {
      ensure!(
        text.len() <= 2048,
        "matcher values must not exceed 2048 bytes"
      );
      budget.bytes += text.len();
    }
    ensure!(
      budget.bytes <= 65_536,
      "policy matcher values must not exceed 64 KiB in total"
    );
    let regex = source
      .regex
      .as_ref()
      .map(|v| {
        regex::RegexBuilder::new(v)
          .size_limit(262_144)
          .dfa_size_limit(262_144)
          .build()
      })
      .transpose()
      .context("regex: invalid or too complex regular expression")?;
    Ok(Self {
      source: source.clone(),
      regex,
    })
  }
  fn matches(&self, value: &str) -> bool {
    self.source.exact.as_ref().is_none_or(|v| value == v)
      && self
        .source
        .prefix
        .as_ref()
        .is_none_or(|v| value.starts_with(v))
      && self
        .source
        .contains
        .as_ref()
        .is_none_or(|v| value.contains(v))
      && self
        .source
        .values
        .as_ref()
        .is_none_or(|v| v.iter().any(|v| v == value))
      && self.regex.as_ref().is_none_or(|v| v.is_match(value))
  }
}
