use crate::ChallengeConfig;
use anyhow::{Context, Result};
use http::HeaderMap;
use rilgate_policy::{PolicyAction, PolicyConfig, PolicyDecision, PolicyEngine};
use rilgate_pow::{AlgorithmDescriptor, AlgorithmRegistry};
use rilgate_protocol::{PowAlgorithm, PowEnforcement};
use std::sync::Arc;
pub struct ChallengeService {
  pub(crate) cfg: Arc<ChallengeConfig>,
  pub(crate) algorithms: AlgorithmRegistry,
  policy: PolicyEngine,
  pub(crate) tokens: crate::tokens::TokenSigners,
}

impl ChallengeService {
  pub fn with_registry(cfg: Arc<ChallengeConfig>, algorithms: AlgorithmRegistry) -> Result<Self> {
    cfg.validate()?;
    let policy = PolicyEngine::new(&PolicyConfig {
      bots: cfg.bots.clone(),
      thresholds: cfg.thresholds.clone(),
    })?;
    algorithms.get(cfg.default_algorithm)?;
    for (name, action, selection) in cfg
      .bots
      .iter()
      .map(|r| (&r.name, r.action, &r.challenge))
      .chain(
        cfg
          .thresholds
          .iter()
          .map(|r| (&r.name, r.action, &r.challenge)),
      )
    {
      if action != PolicyAction::Challenge {
        continue;
      }
      let backend = algorithms
        .get(selection.algorithm.unwrap_or(cfg.default_algorithm))
        .with_context(|| format!("Invalid challenge policy in {name}"))?;
      if let Some(difficulty) = selection.difficulty {
        backend
          .validate_difficulty(difficulty)
          .with_context(|| format!("Invalid challenge policy in {name}"))?;
      }
    }
    let tokens = crate::tokens::TokenSigners::new(&cfg)?;
    Ok(Self {
      tokens,
      cfg,
      algorithms,
      policy,
    })
  }

  pub fn algorithm_descriptors(&self) -> Vec<AlgorithmDescriptor> {
    self.algorithms.descriptors()
  }

  pub fn algorithm_descriptor(&self, id: PowAlgorithm) -> Result<AlgorithmDescriptor> {
    Ok(self.algorithms.get(id)?.descriptor())
  }

  pub fn enforcement(&self) -> PowEnforcement {
    self.cfg.enforce
  }

  pub fn evaluate_request(&self, headers: &HeaderMap, path: &str) -> PolicyDecision<'_> {
    let initial_weight = self.initial_weight(headers);
    let mut decision = self.policy.evaluate(headers, path, initial_weight);

    if decision.action == PolicyAction::Challenge {
      if decision.algorithm.is_none() {
        decision.algorithm = Some(self.cfg.default_algorithm);
      }

      if decision.difficulty.is_none() {
        let algorithm = decision.algorithm.unwrap_or(self.cfg.default_algorithm);
        decision.difficulty = self
          .algorithms
          .get(algorithm)
          .ok()
          .map(|backend| backend.difficulty_for_weight(decision.weight));
      }
    }

    decision
  }

  fn initial_weight(&self, headers: &HeaderMap) -> i32 {
    let risk = &self.cfg.risk;

    if risk.allow_debug_header
      && let Some(value) = headers.get(risk.debug_header.as_str())
      && let Ok(value) = value.to_str()
      && let Ok(weight) = value.parse::<i32>()
    {
      return weight;
    }

    risk.default_weight
  }

  pub fn difficulty_for_weight(&self, algorithm: PowAlgorithm, weight: i32) -> Result<u64> {
    Ok(
      self
        .algorithms
        .get(algorithm)?
        .difficulty_for_weight(weight),
    )
  }

  pub fn config(&self) -> &Arc<ChallengeConfig> {
    &self.cfg
  }
}
