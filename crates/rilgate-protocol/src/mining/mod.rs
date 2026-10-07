use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StratumJob {
  pub blob: String,
  pub job_id: String,
  pub target: String,
  pub seed_hash: String,
  pub height: Option<u64>,
  pub algo: Option<String>,
}
