use rilgate_protocol::{PowAlgorithm, PowEnforcement, StratumJob};
#[derive(Debug, Clone)]
pub struct WorkRequest {
  pub difficulty: u64,
  pub enforce: PowEnforcement,
}
#[derive(Debug, Clone)]
pub enum Work {
  Fast {
    challenge: String,
    difficulty: u64,
  },
  Randomx {
    job: StratumJob,
    difficulty: u64,
    nonce_offset: usize,
  },
}
#[derive(Debug, Clone)]
pub struct PreparedWork {
  pub challenge_id: String,
  pub issued_at: i64,
  pub expires_at: i64,
  pub work: Work,
}
impl PreparedWork {
  pub fn algorithm(&self) -> PowAlgorithm {
    match self.work {
      Work::Fast { .. } => PowAlgorithm::Fast,
      Work::Randomx { .. } => PowAlgorithm::RandomX,
    }
  }
}
#[derive(Debug, Clone, Copy)]
pub struct Proof<'a> {
  pub nonce: &'a str,
}
