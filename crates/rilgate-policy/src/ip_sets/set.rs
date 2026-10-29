use crate::feed_state::{Progress, Validators, lock};
use crate::ip_sets::{IpRanges, RemoteIpSet};
use arc_swap::ArcSwap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Membership {
  Member,
  NotMember,
  Unknown,
}

/// A snapshot of published membership observed as a single unit.
#[derive(Debug, Default)]
struct Snapshot {
  ranges: Option<Arc<IpRanges>>,
  loaded_at: Option<Instant>,
}

/// A named address set evaluated against the client address.
///
/// Static sets are loaded at construction. Remote sets start unknown and are
/// loaded by publishing refreshed snapshots. The snapshot is swapped
/// atomically, so evaluations racing a publish always observe one complete
/// snapshot.
#[derive(Debug)]
pub struct IpSet {
  name: String,
  remote: Option<RemoteIpSet>,
  shared: Arc<Shared>,
}

/// The state that outlives a policy replacement when the source stays the same.
#[derive(Debug, Default)]
struct Shared {
  state: ArcSwap<Snapshot>,
  failures: AtomicU64,
  progress: Mutex<Progress<Validators>>,
}

impl IpSet {
  pub(crate) fn static_set(name: String, ranges: IpRanges) -> Self {
    Self {
      name,
      remote: None,
      shared: Arc::new(Shared {
        state: ArcSwap::from_pointee(Snapshot {
          ranges: Some(Arc::new(ranges)),
          loaded_at: Some(Instant::now()),
        }),
        ..Shared::default()
      }),
    }
  }

  pub(crate) fn remote_set(name: String, remote: RemoteIpSet) -> Self {
    Self {
      name,
      remote: Some(remote),
      shared: Arc::new(Shared::default()),
    }
  }

  /// Takes over the loaded data and refresh progress of `previous` when both
  /// fetch the same feed. The refresh interval and staleness limit stay those
  /// of this set.
  pub(crate) fn adopt_state_of(mut self, previous: &IpSet) -> Self {
    if let (Some(current), Some(earlier)) = (&self.remote, &previous.remote)
      && current.url == earlier.url
      && current.format == earlier.format
    {
      self.shared = previous.shared.clone();
    }
    self
  }

  pub fn name(&self) -> &str {
    &self.name
  }

  pub fn remote(&self) -> Option<&RemoteIpSet> {
    self.remote.as_ref()
  }

  /// Publishes a new membership snapshot stamped at the current instant.
  pub fn publish(&self, ranges: IpRanges) {
    self.shared.state.store(Arc::new(Snapshot {
      ranges: Some(Arc::new(ranges)),
      loaded_at: Some(Instant::now()),
    }));
  }

  /// Re-stamps the current snapshot after a not-modified response.
  pub fn renew(&self) {
    let snapshot = self.shared.state.load();
    if snapshot.ranges.is_some() {
      self.shared.state.store(Arc::new(Snapshot {
        ranges: snapshot.ranges.clone(),
        loaded_at: Some(Instant::now()),
      }));
    }
  }

  /// The markers and schedule that the refresher of this set saved.
  pub fn progress(&self) -> Progress<Validators> {
    lock(&self.shared.progress).clone()
  }

  pub fn save_progress(&self, progress: Progress<Validators>) {
    *lock(&self.shared.progress) = progress;
  }

  pub fn record_failure(&self) {
    self.shared.failures.fetch_add(1, Ordering::Relaxed);
  }

  pub fn membership(&self, ip: IpAddr) -> Membership {
    self.membership_at(ip, Instant::now())
  }

  pub fn membership_at(&self, ip: IpAddr, now: Instant) -> Membership {
    let snapshot = self.shared.state.load();
    let Some(ranges) = &snapshot.ranges else {
      return Membership::Unknown;
    };
    if self.is_stale(&snapshot, now) {
      return Membership::Unknown;
    }
    if ranges.contains(ip) {
      Membership::Member
    } else {
      Membership::NotMember
    }
  }

  pub fn status(&self) -> IpSetStatus {
    self.status_at(Instant::now())
  }

  pub fn status_at(&self, now: Instant) -> IpSetStatus {
    let snapshot = self.shared.state.load();
    let loaded = snapshot.ranges.is_some() && snapshot.loaded_at.is_some();
    IpSetStatus {
      loaded,
      stale: !loaded || self.is_stale(&snapshot, now),
      age: snapshot
        .loaded_at
        .map(|at| now.saturating_duration_since(at)),
      prefixes: snapshot.ranges.as_ref().map_or(0, |r| r.prefix_count()),
      refresh_failures: self.shared.failures.load(Ordering::Relaxed),
    }
  }

  fn is_stale(&self, snapshot: &Snapshot, now: Instant) -> bool {
    match (&self.remote, snapshot.loaded_at) {
      (Some(remote), Some(at)) => now.saturating_duration_since(at) > remote.max_stale,
      _ => false,
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpSetStatus {
  pub loaded: bool,
  pub stale: bool,
  pub age: Option<Duration>,
  pub prefixes: usize,
  pub refresh_failures: u64,
}
