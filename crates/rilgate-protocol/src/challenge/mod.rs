use super::{StratumJob, difficulty};
use serde::Serialize;
#[derive(Debug, Serialize)]
#[serde(tag = "algorithm", rename_all = "snake_case")]
pub enum ChallengeResponse {
  Fast {
    challenge_token: String,
    challenge: String,
    difficulty: u64,
  },

  Hashwx {
    challenge_token: String,
    challenge: String,
    difficulty: u64,
    nonces_per_seed: u32,
    expires_at: i64,
    expires_in: u64,
  },

  Randomx {
    challenge_token: String,
    job: StratumJob,
    #[serde(with = "difficulty")]
    difficulty: u64,
    nonce_offset: usize,
    expires_at: i64,
    expires_in: u64,
  },
}
