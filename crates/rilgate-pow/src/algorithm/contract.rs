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
pub trait Algorithm: Send + Sync {
  fn verifier_diagnostics(&self) -> Option<VerifierDiagnostics> {
    None
  }

  fn descriptor(&self) -> AlgorithmDescriptor;
  fn difficulty_for_weight(&self, weight: i32) -> u64;
  fn validate_difficulty(&self, difficulty: u64) -> Result<()>;
  fn validate_enforcement(&self, _enforce: rilgate_protocol::PowEnforcement) -> Result<()> {
    Ok(())
  }
  fn challenge_ttl(&self) -> u64;
  fn issue(&self, request: WorkRequest) -> AlgorithmFuture<'_, PreparedWork>;
  fn verify<'a>(&'a self, work: &'a PreparedWork, proof: Proof<'a>) -> AlgorithmFuture<'a, ()>;
  fn id(&self) -> PowAlgorithm {
    self.descriptor().id
  }
}
