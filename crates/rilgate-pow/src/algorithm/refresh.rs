use super::{Algorithm, AlgorithmDescriptor, AlgorithmFuture, PowAlgorithm};
use crate::{PreparedWork, Proof, WorkRequest};
use anyhow::{Result, anyhow, ensure};

/// Longest delay a refresh challenge can request, in seconds.
pub const MAX_REFRESH_SECONDS: u64 = 10;

/// A challenge that waits for a delay and carries no work. The browser proves nothing, so the
/// registry entry exists to validate delays and to name the algorithm.
pub struct RefreshAlgorithm {
  ttl: u64,
}

impl RefreshAlgorithm {
  pub fn new(ttl: u64) -> Self {
    Self { ttl }
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
    }
  }

  fn difficulty_for_weight(&self, weight: i32) -> u64 {
    (weight / 10).clamp(1, MAX_REFRESH_SECONDS as i32) as u64
  }

  fn validate_difficulty(&self, difficulty: u64) -> Result<()> {
    ensure!(
      (1..=MAX_REFRESH_SECONDS).contains(&difficulty),
      "refresh difficulty must be between 1 and {MAX_REFRESH_SECONDS} seconds"
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
