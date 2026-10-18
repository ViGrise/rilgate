use crate::ip_sets::set::IpSet;
use crate::ip_sets::{IpRanges, MAX_IP_SETS, MAX_PREFIXES_PER_SET, MAX_SET_NAME_BYTES};
use crate::policy::schema::IpSetDefinition;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use std::sync::Arc;

/// The compiled registry of named IP sets referenced by policy conditions.
#[derive(Debug, Default)]
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
        definition.cidrs.is_some(),
        "{path}: expected exactly one of cidrs, url"
      );
      let cidrs = definition.cidrs.as_ref().expect("checked above");
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
      let ranges = IpRanges::from_networks(networks);
      sets.insert(
        name.clone(),
        Arc::new(IpSet::static_set(name.clone(), ranges)),
      );
    }
    Ok(Self { sets })
  }

  pub fn get(&self, name: &str) -> Option<&Arc<IpSet>> {
    self.sets.get(name)
  }

  pub fn iter(&self) -> impl Iterator<Item = &Arc<IpSet>> {
    self.sets.values()
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
