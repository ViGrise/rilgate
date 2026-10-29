use crate::{ChallengeConfig, authorization::binding::AuthBinding};
use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use http::HeaderMap;
use rilgate_policy::{PolicyAction, PolicyDecision, PolicyEngine, policy::schema::Definition};
use rilgate_pow::{AlgorithmDescriptor, AlgorithmRegistry};
use rilgate_protocol::{PowAlgorithm, PowEnforcement};
use std::sync::Arc;
pub struct ChallengeService {
  pub(crate) cfg: Arc<ChallengeConfig>,
  pub(crate) algorithms: AlgorithmRegistry,
  policy: ArcSwap<CompiledPolicy>,
  pub(crate) tokens: crate::tokens::TokenSigners,
  pub(crate) binding_key: [u8; 32],
}

/// A policy that passed every check against the service's algorithms and is ready to install.
pub struct CompiledPolicy {
  engine: PolicyEngine,
  initial_weight: i32,
  debug_header: Option<String>,
}

impl CompiledPolicy {
  pub fn engine(&self) -> &PolicyEngine {
    &self.engine
  }
}

impl ChallengeService {
  pub fn with_registry(cfg: Arc<ChallengeConfig>, algorithms: AlgorithmRegistry) -> Result<Self> {
    cfg.validate()?;
    algorithms.get(cfg.default_algorithm)?;
    let policy = Self::compile_policy(&cfg, &algorithms, &cfg.policy)?;
    let tokens = crate::tokens::TokenSigners::new(&cfg)?;
    let binding_key = AuthBinding::derive_key(cfg.auth_signing_key.expose().as_bytes())?;
    Ok(Self {
      tokens,
      binding_key,
      cfg,
      algorithms,
      policy: ArcSwap::from_pointee(policy),
    })
  }

  /// Compiles a policy and checks its challenges against the registered algorithms.
  pub fn compile(&self, definition: &Definition) -> Result<CompiledPolicy> {
    Self::compile_policy(&self.cfg, &self.algorithms, definition)
  }

  /// Installs a compiled policy. Requests that start afterwards use it.
  pub fn install(&self, policy: CompiledPolicy) {
    self.policy.store(Arc::new(policy));
  }

  /// Compiles the definition and installs it. The running policy stays when a check fails.
  pub fn replace_policy(&self, definition: &Definition) -> Result<()> {
    self.install(self.compile(definition)?);
    Ok(())
  }

  fn compile_policy(
    cfg: &ChallengeConfig,
    algorithms: &AlgorithmRegistry,
    definition: &Definition,
  ) -> Result<CompiledPolicy> {
    let engine = PolicyEngine::from_definition(definition)?;
    for (name, selection) in definition.challenges()? {
      let backend = algorithms
        .get(selection.algorithm.unwrap_or(cfg.default_algorithm))
        .with_context(|| format!("Invalid challenge policy in {name}"))?;
      anyhow::ensure!(
        cfg.enforce != PowEnforcement::StratumPoolAccepted || backend.id() == PowAlgorithm::RandomX,
        "policy ({name}): stratum_pool_accepted requires RandomX challenges"
      );
      if let Some(difficulty) = selection.difficulty {
        backend
          .validate_difficulty(difficulty)
          .with_context(|| format!("Invalid challenge policy in {name}"))?;
      }
    }
    let (initial_weight, debug_header) = match &definition.score {
      Some(score) => (
        score.initial,
        score
          .debug_header
          .enabled
          .then(|| score.debug_header.name.clone()),
      ),
      None => (0, None),
    };
    Ok(CompiledPolicy {
      engine,
      initial_weight,
      debug_header,
    })
  }

  pub fn verifier_diagnostics(&self) -> Option<rilgate_pow::algorithm::VerifierDiagnostics> {
    self
      .algorithms
      .get(PowAlgorithm::RandomX)
      .ok()?
      .verifier_diagnostics()
  }

  pub fn hash_diagnostics(&self) -> Option<rilgate_pow::algorithm::HashDiagnostics> {
    self
      .algorithms
      .get(PowAlgorithm::RandomX)
      .ok()?
      .hash_diagnostics()
  }

  pub async fn remote_ready(&self, timeout: std::time::Duration) -> bool {
    match self.algorithms.get(PowAlgorithm::RandomX) {
      Ok(algorithm) => algorithm.remote_ready(timeout).await.unwrap_or(false),
      Err(_) => false,
    }
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

  pub fn ip_sets(&self) -> rilgate_policy::IpSets {
    self.policy.load().engine.ip_sets().clone()
  }

  pub fn geoip(&self) -> rilgate_policy::GeoDatabases {
    self.policy.load().engine.geoip().clone()
  }

  pub fn lookup_plan(&self) -> rilgate_policy::LookupPlan {
    self.policy.load().engine.lookup_plan().clone()
  }

  pub fn uses_load(&self) -> bool {
    self.policy.load().engine.uses_load()
  }

  pub fn evaluate_request(&self, headers: &HeaderMap, path: &str) -> PolicyDecision {
    let host = headers
      .get(http::header::HOST)
      .and_then(|value| value.to_str().ok())
      .unwrap_or_default()
      .to_ascii_lowercase();
    self.evaluate_context(&rilgate_policy::conditions::Request {
      headers,
      host: &host,
      path,
      method: &http::Method::GET,
      client_ip: None,
      lookups: &rilgate_policy::Lookups::NONE,
    })
  }

  pub fn evaluate_context(
    &self,
    request: &rilgate_policy::conditions::Request<'_>,
  ) -> PolicyDecision {
    let policy = self.policy.load();
    let initial_weight = Self::initial_weight(&policy, request.headers);
    let mut decision = policy.engine.evaluate_context(request, initial_weight);

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

  fn initial_weight(policy: &CompiledPolicy, headers: &HeaderMap) -> i32 {
    if let Some(name) = &policy.debug_header
      && let Some(value) = headers.get(name.as_str())
      && let Ok(value) = value.to_str()
      && let Ok(weight) = value.parse::<i32>()
    {
      return weight;
    }
    policy.initial_weight
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
