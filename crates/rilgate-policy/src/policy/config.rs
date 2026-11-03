use rilgate_protocol::PowAlgorithm;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ChallengePolicy {
  #[serde(
    default,
    deserialize_with = "super::schema::present",
    skip_serializing_if = "Option::is_none"
  )]
  pub algorithm: Option<PowAlgorithm>,
  #[serde(
    default,
    deserialize_with = "super::schema::present",
    skip_serializing_if = "Option::is_none"
  )]
  pub difficulty: Option<u64>,
  #[serde(
    default,
    deserialize_with = "super::schema::present",
    skip_serializing_if = "Option::is_none"
  )]
  pub steps: Option<Vec<StepPolicy>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct StepPolicy {
  #[serde(
    default,
    deserialize_with = "super::schema::present",
    skip_serializing_if = "Option::is_none"
  )]
  pub algorithm: Option<PowAlgorithm>,
  #[serde(
    default,
    deserialize_with = "super::schema::present",
    skip_serializing_if = "Option::is_none"
  )]
  pub difficulty: Option<u64>,
}

impl ChallengePolicy {
  pub fn step_policies(&self) -> Vec<StepPolicy> {
    self.steps.clone().unwrap_or_else(|| {
      vec![StepPolicy {
        algorithm: self.algorithm,
        difficulty: self.difficulty,
      }]
    })
  }
  pub fn validate_steps(&self, default_algorithm: PowAlgorithm) -> anyhow::Result<()> {
    let steps = self.step_policies();
    let mut algorithms = std::collections::HashSet::new();
    for step in &steps {
      let algorithm = step.algorithm.unwrap_or(default_algorithm);
      anyhow::ensure!(
        algorithms.insert(algorithm),
        "challenge.steps: each algorithm may appear only once"
      );
      anyhow::ensure!(
        steps.len() < 2 || algorithm != PowAlgorithm::Refresh,
        "challenge.steps: refresh is valid only as a one-step challenge"
      );
    }
    Ok(())
  }
}
