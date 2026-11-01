use super::{FeedIndex, RemoteFeed};
use crate::feed_state::{Progress, Validators, lock};
use arc_swap::ArcSwap;
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
#[derive(Debug)]
struct Snapshot<I: FeedIndex> {
  index: Option<Arc<I>>,
  loaded_at: Option<Instant>,
}

/// A named membership index published as atomic snapshots.
#[derive(Debug)]
pub struct FeedSet<I: FeedIndex> {
  name: String,
  remote: Option<RemoteFeed>,
  shared: Arc<Shared<I>>,
}

/// The state that outlives a policy replacement when the source stays the same.
#[derive(Debug)]
struct Shared<I: FeedIndex> {
  state: ArcSwap<Snapshot<I>>,
  failures: AtomicU64,
  progress: Mutex<Progress<Validators>>,
}

impl<I: FeedIndex> FeedSet<I> {
  pub(crate) fn static_set(name: String, index: I) -> Self {
    Self {
      name,
      remote: None,
      shared: Arc::new(Shared {
        state: ArcSwap::from_pointee(Snapshot {
          index: Some(Arc::new(index)),
          loaded_at: Some(Instant::now()),
        }),
        ..Shared::default()
      }),
    }
  }

  pub(crate) fn remote_set(name: String, remote: RemoteFeed) -> Self {
    Self {
      name,
      remote: Some(remote),
      shared: Arc::new(Shared::default()),
    }
  }

  /// Takes over the loaded data and refresh progress of `previous` when both
  /// fetch the same feed. The refresh interval and staleness limit stay those
  /// of this set.
  pub(crate) fn adopt_state_of(mut self, previous: &FeedSet<I>) -> Self {
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

  pub fn remote(&self) -> Option<&RemoteFeed> {
    self.remote.as_ref()
  }

  /// Publishes a new membership snapshot stamped at the current instant.
  pub fn publish(&self, index: I) {
    self.shared.state.store(Arc::new(Snapshot {
      index: Some(Arc::new(index)),
      loaded_at: Some(Instant::now()),
    }));
  }

  /// Re-stamps the current snapshot after a not-modified response.
  pub fn renew(&self) {
    let snapshot = self.shared.state.load();
    if snapshot.index.is_some() {
      self.shared.state.store(Arc::new(Snapshot {
        index: snapshot.index.clone(),
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

  pub fn membership(&self, key: I::Key) -> Membership {
    self.membership_at(key, Instant::now())
  }

  pub fn membership_at(&self, key: I::Key, now: Instant) -> Membership {
    let snapshot = self.shared.state.load();
    let Some(index) = &snapshot.index else {
      return Membership::Unknown;
    };
    if self.is_stale(&snapshot, now) {
      return Membership::Unknown;
    }
    if index.contains(key) {
      Membership::Member
    } else {
      Membership::NotMember
    }
  }

  pub fn status(&self) -> FeedSetStatus {
    self.status_at(Instant::now())
  }

  pub fn status_at(&self, now: Instant) -> FeedSetStatus {
    let snapshot = self.shared.state.load();
    let loaded = snapshot.index.is_some() && snapshot.loaded_at.is_some();
    FeedSetStatus {
      loaded,
      stale: !loaded || self.is_stale(&snapshot, now),
      age: snapshot
        .loaded_at
        .map(|at| now.saturating_duration_since(at)),
      entries: snapshot.index.as_ref().map_or(0, |r| r.entries()),
      refresh_failures: self.shared.failures.load(Ordering::Relaxed),
    }
  }

  fn is_stale(&self, snapshot: &Snapshot<I>, now: Instant) -> bool {
    match (&self.remote, snapshot.loaded_at) {
      (Some(remote), Some(at)) => now.saturating_duration_since(at) > remote.max_stale,
      _ => false,
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedSetStatus {
  pub loaded: bool,
  pub stale: bool,
  pub age: Option<Duration>,
  pub entries: usize,
  pub refresh_failures: u64,
}

impl<I: FeedIndex> Default for Snapshot<I> {
  fn default() -> Self {
    Self {
      index: None,
      loaded_at: None,
    }
  }
}
impl<I: FeedIndex> Default for Shared<I> {
  fn default() -> Self {
    Self {
      state: ArcSwap::from_pointee(Snapshot::default()),
      failures: AtomicU64::new(0),
      progress: Mutex::new(Progress::default()),
    }
  }
}
