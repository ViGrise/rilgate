use crate::{
  PolicySets,
  conditions::{Budget, Compiled, Request, Truth},
  geoip::{GeoDatabases, Lookup},
  host_load::HostLoad,
  policy::schema::{CounterDefinition, CounterField, CounterIp, CounterKeyPart, Definition},
};
use anyhow::{Context, Result, ensure};
use http::HeaderName;
use ipnet::IpNet;
use sha2::{Digest, Sha256};
use std::{cell::Cell, collections::HashSet, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyPart {
  Network(IpNet),
  Text(Box<[u8]>),
  Digest([u8; 32]),
  Number(u32),
}
pub type CounterKey = Box<[KeyPart]>;

#[derive(Debug)]
enum Extractor {
  Ip(CounterIp),
  Field(CounterField),
  Header(HeaderName),
}

#[derive(Debug)]
pub struct CompiledCounter {
  pub name: String,
  pub definition: CounterDefinition,
  pub used: bool,
  parts: Vec<Extractor>,
  count_when: Option<Compiled>,
}

impl CompiledCounter {
  pub fn key(&self, request: &Request<'_>, geoip: &GeoDatabases) -> Option<CounterKey> {
    self
      .parts
      .iter()
      .map(|part| {
        Some(match part {
          Extractor::Ip(prefix) => {
            let ip = request.client_ip?.to_canonical();
            let prefix = if ip.is_ipv4() {
              prefix.ipv4_prefix
            } else {
              prefix.ipv6_prefix
            };
            KeyPart::Network(IpNet::new(ip, prefix).ok()?.trunc())
          }
          Extractor::Header(name) => text(request.headers.get(name)?.as_bytes()),
          Extractor::Field(field) => match field {
            CounterField::Host => text(request.host.as_bytes()),
            CounterField::Path => text(request.path.as_bytes()),
            CounterField::Method => text(request.method.as_str().as_bytes()),
            CounterField::UserAgent => {
              text(request.headers.get(http::header::USER_AGENT)?.as_bytes())
            }
            CounterField::Country => {
              match geoip.country()?.country(request.client_ip?.to_canonical()) {
                Lookup::Known(Some(code)) => text(&code),
                _ => return None,
              }
            }
            CounterField::Asn => match geoip.asn()?.asn(request.client_ip?.to_canonical()) {
              Lookup::Known(Some(number)) => KeyPart::Number(number),
              _ => return None,
            },
            CounterField::ClientIp => unreachable!(),
          },
        })
      })
      .collect()
  }

  pub(crate) fn selects(&self, request: &Request<'_>, load: &HostLoad) -> bool {
    self
      .count_when
      .as_ref()
      .is_none_or(|condition| condition.matches(request, 0, load, &Cell::new(false)) == Truth::True)
  }
}

fn text(value: &[u8]) -> KeyPart {
  if value.len() <= 256 {
    KeyPart::Text(value.into())
  } else {
    KeyPart::Digest(Sha256::digest(value).into())
  }
}

pub(crate) fn compile(
  definition: &Definition,
  sets: &PolicySets,
  geoip: &GeoDatabases,
  budget: &mut Budget,
) -> Result<Vec<CompiledCounter>> {
  let Some(counters) = &definition.counters else {
    return Ok(Vec::new());
  };
  ensure!(
    counters.len() <= 32,
    "policy.counters: at most 32 counters are supported"
  );
  let mut compiled = Vec::new();
  for (name, counter) in counters {
    let path = format!("policy.counters.{name}");
    ensure!(
      (1..=64).contains(&name.len())
        && name
          .bytes()
          .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
      "{path}: name must contain 1 to 64 ASCII letters, digits, - or _"
    );
    ensure!(
      (Duration::from_secs(1)..=Duration::from_secs(3600)).contains(&counter.window),
      "{path}.window: expected 1s to 1h"
    );
    ensure!(
      (1..=1_048_576).contains(&counter.max_entries),
      "{path}.max_entries: expected 1 to 1048576"
    );
    ensure!(
      (1..=4).contains(&counter.key.len()),
      "{path}.key: expected 1 to 4 parts"
    );
    let mut seen = HashSet::new();
    let mut parts = Vec::new();
    for part in &counter.key {
      let extractor = match part {
        CounterKeyPart::Field(CounterField::ClientIp) => Extractor::Ip(CounterIp::default()),
        CounterKeyPart::ClientIp { client_ip } => Extractor::Ip(client_ip.clone()),
        CounterKeyPart::Header { header } => Extractor::Header(
          HeaderName::from_bytes(header.as_bytes())
            .with_context(|| format!("{path}.key: expected an HTTP header name"))?,
        ),
        CounterKeyPart::Field(field) => Extractor::Field(*field),
      };
      let identity = match &extractor {
        Extractor::Ip(prefix) => {
          ensure!(
            (1..=32).contains(&prefix.ipv4_prefix),
            "{path}.key.client_ip.ipv4_prefix: expected 1 to 32"
          );
          ensure!(
            (1..=128).contains(&prefix.ipv6_prefix),
            "{path}.key.client_ip.ipv6_prefix: expected 1 to 128"
          );
          "client_ip".to_string()
        }
        Extractor::Field(field) => {
          if *field == CounterField::Country {
            ensure!(
              geoip.country().is_some(),
              "{path}.key.country: requires policy.geoip.country"
            );
          }
          if *field == CounterField::Asn {
            ensure!(
              geoip.asn().is_some(),
              "{path}.key.asn: requires policy.geoip.asn"
            );
          }
          format!("{field:?}")
        }
        Extractor::Header(header) => format!("header:{header}"),
      };
      ensure!(seen.insert(identity), "{path}.key: duplicate part");
      parts.push(extractor);
    }
    budget.counter_selection = true;
    let count_when = counter
      .count_when
      .as_ref()
      .map(|condition| {
        Compiled::compile(
          condition,
          &format!("{path}.count_when"),
          0,
          budget,
          sets,
          geoip,
        )
      })
      .transpose()?;
    budget.counter_selection = false;
    let used = definition
      .rules
      .iter()
      .flatten()
      .any(|rule| rule.when.reads_counter(name));
    compiled.push(CompiledCounter {
      name: name.clone(),
      definition: counter.clone(),
      used,
      parts,
      count_when,
    });
  }
  Ok(compiled)
}
