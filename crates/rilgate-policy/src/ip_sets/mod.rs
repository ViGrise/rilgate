mod ranges;
mod registry;
mod set;
pub use ranges::IpRanges;
pub use registry::IpSets;
pub use set::{IpSet, IpSetStatus, Membership};
use std::time::Duration;

pub const MAX_IP_SETS: usize = 64;
pub const MAX_SET_NAME_BYTES: usize = 64;
pub const MAX_SETS_PER_MATCHER: usize = 16;
pub const MAX_PREFIXES_PER_SET: usize = 65_536;
pub const DEFAULT_REFRESH: Duration = Duration::from_secs(24 * 3600);
pub const MIN_REFRESH: Duration = Duration::from_secs(3600);
pub const MAX_REFRESH: Duration = Duration::from_secs(7 * 24 * 3600);
pub const DEFAULT_MAX_STALE: Duration = Duration::from_secs(7 * 24 * 3600);
pub const MAX_MAX_STALE: Duration = Duration::from_secs(30 * 24 * 3600);

/// The resolved remote-feed configuration of an IP set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteIpSet {
  pub url: String,
  pub format: crate::policy::schema::FeedFormat,
  pub refresh: Duration,
  pub max_stale: Duration,
}
