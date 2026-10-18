use super::schema::{Action, Definition, Mode, Rule};
use crate::conditions::{Budget, Compiled, Request, Truth};
use crate::ip_sets::IpSets;
use crate::{ChallengePolicy, PolicyAction, PowAlgorithm};
use anyhow::{Context, Result, ensure};
use http::{HeaderMap, Method};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct PolicyDecision<'a> {
  pub action: PolicyAction,
  pub weight: i32,
  pub algorithm: Option<PowAlgorithm>,
  pub difficulty: Option<u64>,
  pub rule: &'a str,
  pub unresolved: bool,
}
#[derive(Debug)]
struct CompiledRule {
  rule: Rule,
  condition: Compiled,
}
#[derive(Debug)]
pub struct PolicyEngine {
  rules: Vec<CompiledRule>,
  default_action: PolicyAction,
  ip_sets: IpSets,
}

impl Definition {
  pub fn resolved_rules(&self) -> Result<(Vec<Rule>, Action)> {
    if self.mode != Mode::Custom {
      ensure!(
        self.ip_sets.is_empty(),
        "policy: ip_sets require mode: custom"
      );
      ensure!(
        self.rules.is_none() && self.score.is_none() && self.default_action.is_none(),
        "policy: rules, score and default_action require mode: custom"
      );
      return Ok(super::presets::rules(self.mode));
    }
    let default = self.default_action.unwrap_or(Action::Allow);
    ensure!(
      default != Action::Score,
      "policy.default_action: score is not terminal"
    );
    let mut rules = self.rules.clone().unwrap_or_default();
    if let Some(score) = &self.score {
      http::HeaderName::from_bytes(score.debug_header.name.as_bytes())
        .context("policy.score.debug_header.name: expected an HTTP header name")?;
      for pair in score.thresholds.windows(2) {
        ensure!(
          pair[0].min > pair[1].min,
          "policy.score.thresholds: min values must be unique and strictly descending"
        );
      }
      for threshold in &score.thresholds {
        ensure!(
          threshold.action != Action::Score,
          "policy.score.thresholds: score is not terminal"
        );
        rules.push(Rule {
          name: format!("score >= {}", threshold.min),
          when: super::schema::Condition {
            score: Some(super::schema::NumberMatch {
              gte: Some(threshold.min),
              ..Default::default()
            }),
            ..Default::default()
          },
          action: threshold.action,
          score: None,
          challenge: threshold.challenge.clone(),
        });
      }
    }
    Ok((rules, default))
  }
  pub fn challenges(&self) -> Result<Vec<(String, ChallengePolicy)>> {
    let (rules, default) = self.resolved_rules()?;
    let mut choices: Vec<_> = rules
      .into_iter()
      .filter(|r| r.action == Action::Challenge)
      .map(|r| (r.name, r.challenge.unwrap_or_default()))
      .collect();
    let remote: BTreeSet<&str> = self
      .ip_sets
      .iter()
      .filter(|(_, definition)| definition.url.is_some())
      .map(|(name, _)| name.as_str())
      .collect();
    if !remote.is_empty() {
      for rule in self.rules.iter().flatten() {
        if rule.action == Action::Deny && rule.when.references(&remote) {
          choices.push((
            format!("{} (unknown ip set)", rule.name),
            ChallengePolicy::default(),
          ));
        }
      }
    }
    if default == Action::Challenge {
      choices.push(("policy.default_action".into(), ChallengePolicy::default()));
    }
    Ok(choices)
  }
}
impl PolicyEngine {
  pub fn from_definition(definition: &Definition) -> Result<Self> {
    let (source, default) = definition.resolved_rules()?;
    let ip_sets = IpSets::compile(&definition.ip_sets)?;
    ensure!(
      source.len() <= 128,
      "policy: at most 128 rules and thresholds are supported"
    );
    let mut names = std::collections::HashSet::new();
    let mut budget = Budget::default();
    let mut rules = Vec::new();
    for (index, rule) in source.into_iter().enumerate() {
      let explicit = definition.rules.as_ref().map_or(0, Vec::len);
      let path = if definition.mode != Mode::Custom {
        "policy.mode".to_string()
      } else if index < explicit {
        format!("policy.rules[{index}]")
      } else {
        format!("policy.score.thresholds[{}]", index - explicit)
      };
      ensure!(
        !rule.name.is_empty() && rule.name.len() <= 128 && names.insert(rule.name.clone()),
        "{path}: name must be unique and contain 1 to 128 bytes"
      );
      ensure!(
        rule.score.is_some() == (rule.action == Action::Score),
        "{path}.score: required only for action: score"
      );
      ensure!(
        rule.challenge.is_none() || rule.action == Action::Challenge,
        "{path}.challenge: only valid for action: challenge"
      );
      let condition = Compiled::compile(
        &rule.when,
        &format!("{path}.when"),
        0,
        &mut budget,
        &ip_sets,
      )?;
      rules.push(CompiledRule { rule, condition });
    }
    Ok(Self {
      rules,
      default_action: default,
      ip_sets,
    })
  }
  pub fn ip_sets(&self) -> &IpSets {
    &self.ip_sets
  }
  pub fn evaluate(
    &self,
    headers: &HeaderMap,
    path: &str,
    initial_weight: i32,
  ) -> PolicyDecision<'_> {
    self.evaluate_context(
      &Request {
        headers,
        path,
        method: &Method::GET,
        client_ip: None,
      },
      initial_weight,
    )
  }
  pub fn evaluate_context(&self, request: &Request<'_>, mut weight: i32) -> PolicyDecision<'_> {
    // A decision is unresolved when any rule evaluated on the way to it,
    // including the deciding rule, depended on unavailable IP set data.
    let mut unresolved = false;
    for entry in &self.rules {
      let truth = entry.condition.matches(request, weight);
      unresolved |= truth == Truth::Unknown;
      match (entry.rule.action, truth) {
        (_, Truth::False) => {}
        (Action::Allow, Truth::True) => {
          return self.decision(entry, PolicyAction::Allow, weight, unresolved);
        }
        (Action::Allow, Truth::Unknown) => {}
        (Action::Deny, Truth::True) => {
          return self.decision(entry, PolicyAction::Deny, weight, unresolved);
        }
        (Action::Deny, Truth::Unknown) => {
          // Unavailable data degrades a deny rule to a challenge, bounding the
          // impact of a feed outage on a denylist.
          return PolicyDecision {
            action: PolicyAction::Challenge,
            weight,
            algorithm: None,
            difficulty: None,
            rule: &entry.rule.name,
            unresolved,
          };
        }
        (Action::Challenge, _) => {
          return self.decision(entry, PolicyAction::Challenge, weight, unresolved);
        }
        (Action::Score, _) => {
          let score = entry.rule.score.unwrap_or(0);
          if truth == Truth::True || score > 0 {
            weight = weight.saturating_add(score);
          }
        }
      }
    }
    PolicyDecision {
      action: self.default_action,
      weight,
      algorithm: None,
      difficulty: None,
      rule: match self.default_action {
        PolicyAction::Allow => "default-allow",
        PolicyAction::Deny => "default-deny",
        _ => "default-challenge",
      },
      unresolved,
    }
  }

  fn decision<'a>(
    &self,
    entry: &'a CompiledRule,
    action: PolicyAction,
    weight: i32,
    unresolved: bool,
  ) -> PolicyDecision<'a> {
    let selection = entry.rule.challenge.as_ref();
    PolicyDecision {
      action,
      weight,
      algorithm: selection.and_then(|c| c.algorithm),
      difficulty: selection.and_then(|c| c.difficulty),
      rule: &entry.rule.name,
      unresolved,
    }
  }
}
