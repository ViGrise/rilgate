mod format;
mod registry;
mod set;
pub use format::{Extracted, FeedFormat, quoted_value};
pub use registry::{FeedSets, SetDefinition};
pub use set::{FeedSet, FeedSetStatus, Membership};
use std::time::Duration;

pub const MAX_SETS_PER_REGISTRY: usize = 64;
pub const MAX_SET_NAME_BYTES: usize = 64;
pub const MAX_SETS_PER_MATCHER: usize = 16;
pub const MAX_ENTRIES_PER_SET: usize = 65_536;
pub const DEFAULT_REFRESH: Duration = Duration::from_secs(24 * 3600);
pub const MIN_REFRESH: Duration = Duration::from_secs(3600);
pub const MAX_REFRESH: Duration = Duration::from_secs(7 * 24 * 3600);
pub const DEFAULT_MAX_STALE: Duration = Duration::from_secs(7 * 24 * 3600);
pub const MAX_MAX_STALE: Duration = Duration::from_secs(30 * 24 * 3600);

/// The resolved remote-feed configuration of a feed set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFeed {
  pub url: String,
  pub format: FeedFormat,
  pub refresh: Duration,
  pub max_stale: Duration,
}

/// A membership index that a feed set publishes as one snapshot.
pub trait FeedIndex: std::fmt::Debug + Send + Sync + 'static {
  type Key: Copy;
  fn contains(&self, key: Self::Key) -> bool;
  fn entries(&self) -> usize;
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
