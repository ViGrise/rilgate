use super::{HashError, HasherKind, remote::UnavailableReason};
use rilgate_pow::algorithm::HashDiagnostics;
use std::{
  sync::atomic::{AtomicU64, Ordering},
  time::Instant,
};

#[derive(Default)]
pub(super) struct HashMetrics {
  outcomes: [[AtomicU64; 3]; 2],
  fallbacks: [[AtomicU64; 5]; 2],
  duration: [AtomicU64; 8],
  micros: AtomicU64,
}
fn index(kind: HasherKind) -> usize {
  match kind {
    HasherKind::Local => 0,
    HasherKind::Remote => 1,
  }
}
impl HashMetrics {
  pub fn observe(&self, kind: HasherKind, result: &Result<[u8; 32], HashError>) {
    let outcome = match result {
      Ok(_) => 0,
      Err(HashError::Unavailable(_)) => 1,
      Err(HashError::Invalid(_)) => 2,
    };
    self.outcomes[index(kind)][outcome].fetch_add(1, Ordering::Relaxed);
  }
  pub fn fallback(&self, kind: HasherKind, error: &anyhow::Error) {
    let reason = error
      .downcast_ref::<UnavailableReason>()
      .copied()
      .unwrap_or_else(|| {
        if error.is::<rilgate_core::error::CapacityExceeded>() {
          UnavailableReason::Overloaded
        } else {
          UnavailableReason::Connection
        }
      });
    let reason = match reason {
      UnavailableReason::Timeout => 0,
      UnavailableReason::Connection => 1,
      UnavailableReason::Overloaded => 2,
      UnavailableReason::Protocol => 3,
      UnavailableReason::Deadline => 4,
    };
    self.fallbacks[index(kind)][reason].fetch_add(1, Ordering::Relaxed);
  }
  pub fn start(&self, kind: HasherKind) -> Option<RemoteDuration<'_>> {
    (kind == HasherKind::Remote).then(|| RemoteDuration {
      metrics: self,
      started: Instant::now(),
    })
  }
  pub fn snapshot(&self) -> HashDiagnostics {
    HashDiagnostics {
      outcomes: std::array::from_fn(|i| {
        std::array::from_fn(|j| self.outcomes[i][j].load(Ordering::Relaxed))
      }),
      fallbacks: std::array::from_fn(|i| {
        std::array::from_fn(|j| self.fallbacks[i][j].load(Ordering::Relaxed))
      }),
      remote_duration: std::array::from_fn(|i| self.duration[i].load(Ordering::Relaxed)),
      remote_micros: self.micros.load(Ordering::Relaxed),
    }
  }
}
pub(super) struct RemoteDuration<'a> {
  metrics: &'a HashMetrics,
  started: Instant,
}
impl Drop for RemoteDuration<'_> {
  fn drop(&mut self) {
    let micros = self.started.elapsed().as_micros().min(u64::MAX as u128) as u64;
    let bucket = HashDiagnostics::DURATION_BOUNDS
      .iter()
      .position(|bound| micros <= *bound)
      .unwrap_or(7);
    self.metrics.duration[bucket].fetch_add(1, Ordering::Relaxed);
    self.metrics.micros.fetch_add(micros, Ordering::Relaxed);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use anyhow::anyhow;

  #[test]
  fn metrics_count_outcomes_and_bounded_fallback_reasons() {
    let metrics = HashMetrics::default();
    for kind in [HasherKind::Local, HasherKind::Remote] {
      metrics.observe(kind, &Ok([7; 32]));
      metrics.observe(kind, &Err(HashError::Invalid(anyhow!("invalid"))));
      for reason in [
        UnavailableReason::Timeout,
        UnavailableReason::Connection,
        UnavailableReason::Overloaded,
        UnavailableReason::Protocol,
        UnavailableReason::Deadline,
      ] {
        let error = anyhow!("unavailable").context(reason);
        metrics.fallback(kind, &error);
        metrics.observe(kind, &Err(HashError::Unavailable(error)));
      }
    }
    let duration = metrics.start(HasherKind::Remote);
    drop(duration);
    assert!(metrics.start(HasherKind::Local).is_none());
    let stats = metrics.snapshot();
    assert_eq!(stats.outcomes, [[1, 5, 1]; 2]);
    assert_eq!(stats.fallbacks, [[1; 5]; 2]);
    assert_eq!(stats.remote_duration.iter().sum::<u64>(), 1);
  }
}
