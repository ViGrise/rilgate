use crate::feed_sets::*;
use crate::policy::schema::AsnSetDefinition;
use anyhow::{Result, ensure};
use std::collections::BTreeMap;
use std::time::Duration;
/// A sorted, deduplicated index of autonomous system numbers.
#[derive(Debug)]
pub struct AsnList {
  numbers: Vec<u32>,
}
impl AsnList {
  pub fn from_numbers(numbers: impl IntoIterator<Item = u32>) -> Self {
    let mut numbers: Vec<_> = numbers.into_iter().collect();
    numbers.sort_unstable();
    numbers.dedup();
    Self { numbers }
  }
}
impl FeedIndex for AsnList {
  type Key = u32;
  fn contains(&self, key: u32) -> bool {
    self.numbers.binary_search(&key).is_ok()
  }
  fn entries(&self) -> usize {
    self.numbers.len()
  }
}
pub type AsnSet = FeedSet<AsnList>;
pub type AsnSets = FeedSets<AsnList>;
impl AsnSets {
  pub fn compile_with(
    registry: &str,
    definitions: &BTreeMap<String, AsnSetDefinition>,
    previous: Option<&Self>,
  ) -> Result<Self> {
    Self::compile_registry(registry, definitions, previous)
  }
}
impl SetDefinition for AsnSetDefinition {
  type Index = AsnList;
  const STATIC_FIELD: &'static str = "asns";
  fn url(&self) -> Option<&str> {
    self.url.as_deref()
  }
  fn format(&self) -> Option<&FeedFormat> {
    self.format.as_ref()
  }
  fn refresh(&self) -> Option<Duration> {
    self.refresh
  }
  fn max_stale(&self) -> Option<Duration> {
    self.max_stale
  }
  fn static_index(&self, path: &str) -> Option<Result<AsnList>> {
    self.asns.as_ref().map(|values| {
      ensure!(
        !values.is_empty() && values.len() <= MAX_ENTRIES_PER_SET,
        "{path}.asns: expected 1 to 65536 autonomous system numbers"
      );
      for (i, number) in values.iter().enumerate() {
        ensure!(
          *number != 0,
          "{path}.asns[{i}]: expected an autonomous system number from 1 to 4294967295"
        );
      }
      Ok(AsnList::from_numbers(values.iter().copied()))
    })
  }
  fn check_format(format: &FeedFormat, path: &str) -> Result<()> {
    ensure!(
      !matches!(format, FeedFormat::IpPrefixesJson),
      "{path}: ip_prefixes_json holds IP networks, use list_text, regex or json_path"
    );
    Ok(())
  }
}
