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
  pub hook: Option<String>,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
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
  pub hook: Option<String>,
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
        hook: self.hook.clone(),
        difficulty: self.difficulty,
      }]
    })
  }
  pub fn validate_steps(
    &self,
    default_algorithm: PowAlgorithm,
    hooks: &std::collections::BTreeSet<String>,
  ) -> anyhow::Result<()> {
    let steps = self.step_policies();
    let mut methods = std::collections::HashSet::new();
    for step in &steps {
      if let Some(name) = &step.hook {
        anyhow::ensure!(
          hooks.contains(name),
          "challenge: unknown challenge hook {name}"
        );
        anyhow::ensure!(
          methods.insert(format!("hook:{name}")),
          "challenge.steps: each algorithm and hook may appear only once"
        );
        continue;
      }
      let algorithm = step.algorithm.unwrap_or(default_algorithm);
      anyhow::ensure!(
        methods.insert(format!("pow:{algorithm:?}")),
        "challenge.steps: each algorithm and hook may appear only once"
      );
      anyhow::ensure!(
        steps.len() < 2 || algorithm != PowAlgorithm::Refresh,
        "challenge.steps: refresh is valid only as a one-step challenge"
      );
    }
    Ok(())
  }
}
