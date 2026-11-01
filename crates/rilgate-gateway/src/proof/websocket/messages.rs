use rilgate_protocol::{StratumJob, difficulty as exact_difficulty};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerWsMessage {
  Job {
    job: StratumJob,
    #[serde(with = "exact_difficulty")]
    difficulty: u64,
    nonce_offset: usize,
  },
  Accepted {
    receipt: String,
  },
  Rejected {
    reason: String,
  },
  Error {
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
  },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientWsMessage {
  Share {
    job_id: String,
    nonce: String,
    result: String,
  },
}

pub(super) fn job(job: StratumJob, difficulty: u64, nonce_offset: usize) -> ServerWsMessage {
  ServerWsMessage::Job {
    job,
    difficulty,
    nonce_offset,
  }
}
