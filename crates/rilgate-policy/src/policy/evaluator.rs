use anyhow::{Context, Result, anyhow};
use http::{HeaderMap, HeaderName};
use regex::Regex;

use crate::{
  BotRuleConfig, PolicyAction, PolicyConfig, PowAlgorithm, ThresholdConfig, WeightExpression,
};

#[derive(Debug, Clone)]
pub struct PolicyDecision<'a> {
  pub action: PolicyAction,
  pub weight: i32,
  pub algorithm: Option<PowAlgorithm>,
  pub difficulty: Option<u64>,
  pub rule: &'a str,
}

#[derive(Debug)]
struct CompiledBotRule {
  config: BotRuleConfig,
  path: Option<Regex>,
  user_agent: Option<Regex>,
  headers: Vec<(HeaderName, Regex)>,
}

#[derive(Debug)]
struct CompiledThreshold {
  config: ThresholdConfig,
  expression: CompiledWeightExpression,
}

#[derive(Debug)]
enum CompiledWeightExpression {
  Single(WeightComparison),
  All(Vec<WeightComparison>),
  Any(Vec<WeightComparison>),
}

#[derive(Debug)]
struct WeightComparison {
  op: ComparisonOperator,
  value: i32,
}

#[derive(Debug)]
enum ComparisonOperator {
  Lt,
  Le,
  Eq,
  Ne,
  Ge,
  Gt,
}

#[derive(Debug)]
pub struct PolicyEngine {
  rules: Vec<CompiledBotRule>,
  thresholds: Vec<CompiledThreshold>,
}

impl PolicyEngine {
  pub fn new(cfg: &PolicyConfig) -> Result<Self> {
    let rules = cfg
      .bots
      .iter()
      .cloned()
      .map(CompiledBotRule::compile)
      .collect::<Result<Vec<_>>>()?;

    let thresholds = cfg
      .thresholds
      .iter()
      .cloned()
      .map(CompiledThreshold::compile)
      .collect::<Result<Vec<_>>>()?;

    Ok(Self { rules, thresholds })
  }

  pub fn evaluate(
    &self,
    headers: &HeaderMap,
    path: &str,
    initial_weight: i32,
  ) -> PolicyDecision<'_> {
    let mut weight = initial_weight;

    for rule in &self.rules {
      if !rule.matches(headers, path) {
        continue;
      }

      match rule.config.action {
        PolicyAction::Weigh => {
          weight = weight.saturating_add(rule.config.weight.adjust);
        }

        PolicyAction::Allow | PolicyAction::Deny | PolicyAction::Challenge => {
          return PolicyDecision {
            action: rule.config.action,
            weight,
            algorithm: rule.config.challenge.algorithm,
            difficulty: rule.config.challenge.difficulty,
            rule: &rule.config.name,
          };
        }
      }
    }

    for threshold in &self.thresholds {
      if threshold.expression.matches(weight) {
        return PolicyDecision {
          action: threshold.config.action,
          weight,
          algorithm: threshold.config.challenge.algorithm,
          difficulty: threshold.config.challenge.difficulty,
          rule: &threshold.config.name,
        };
      }
    }

    // Requests are allowed when no terminal policy or threshold matches.
    PolicyDecision {
      action: PolicyAction::Allow,
      weight,
      algorithm: None,
      difficulty: None,
      rule: "default-allow",
    }
  }
}

impl CompiledBotRule {
  fn compile(config: BotRuleConfig) -> Result<Self> {
    let path = config
      .path_regex
      .as_deref()
      .map(Regex::new)
      .transpose()
      .with_context(|| format!("Invalid path_regex in policy {}", config.name))?;

    let user_agent = config
      .user_agent_regex
      .as_deref()
      .map(Regex::new)
      .transpose()
      .with_context(|| format!("Invalid user_agent_regex in policy {}", config.name))?;

    let headers = config
      .headers_regex
      .iter()
      .map(|(name, expression)| {
        let regex = Regex::new(expression)
          .with_context(|| format!("Invalid header regex {name:?} in policy {}", config.name))?;

        let name = HeaderName::from_bytes(name.as_bytes())
          .with_context(|| format!("Invalid policy header name {name:?}"))?;
        Ok((name, regex))
      })
      .collect::<Result<Vec<_>>>()?;

    Ok(Self {
      config,
      path,
      user_agent,
      headers,
    })
  }

  fn matches(&self, headers: &HeaderMap, path: &str) -> bool {
    if let Some(regex) = &self.path
      && !regex.is_match(path)
    {
      return false;
    }

    if let Some(regex) = &self.user_agent {
      let user_agent = headers
        .get(http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

      if !regex.is_match(user_agent) {
        return false;
      }
    }

    for (name, regex) in &self.headers {
      let value = headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

      if !regex.is_match(value) {
        return false;
      }
    }

    true
  }
}

impl CompiledThreshold {
  fn compile(config: ThresholdConfig) -> Result<Self> {
    if config.action == PolicyAction::Weigh {
      return Err(anyhow!(
        "Threshold {} cannot use WEIGH; WEIGH is only valid in bots rules",
        config.name
      ));
    }

    let expression = CompiledWeightExpression::compile(&config.expression)
      .with_context(|| format!("Invalid threshold expression in {}", config.name))?;

    Ok(Self { config, expression })
  }
}

impl CompiledWeightExpression {
  fn compile(expression: &WeightExpression) -> Result<Self> {
    match expression {
      WeightExpression::Single(value) => Ok(Self::Single(WeightComparison::parse(value)?)),

      WeightExpression::All { all } if all.is_empty() => {
        Err(anyhow!("all expressions cannot be empty"))
      }
      WeightExpression::Any { any } if any.is_empty() => {
        Err(anyhow!("any expressions cannot be empty"))
      }
      WeightExpression::All { all } => Ok(Self::All(
        all
          .iter()
          .map(|value| WeightComparison::parse(value))
          .collect::<Result<Vec<_>>>()?,
      )),

      WeightExpression::Any { any } => Ok(Self::Any(
        any
          .iter()
          .map(|value| WeightComparison::parse(value))
          .collect::<Result<Vec<_>>>()?,
      )),
    }
  }

  fn matches(&self, weight: i32) -> bool {
    match self {
      Self::Single(value) => value.matches(weight),
      Self::All(values) => values.iter().all(|value| value.matches(weight)),
      Self::Any(values) => values.iter().any(|value| value.matches(weight)),
    }
  }
}

impl WeightComparison {
  fn parse(expression: &str) -> Result<Self> {
    let regex = Regex::new(r"^\s*weight\s*(<=|>=|==|!=|<|>)\s*(-?\d+)\s*$")
      .expect("weight expression parser regex must compile");

    let captures = regex.captures(expression).ok_or_else(|| {
      anyhow!("Unsupported expression {expression:?}; expected e.g. `weight >= 10`")
    })?;

    let op = match &captures[1] {
      "<" => ComparisonOperator::Lt,
      "<=" => ComparisonOperator::Le,
      "==" => ComparisonOperator::Eq,
      "!=" => ComparisonOperator::Ne,
      ">=" => ComparisonOperator::Ge,
      ">" => ComparisonOperator::Gt,
      _ => unreachable!(),
    };

    let value = captures[2].parse::<i32>()?;

    Ok(Self { op, value })
  }

  fn matches(&self, weight: i32) -> bool {
    match self.op {
      ComparisonOperator::Lt => weight < self.value,
      ComparisonOperator::Le => weight <= self.value,
      ComparisonOperator::Eq => weight == self.value,
      ComparisonOperator::Ne => weight != self.value,
      ComparisonOperator::Ge => weight >= self.value,
      ComparisonOperator::Gt => weight > self.value,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{ChallengePolicy, PolicyAction, PowAlgorithm, ThresholdConfig, WeightExpression};

  #[test]
  fn decisions_preserve_rule_names_weights_and_challenge_settings() {
    let cfg = PolicyConfig {
      bots: vec![
        serde_yaml_ng::from_str("name: weigh\naction: WEIGH\nweight:\n  adjust: 10").unwrap(),
        serde_yaml_ng::from_str(
          "name: terminal\npath_regex: ^/terminal$\naction: CHALLENGE\nchallenge:\n  algorithm: fast\n  difficulty: 2",
        ).unwrap(),
      ],
      thresholds: vec![ThresholdConfig {
        name: "threshold".into(),
        expression: WeightExpression::Single("weight >= 20".into()),
        action: PolicyAction::Deny,
        challenge: ChallengePolicy::default(),
      }],
    };
    let engine = PolicyEngine::new(&cfg).unwrap();
    drop(cfg);
    for (path, initial, name, action, weight) in [
      ("/terminal", 10, "terminal", PolicyAction::Challenge, 20),
      ("/other", 10, "threshold", PolicyAction::Deny, 20),
      ("/other", 0, "default-allow", PolicyAction::Allow, 10),
      (
        "/other",
        i32::MAX,
        "threshold",
        PolicyAction::Deny,
        i32::MAX,
      ),
    ] {
      let decision = engine.evaluate(&HeaderMap::new(), path, initial);
      assert_eq!(decision.rule, name);
      assert_eq!(decision.action, action);
      assert_eq!(decision.weight, weight);
      if action == PolicyAction::Challenge {
        assert_eq!(decision.algorithm, Some(PowAlgorithm::Fast));
        assert_eq!(decision.difficulty, Some(2));
      }
    }
  }

  #[test]
  fn parses_all_weight_expression() {
    let threshold = CompiledThreshold::compile(ThresholdConfig {
      name: "moderate".to_string(),
      expression: WeightExpression::All {
        all: vec!["weight >= 10".to_string(), "weight < 20".to_string()],
      },
      action: PolicyAction::Challenge,
      challenge: ChallengePolicy {
        algorithm: Some(PowAlgorithm::RandomX),
        difficulty: Some(2_000),
      },
    })
    .unwrap();

    assert!(threshold.expression.matches(10));
    assert!(threshold.expression.matches(19));
    assert!(!threshold.expression.matches(20));
  }

  #[test]
  fn a_threshold_difficulty_written_outside_challenge_is_rejected() {
    // Writing `difficulty` beside `challenge` rather than inside it used to be
    // accepted and then ignored, leaving the threshold to fall back to a
    // difficulty derived from weight.
    let misplaced = serde_yaml_ng::from_str::<ThresholdConfig>(
      r#"
name: moderate
expression: weight >= 10
action: CHALLENGE
difficulty: 2
challenge:
  algorithm: fast
"#,
    )
    .expect_err("difficulty beside challenge must be rejected");
    assert!(
      misplaced.to_string().contains("difficulty"),
      "unexpected error: {misplaced}"
    );

    let correct: ThresholdConfig = serde_yaml_ng::from_str(
      r#"
name: moderate
expression: weight >= 10
action: CHALLENGE
challenge:
  algorithm: fast
  difficulty: 2
"#,
    )
    .expect("nested difficulty loads");
    assert_eq!(correct.challenge.difficulty, Some(2));
  }
}
