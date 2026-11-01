use super::{Budget, Compiled, Request, Truth, lookups};
use crate::PolicySets;
use crate::feed_sets::MAX_SETS_PER_MATCHER;
use crate::ip_sets::{IpRanges, IpSet};
use crate::policy::schema::IpMatch;
use anyhow::{Context, Result, ensure};
use std::sync::Arc;

pub(super) fn compile_ip(
  value: &IpMatch,
  path: &str,
  depth: usize,
  sets: &PolicySets,
  budget: &mut Budget,
) -> Result<Compiled> {
  budget.consume(depth, path)?;
  ensure!(
    (value.cidr.is_some() as usize
      + value.in_sets.is_some() as usize
      + value.reverse_dns.is_some() as usize)
      == 1,
    "{path}: client_ip: expected exactly one of cidr, in_sets, reverse_dns"
  );
  if let Some(domains) = &value.reverse_dns {
    return lookups::compile_reverse_dns(domains, path, budget);
  }
  if let Some(cidr) = &value.cidr {
    let network = cidr
      .parse()
      .with_context(|| format!("{path}.client_ip.cidr: expected an IPv4 or IPv6 network"))?;
    return Ok(Compiled::Ip(IpRanges::from_networks([network])));
  }
  let names = value.in_sets.as_ref().expect("one of cidr, in_sets");
  ensure!(
    !names.is_empty() && names.len() <= MAX_SETS_PER_MATCHER,
    "{path}.client_ip.in_sets: in_sets requires 1 to {MAX_SETS_PER_MATCHER} set names"
  );
  let mut resolved = Vec::with_capacity(names.len());
  for name in names {
    let set = sets
      .ip
      .get(name)
      .with_context(|| format!("{path}.client_ip.in_sets: unknown ip set {name}"))?;
    resolved.push(set.clone());
  }
  Ok(Compiled::Sets(resolved))
}

pub(super) fn match_ip(ranges: &IpRanges, request: &Request<'_>) -> Truth {
  Truth::from_bool(request.client_ip.is_some_and(|ip| ranges.contains(ip)))
}

pub(super) fn match_sets(sets: &[Arc<IpSet>], request: &Request<'_>) -> Truth {
  let Some(ip) = request.client_ip else {
    return Truth::False;
  };
  Truth::any(sets.iter().map(|set| set.membership(ip).into()))
}
