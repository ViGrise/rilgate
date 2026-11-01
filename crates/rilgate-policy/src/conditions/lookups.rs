use super::{
  Budget, Compiled, MAX_DNSBL_ZONES, MAX_REVERSE_DNS_DOMAINS, MAX_ZONE_BYTES, Request, Truth,
};
use crate::lookups::{in_domains, normalize_name};
use anyhow::{Context, Result, ensure};
use std::cell::Cell;

pub(super) fn compile_dnsbl(
  zone: &str,
  path: &str,
  depth: usize,
  budget: &mut Budget,
) -> Result<Compiled> {
  budget.require_rule(&format!("{path}.dnsbl"), "dnsbl", false)?;
  budget.consume(depth, path)?;
  let zone = normalize_name(zone, MAX_ZONE_BYTES)
    .with_context(|| format!("{path}.dnsbl: expected a DNS zone name"))?;
  budget.dnsbl_zones.insert(zone.clone());
  ensure!(
    budget.dnsbl_zones.len() <= MAX_DNSBL_ZONES,
    "{path}.dnsbl: policy must not use more than {MAX_DNSBL_ZONES} DNSBL zones"
  );
  Ok(Compiled::Dnsbl(zone))
}

pub(super) fn compile_honeypot(
  flagged: bool,
  path: &str,
  depth: usize,
  budget: &mut Budget,
) -> Result<Compiled> {
  budget.require_rule(&format!("{path}.honeypot"), "honeypot", true)?;
  ensure!(
    budget.honeypot,
    "{path}.honeypot: requires signals.honeypot"
  );
  budget.consume(depth, path)?;
  Ok(Compiled::Honeypot(flagged))
}

pub(super) fn compile_reverse_dns(
  domains: &[String],
  path: &str,
  budget: &mut Budget,
) -> Result<Compiled> {
  let label = format!("{path}.client_ip.reverse_dns");
  budget.require_rule(&label, "reverse_dns", false)?;
  ensure!(
    (1..=MAX_REVERSE_DNS_DOMAINS).contains(&domains.len()),
    "{label}: expected 1 to {MAX_REVERSE_DNS_DOMAINS} domain names"
  );
  let domains = domains
    .iter()
    .enumerate()
    .map(|(index, domain)| {
      normalize_name(domain, 253)
        .with_context(|| format!("{label}[{index}]: expected a domain name"))
    })
    .collect::<Result<Vec<_>>>()?;
  budget.reverse_dns = true;
  Ok(Compiled::ReverseDns(domains))
}

pub(super) fn match_dnsbl(zone: &str, request: &Request<'_>, needs_lookups: &Cell<bool>) -> Truth {
  if request.client_ip.is_none() {
    return Truth::False;
  }
  match request.lookups.dnsbl.get(zone) {
    Some(listed) => Truth::from_bool(*listed),
    None => {
      needs_lookups.set(true);
      Truth::Unknown
    }
  }
}

pub(super) fn match_honeypot(expected: bool, request: &Request<'_>) -> Truth {
  Truth::from_bool(request.lookups.honeypot.unwrap_or(false) == expected)
}

pub(super) fn match_reverse_dns(
  domains: &[String],
  request: &Request<'_>,
  needs_lookups: &Cell<bool>,
) -> Truth {
  if request.client_ip.is_none() {
    return Truth::False;
  }
  match &request.lookups.reverse_dns {
    Some(name) => Truth::from_bool(
      name
        .as_deref()
        .is_some_and(|name| in_domains(name, domains)),
    ),
    None => {
      needs_lookups.set(true);
      Truth::Unknown
    }
  }
}
