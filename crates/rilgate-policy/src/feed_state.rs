use std::sync::{Mutex, MutexGuard};
use std::time::{Instant, SystemTime};

/// Conditional-request validators captured from a successful response.
#[derive(Debug, Clone, Default)]
pub struct Validators {
  pub etag: Option<http::HeaderValue>,
  pub last_modified: Option<http::HeaderValue>,
}

/// The change markers of the last database a source delivered.
#[derive(Debug, Clone, Default)]
pub struct GeoState {
  pub validators: Validators,
  pub modified: Option<SystemTime>,
}

/// What a refresher knows about its source between fetches.
///
/// It lives next to the loaded data, so a refresher that takes over the data
/// continues the schedule and the conditional requests of its predecessor.
#[derive(Debug, Clone, Default)]
pub struct Progress<Markers> {
  pub markers: Markers,
  pub consecutive_failures: u32,
  pub next_attempt: Option<Instant>,
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex
    .lock()
    .unwrap_or_else(std::sync::PoisonError::into_inner)
}
