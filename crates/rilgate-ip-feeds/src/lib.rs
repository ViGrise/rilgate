mod fetch;
mod geoip;
mod parse;
mod refresh;
pub use fetch::{FeedClient, FetchOptions, Fetched};
pub use geoip::{GeoRefresher, MAXMIND_DOWNLOAD_BASE, spawn_geoip};
pub use parse::parse;
pub use refresh::{Outcome, next_delay, refresh_once, resume_delay, spawn};
pub use rilgate_policy::{GeoState, Validators};
use std::time::Duration;

/// Background refresh tasks. Dropping the handle stops them.
#[derive(Default)]
pub struct Refreshers {
  tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Refreshers {
  /// The number of running refresh tasks.
  pub fn len(&self) -> usize {
    self.tasks.len()
  }

  pub fn is_empty(&self) -> bool {
    self.tasks.is_empty()
  }

  /// Takes over the tasks of `other`.
  pub fn absorb(&mut self, mut other: Refreshers) {
    self.tasks.append(&mut other.tasks);
  }
}

impl Drop for Refreshers {
  fn drop(&mut self) {
    for task in &self.tasks {
      task.abort();
    }
  }
}

pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_REDIRECTS: usize = 3;
pub const FIRST_LOAD_RETRY: Duration = Duration::from_secs(15);
pub const FIRST_RETRY: Duration = Duration::from_secs(5 * 60);
pub const JITTER_FRACTION: f64 = 0.10;
