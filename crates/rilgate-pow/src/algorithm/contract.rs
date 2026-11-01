use super::{AlgorithmDescriptor, PowAlgorithm};
use crate::{PreparedWork, Proof, WorkRequest};
use anyhow::Result;
pub type AlgorithmFuture<'a, T> = rilgate_core::BoxFuture<'a, T>;
#[derive(Default, Clone, Copy)]
pub struct VerifierDiagnostics {
  pub queued: usize,
  pub active: bool,
  pub completed: u64,
  pub rejected: u64,
}
#[derive(Default, Clone, Copy)]
pub struct HashDiagnostics {
  pub outcomes: [[u64; 3]; 2],
  pub fallbacks: [[u64; 5]; 2],
  pub remote_duration: [u64; 8],
  pub remote_micros: u64,
}
impl HashDiagnostics {
  pub const DURATION_BOUNDS: [u64; 7] = [1000, 5000, 10000, 50000, 100000, 1000000, 10000000];
}

pub trait Algorithm: Send + Sync {
  fn verifier_diagnostics(&self) -> Option<VerifierDiagnostics> {
    None
  }

  fn hash_diagnostics(&self) -> Option<HashDiagnostics> {
    None
  }

  fn remote_ready(&self, _: std::time::Duration) -> AlgorithmFuture<'_, bool> {
    Box::pin(async { Ok(false) })
  }

  fn descriptor(&self) -> AlgorithmDescriptor;
  fn difficulty_for_weight(&self, weight: i32) -> u64;
  fn validate_difficulty(&self, difficulty: u64) -> Result<()>;
  fn validate_verification(&self, _verification: rilgate_protocol::PowVerification) -> Result<()> {
    Ok(())
  }
  fn challenge_ttl(&self) -> u64;
  fn issue(&self, request: WorkRequest) -> AlgorithmFuture<'_, PreparedWork>;
  fn verify<'a>(&'a self, work: &'a PreparedWork, proof: Proof<'a>) -> AlgorithmFuture<'a, ()>;
  fn id(&self) -> PowAlgorithm {
    self.descriptor().id
  }
}
