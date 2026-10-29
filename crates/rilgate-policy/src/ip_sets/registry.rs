use crate::ip_sets::set::IpSet;
use crate::ip_sets::{
  DEFAULT_MAX_STALE, DEFAULT_REFRESH, IpRanges, MAX_IP_SETS, MAX_MAX_STALE, MAX_PREFIXES_PER_SET,
  MAX_REFRESH, MAX_SET_NAME_BYTES, MIN_REFRESH, RemoteIpSet,
};
use crate::policy::schema::IpSetDefinition;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

/// The compiled registry of named IP sets referenced by policy conditions.
#[derive(Debug, Default, Clone)]
pub struct IpSets {
  sets: BTreeMap<String, Arc<IpSet>>,
}

impl IpSets {
  pub fn compile(definitions: &BTreeMap<String, IpSetDefinition>) -> Result<Self> {
    ensure!(
      definitions.len() <= MAX_IP_SETS,
      "policy.ip_sets: at most {MAX_IP_SETS} sets are supported"
    );
    let mut sets = BTreeMap::new();
    for (name, definition) in definitions {
      let path = format!("policy.ip_sets.{name}");
      ensure!(
        valid_name(name),
        "{path}: name must contain 1 to {MAX_SET_NAME_BYTES} bytes of a-z, 0-9, - and _"
      );
      ensure!(
        definition.cidrs.is_some() ^ definition.url.is_some(),
        "{path}: expected exactly one of cidrs, url"
      );
      let set = if let Some(cidrs) = &definition.cidrs {
        static_set(&path, name, definition, cidrs)?
      } else {
        remote_set(&path, name, definition)?
      };
      sets.insert(name.clone(), Arc::new(set));
    }
    Ok(Self { sets })
  }

  pub fn get(&self, name: &str) -> Option<&Arc<IpSet>> {
    self.sets.get(name)
  }

  pub fn iter(&self) -> impl Iterator<Item = &Arc<IpSet>> {
    self.sets.values()
  }

  pub fn all_remote_loaded(&self) -> bool {
    self
      .sets
      .values()
      .filter(|set| set.remote().is_some())
      .all(|set| set.status().loaded)
  }
}

fn static_set(
  path: &str,
  name: &str,
  definition: &IpSetDefinition,
  cidrs: &[String],
) -> Result<IpSet> {
  ensure!(
    definition.format.is_none(),
    "{path}.format: only valid with url"
  );
  ensure!(
    definition.refresh.is_none(),
    "{path}.refresh: only valid with url"
  );
  ensure!(
    definition.max_stale.is_none(),
    "{path}.max_stale: only valid with url"
  );
  ensure!(
    !cidrs.is_empty() && cidrs.len() <= MAX_PREFIXES_PER_SET,
    "{path}.cidrs: expected 1 to {MAX_PREFIXES_PER_SET} networks"
  );
  let networks = cidrs
    .iter()
    .enumerate()
    .map(|(index, cidr)| {
      cidr
        .parse()
        .with_context(|| format!("{path}.cidrs[{index}]: expected an IPv4 or IPv6 network"))
    })
    .collect::<Result<Vec<_>>>()?;
  Ok(IpSet::static_set(
    name.to_string(),
    IpRanges::from_networks(networks),
  ))
}

fn remote_set(path: &str, name: &str, definition: &IpSetDefinition) -> Result<IpSet> {
  let url = definition.url.clone().expect("url present");
  let uri = url
    .parse::<http::Uri>()
    .with_context(|| format!("{path}.url: expected an absolute http or https URL"))?;
  ensure!(
    matches!(uri.scheme_str(), Some("http" | "https")) && uri.host().is_some(),
    "{path}.url: expected an absolute http or https URL"
  );
  let format = definition
    .format
    .with_context(|| format!("{path}.format: required with url"))?;
  let refresh = definition.refresh.unwrap_or(DEFAULT_REFRESH);
  ensure!(
    (MIN_REFRESH..=MAX_REFRESH).contains(&refresh),
    "{path}.refresh: expected {} to {}",
    describe(MIN_REFRESH),
    describe(MAX_REFRESH)
  );
  let max_stale = definition.max_stale.unwrap_or(DEFAULT_MAX_STALE);
  ensure!(
    max_stale >= refresh && max_stale <= MAX_MAX_STALE,
    "{path}.max_stale: expected refresh to {}",
    describe(MAX_MAX_STALE)
  );
  Ok(IpSet::remote_set(
    name.to_string(),
    RemoteIpSet {
      url,
      format,
      refresh,
      max_stale,
    },
  ))
}

pub(crate) fn describe(duration: Duration) -> String {
  let seconds = duration.as_secs();
  if seconds.is_multiple_of(86_400) {
    format!("{}d", seconds / 86_400)
  } else if seconds.is_multiple_of(3_600) {
    format!("{}h", seconds / 3_600)
  } else if seconds.is_multiple_of(60) {
    format!("{}m", seconds / 60)
  } else {
    format!("{seconds}s")
  }
}

fn valid_name(name: &str) -> bool {
  let bytes = name.as_bytes();
  !bytes.is_empty()
    && bytes.len() <= MAX_SET_NAME_BYTES
    && matches!(bytes[0], b'a'..=b'z' | b'0'..=b'9')
    && bytes
      .iter()
      .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'))
}
