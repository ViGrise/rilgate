use crate::model::gateway::RateLimitConfig;
use rilgate_crypto::Secret;
use rilgate_policy::{BotRuleConfig, RiskConfig, ThresholdConfig};
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
  pub default_algorithm: PowAlgorithm,
  pub signing_key: Secret,
  pub auth_signing_key: Secret,
  pub signing_keys: rilgate_crypto::token::KeyRotation,
  pub auth_signing_keys: rilgate_crypto::token::KeyRotation,
  pub challenge_ttl_seconds: u64,
  pub randomx_challenge_ttl_seconds: u64,
  pub receipt_ttl_seconds: u64,
  pub auth_ttl_seconds: u64,
  pub auth_cookie_name: String,
  pub nonce_offset: usize,
  pub verifier_queue_capacity: usize,
  pub rate_limit: RateLimitConfig,
  pub p2pool: P2PoolConfig,
  pub difficulty: DifficultyConfig,
  pub risk: RiskConfig,
  pub bots: Vec<BotRuleConfig>,
  pub thresholds: Vec<ThresholdConfig>,
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
      risk: self.risk.clone(),
      bots: self.bots.clone(),
      thresholds: self.thresholds.clone(),
    }
  }
}
