use crate::*;
pub(super) fn default_thresholds() -> Vec<ThresholdConfig> {
  vec![
    ThresholdConfig {
      name: "minimal-suspicion".to_string(),
      expression: WeightExpression::Single("weight <= 0".to_string()),
      action: PolicyAction::Allow,
      challenge: ChallengePolicy::default(),
    },
    ThresholdConfig {
      name: "mild-suspicion".to_string(),
      expression: WeightExpression::All {
        all: vec!["weight > 0".to_string(), "weight < 10".to_string()],
      },
      action: PolicyAction::Challenge,
      challenge: ChallengePolicy {
        algorithm: Some(PowAlgorithm::Fast),
        difficulty: Some(1),
      },
    },
    ThresholdConfig {
      name: "moderate-suspicion".to_string(),
      expression: WeightExpression::All {
        all: vec!["weight >= 10".to_string(), "weight < 20".to_string()],
      },
      action: PolicyAction::Challenge,
      challenge: ChallengePolicy {
        algorithm: Some(PowAlgorithm::Fast),
        difficulty: Some(2),
      },
    },
    ThresholdConfig {
      name: "high-suspicion".to_string(),
      expression: WeightExpression::All {
        all: vec!["weight >= 20".to_string(), "weight < 30".to_string()],
      },
      action: PolicyAction::Challenge,
      challenge: ChallengePolicy {
        algorithm: Some(PowAlgorithm::Fast),
        difficulty: Some(4),
      },
    },
    ThresholdConfig {
      name: "extreme-suspicion".to_string(),
      expression: WeightExpression::Single("weight >= 30".to_string()),
      action: PolicyAction::Challenge,
      challenge: ChallengePolicy {
        algorithm: Some(PowAlgorithm::RandomX),
        difficulty: Some(8_000),
      },
    },
  ]
}
