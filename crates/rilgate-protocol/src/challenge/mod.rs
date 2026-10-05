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

  Randomx {
    challenge_token: String,
    job: StratumJob,
    #[serde(with = "difficulty")]
    difficulty: u64,
    nonce_offset: usize,
    expires_at: i64,
  },
}
