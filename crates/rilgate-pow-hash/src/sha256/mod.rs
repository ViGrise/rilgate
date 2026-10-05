pub mod verifier;
use anyhow::{Result, anyhow};
use rilgate_core::time::unix_now;
use rilgate_pow::{
  Algorithm, AlgorithmDescriptor, AlgorithmFuture, PowAlgorithm, PreparedWork, Proof, Work,
  WorkRequest,
};
use uuid::Uuid;
pub struct FastAlgorithm {
  ttl: u64,
}
#[derive(Debug, Clone)]
pub struct FastConfig {
  pub ttl: u64,
}
impl FastAlgorithm {
  pub fn new(config: &FastConfig) -> Self {
    Self { ttl: config.ttl }
  }
}
impl Algorithm for FastAlgorithm {
  fn descriptor(&self) -> AlgorithmDescriptor {
    AlgorithmDescriptor {
      id: PowAlgorithm::Fast,
      version: 1,
      display_name: "SHA-256",
      solver_module_path: "/static/js/solvers/fast/solver.js",
      requires_consent: false,
      memory_bytes: 0,
    }
  }
  fn difficulty_for_weight(&self, weight: i32) -> u64 {
    if weight < 10 {
      1
    } else {
      (2_u64.saturating_mul((weight / 10) as u64)).clamp(1, 64)
    }
  }
  fn validate_difficulty(&self, difficulty: u64) -> Result<()> {
    anyhow::ensure!(
      (1..=64).contains(&difficulty),
      "fast difficulty must be between 1 and 64 leading hexadecimal zeroes"
    );
    Ok(())
  }
  fn challenge_ttl(&self) -> u64 {
    self.ttl
  }
  fn issue(&self, request: WorkRequest) -> AlgorithmFuture<'_, PreparedWork> {
    Box::pin(async move {
      self.validate_difficulty(request.difficulty)?;
      let now = unix_now()?;
      Ok(PreparedWork {
        challenge_id: Uuid::new_v4().to_string(),
        issued_at: now,
        expires_at: now + self.ttl as i64,
        work: Work::Fast {
          challenge: Uuid::new_v4().simple().to_string(),
          difficulty: request.difficulty,
        },
      })
    })
  }
  fn verify<'a>(&'a self, work: &'a PreparedWork, proof: Proof<'a>) -> AlgorithmFuture<'a, ()> {
    Box::pin(async move {
      let Work::Fast {
        challenge,
        difficulty,
      } = &work.work
      else {
        return Err(anyhow!("Expected fast work"));
      };
      self.validate_difficulty(*difficulty)?;
      anyhow::ensure!(
        verifier::verify_fast(challenge, proof.nonce, *difficulty)?,
        "SHA-256 proof does not meet the required difficulty"
      );
      Ok(())
    })
  }
}
