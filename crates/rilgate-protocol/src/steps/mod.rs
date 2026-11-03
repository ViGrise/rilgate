use crate::{PowAlgorithm, PowVerification};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowStep {
  pub algorithm: PowAlgorithm,
  pub difficulty: u64,
  #[serde(
    rename = "enforce",
    with = "crate::algorithm::verification::token_format"
  )]
  pub verification: PowVerification,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeStep {
  Pow(PowStep),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowGrant {
  pub algorithm: PowAlgorithm,
  pub difficulty: u64,
  #[serde(
    rename = "enforce",
    with = "crate::algorithm::verification::token_format"
  )]
  pub verification: PowVerification,
  pub expires_at: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grant {
  Pow(PowGrant),
}
impl Grant {
  pub fn method_key(&self) -> String {
    match self {
      Self::Pow(step) => format!("pow:{}", algorithm_id(step.algorithm)),
    }
  }
}
impl ChallengeStep {
  pub fn method_key(&self) -> String {
    match self {
      Self::Pow(step) => format!("pow:{}", algorithm_id(step.algorithm)),
    }
  }
  pub fn summary(&self) -> String {
    match self {
      Self::Pow(step) => format!("{}:{}", algorithm_id(step.algorithm), step.difficulty),
    }
  }
}

fn algorithm_id(algorithm: PowAlgorithm) -> &'static str {
  match algorithm {
    PowAlgorithm::Fast => "fast",
    PowAlgorithm::Hashwx => "hashwx",
    PowAlgorithm::RandomX => "randomx",
    PowAlgorithm::Refresh => "refresh",
  }
}
