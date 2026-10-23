use crate::model::gateway::RateLimitConfig;
use rilgate_challenge::authorization::binding::AuthBinding;
use rilgate_crypto::Secret;
use rilgate_policy::policy::schema::Definition;
use rilgate_pool::P2PoolConfig;
use rilgate_pow::difficulty::DifficultyConfig;
use rilgate_protocol::{PowAlgorithm, PowEnforcement};
#[derive(Debug, Clone)]
pub struct BrowserConfig {
  // Relative paths are resolved below the configured web namespace.
  pub solver_module_path: String,
}

#[derive(Debug, Clone)]
pub struct PowConfig {
  pub enforce: PowEnforcement,
  pub randomx_enabled: bool,
  pub default_algorithm: PowAlgorithm,
  pub signing_key: Secret,
  pub auth_signing_key: Secret,
  pub signing_keys: rilgate_crypto::token::KeyRotation,
  pub auth_signing_keys: rilgate_crypto::token::KeyRotation,
  pub challenge_ttl_seconds: u64,
  pub randomx_challenge_ttl_seconds: u64,
  pub receipt_ttl_seconds: u64,
  pub auth_ttl_seconds: u64,
  pub auth_binding: AuthBinding,
  pub auth_cookie_name: String,
  pub nonce_offset: usize,
  pub verifier_queue_capacity: usize,
  pub randomx_verify: crate::schema::pow::Verify,
  pub randomx_cache_idle_timeout: std::time::Duration,
  pub rate_limit: RateLimitConfig,
  pub p2pool: P2PoolConfig,
  pub difficulty: DifficultyConfig,
  pub policy: Definition,
  pub browser: BrowserConfig,
}

impl PowConfig {
  pub fn challenge(&self) -> rilgate_challenge::ChallengeConfig {
    rilgate_challenge::ChallengeConfig {
      enforce: self.enforce,
      default_algorithm: self.default_algorithm,
      signing_key: self.signing_key.clone(),
      auth_signing_key: self.auth_signing_key.clone(),
      signing_keys: self.signing_keys.clone(),
      auth_signing_keys: self.auth_signing_keys.clone(),
      challenge_ttl_seconds: self.challenge_ttl_seconds,
      receipt_ttl_seconds: self.receipt_ttl_seconds,
      auth_ttl_seconds: self.auth_ttl_seconds,
      auth_binding: self.auth_binding.clone(),
      policy: self.policy.clone(),
    }
  }
}

impl PowConfig {
  pub fn selects_randomx(&self) -> bool {
    self
      .policy
      .challenges()
      .expect("validated policy")
      .iter()
      .any(|(_, selection)| {
        selection.algorithm.unwrap_or(self.default_algorithm) == PowAlgorithm::RandomX
      })
  }

  pub fn needs_remote_verifier(&self) -> bool {
    self.enforce == PowEnforcement::LocalVerify
      && self.selects_randomx()
      && !self
        .randomx_verify
        .order
        .contains(&crate::schema::pow::Hasher::Local)
  }
}

impl crate::schema::pow::Verify {
  /// The hasher order for the RandomX backend.
  pub fn hashers(&self) -> rilgate_pow_randomx::HasherOrder {
    let remote = &self.remote;
    rilgate_pow_randomx::HasherOrder {
      order: self.order.clone(),
      remote: remote
        .endpoint
        .clone()
        .map(|endpoint| rilgate_pow_randomx::RemoteHasherConfig {
          endpoint,
          server_name: remote.effective_server_name().unwrap_or_default(),
          timeout: remote.timeout.0,
          ca: remote.tls.ca.clone(),
          client_cert: remote.tls.client_cert.clone(),
          client_key: remote.tls.client_key.clone(),
        }),
      deadline_policy: remote.on_deadline_rejected,
    }
  }
}
