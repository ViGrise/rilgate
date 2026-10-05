use rilgate_protocol::PowAlgorithm;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum PolicyAction {
  Allow,
  Deny,
  Challenge,
  Weigh,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeightAdjustment {
  #[serde(default)]
  pub adjust: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChallengePolicy {
  #[serde(default)]
  pub algorithm: Option<PowAlgorithm>,

  pub difficulty: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WeightExpression {
  Single(String),
  All { all: Vec<String> },
  Any { any: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotRuleConfig {
  pub name: String,

  #[serde(default)]
  pub path_regex: Option<String>,

  #[serde(default)]
  pub user_agent_regex: Option<String>,

  #[serde(default)]
  pub headers_regex: BTreeMap<String, String>,

  pub action: PolicyAction,

  #[serde(default)]
  pub weight: WeightAdjustment,

  #[serde(default)]
  pub challenge: ChallengePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdConfig {
  pub name: String,
  pub expression: WeightExpression,
  pub action: PolicyAction,

  #[serde(default)]
  pub challenge: ChallengePolicy,
}

#[derive(Debug, Clone)]
pub struct RiskConfig {
  pub default_weight: i32,
  pub allow_debug_header: bool,
  pub debug_header: String,
}

#[derive(Debug, Clone)]
pub struct PolicyConfig {
  pub bots: Vec<BotRuleConfig>,
  pub thresholds: Vec<ThresholdConfig>,
}
