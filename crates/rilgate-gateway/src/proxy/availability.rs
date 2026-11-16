//! Upstream availability events for the default log level.
//!
//! A failed proxied request logs a warning, at most once per [`WARNING_INTERVAL`], with the
//! failures since the last success. The first success after a warning logs one info event.

use std::{
  sync::atomic::{AtomicBool, AtomicU64, Ordering},
  time::{Duration, Instant},
};

const WARNING_INTERVAL: Duration = Duration::from_secs(10);

pub(crate) struct UpstreamAvailability {
  started: Instant,
  failures: AtomicU64,
  warned: AtomicBool,
  /// Milliseconds after `started` plus one, so zero means that no warning was logged.
  last_warning_ms: AtomicU64,
}

impl Default for UpstreamAvailability {
  fn default() -> Self {
    Self {
      started: Instant::now(),
      failures: AtomicU64::new(0),
      warned: AtomicBool::new(false),
      last_warning_ms: AtomicU64::new(0),
    }
  }
}

impl UpstreamAvailability {
  pub(crate) fn record_success(&self) {
    if self.failures.load(Ordering::Relaxed) == 0 {
      return;
    }
    let failures = self.failures.swap(0, Ordering::Relaxed);
    if self.warned.swap(false, Ordering::Relaxed) {
      tracing::info!(failures, "Upstream requests succeed again");
    }
  }

  pub(crate) fn record_failure(&self, status: u16, error: &pingora::Error) {
    let failures = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
    let now = self.started.elapsed().as_millis() as u64 + 1;
    let last = self.last_warning_ms.load(Ordering::Relaxed);
    if (last == 0 || now.saturating_sub(last) >= WARNING_INTERVAL.as_millis() as u64)
      && self
        .last_warning_ms
        .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
      self.warned.store(true, Ordering::Relaxed);
      tracing::warn!(failures, status, error = %error, "Upstream requests are failing");
    }
  }
}
