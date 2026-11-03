use crate::{PowAlgorithm, PowVerification};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowStep {
  pub algorithm: PowAlgorithm,
  pub difficulty: u64,
  #[serde(
    rename = "enforce",
    with = "crate::algorithm::verification::token_format"
  )]
  pub verification: PowVerification,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub fallback_for: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeStep {
  Pow(PowStep),
  Hook(HookStep),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowGrant {
  pub algorithm: PowAlgorithm,
  pub difficulty: u64,
  #[serde(
    rename = "enforce",
    with = "crate::algorithm::verification::token_format"
  )]
  pub verification: PowVerification,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub fallback_for: Option<String>,
  pub expires_at: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grant {
  Pow(PowGrant),
  Hook(HookGrant),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookStep {
  pub name: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookGrant {
  pub name: String,
  pub expires_at: i64,
}
impl Grant {
  pub fn expires_at(&self) -> i64 {
    match self {
      Self::Pow(g) => g.expires_at,
      Self::Hook(g) => g.expires_at,
    }
  }
  pub fn method_key(&self) -> String {
    match self {
      Self::Pow(step) => format!("pow:{}", algorithm_id(step.algorithm)),
      Self::Hook(step) => format!("hook:{}", step.name),
    }
  }
}
impl ChallengeStep {
  pub fn method_key(&self) -> String {
    match self {
      Self::Pow(step) => format!("pow:{}", algorithm_id(step.algorithm)),
      Self::Hook(step) => format!("hook:{}", step.name),
    }
  }
  pub fn summary(&self) -> String {
    match self {
      Self::Pow(step) => format!("{}:{}", algorithm_id(step.algorithm), step.difficulty),
      Self::Hook(step) => format!("hook:{}", step.name),
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
