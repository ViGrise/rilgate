use crate::feed_sets::*;
use crate::policy::schema::IpSetDefinition;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use std::time::Duration;
mod ranges;
pub use crate::feed_sets::Membership;
pub use ranges::IpRanges;
pub const MAX_PREFIXES_PER_SET: usize = MAX_ENTRIES_PER_SET;
pub type IpSet = FeedSet<IpRanges>;
pub type IpSets = FeedSets<IpRanges>;
impl IpSets {
  pub fn compile(definitions: &BTreeMap<String, IpSetDefinition>) -> Result<Self> {
    Self::compile_with(definitions, None)
  }
  pub fn compile_with(
    definitions: &BTreeMap<String, IpSetDefinition>,
    previous: Option<&Self>,
  ) -> Result<Self> {
    Self::compile_registry("signals.ip_sets", definitions, previous)
  }
}
impl SetDefinition for IpSetDefinition {
  type Index = IpRanges;
  const STATIC_FIELD: &'static str = "cidrs";
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
  fn static_index(&self, path: &str) -> Option<Result<IpRanges>> {
    self.cidrs.as_ref().map(|values| {
      ensure!(
        !values.is_empty() && values.len() <= MAX_ENTRIES_PER_SET,
        "{path}.cidrs: expected 1 to 65536 networks"
      );
      let networks = values
        .iter()
        .enumerate()
        .map(|(i, value)| {
          value
            .parse()
            .with_context(|| format!("{path}.cidrs[{i}]: expected an IPv4 or IPv6 network"))
        })
        .collect::<Result<Vec<_>>>()?;
      Ok(IpRanges::from_networks(networks))
    })
  }
  fn check_format(_format: &FeedFormat, _path: &str) -> Result<()> {
    Ok(())
  }
}
