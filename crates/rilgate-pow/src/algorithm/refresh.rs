use super::{Algorithm, AlgorithmDescriptor, AlgorithmFuture, PowAlgorithm};
use crate::{PreparedWork, Proof, WorkRequest};
use anyhow::{Result, anyhow, ensure};

/// A challenge that waits for a delay and carries no work. The browser proves nothing, so the
/// registry entry exists to validate delays and to name the algorithm.
pub struct RefreshAlgorithm {
  ttl: u64,
  max: u64,
}

impl RefreshAlgorithm {
  /// `max` is the longest delay in seconds, for score-mapped and explicit delays.
  pub fn new(ttl: u64, max: u64) -> Self {
    Self { ttl, max }
  }
}

impl Algorithm for RefreshAlgorithm {
  fn descriptor(&self) -> AlgorithmDescriptor {
    AlgorithmDescriptor {
      id: PowAlgorithm::Refresh,
      version: 1,
      display_name: "Refresh",
      solver_module_path: String::new(),
      requires_consent: false,
      memory_bytes: 0,
      browser: None,
    }
  }

  fn difficulty_for_weight(&self, weight: i32) -> u64 {
    (weight / 10).clamp(1, i32::try_from(self.max).unwrap_or(i32::MAX).max(1)) as u64
  }

  fn validate_difficulty(&self, difficulty: u64) -> Result<()> {
    ensure!(
      (1..=self.max).contains(&difficulty),
      "refresh difficulty must be between 1 and {} seconds",
      self.max
    );
    Ok(())
  }

  fn challenge_ttl(&self) -> u64 {
    self.ttl
  }

  fn issue(&self, _: WorkRequest) -> AlgorithmFuture<'_, PreparedWork> {
    Box::pin(async { Err(anyhow!("Refresh challenges issue no work")) })
  }

  fn verify<'a>(&'a self, _: &'a PreparedWork, _: Proof<'a>) -> AlgorithmFuture<'a, ()> {
    Box::pin(async { Err(anyhow!("Refresh challenges accept no proof")) })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn score_mapped_delays_reach_the_configured_max() {
    let algorithm = RefreshAlgorithm::new(60, 30);
    assert_eq!(algorithm.difficulty_for_weight(-5), 1);
    assert_eq!(algorithm.difficulty_for_weight(250), 25);
    assert_eq!(algorithm.difficulty_for_weight(1000), 30);
  }
}
