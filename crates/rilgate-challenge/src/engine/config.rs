use crate::authorization::binding::AuthBinding;
use rilgate_crypto::Secret;
use rilgate_policy::policy::schema::Definition;
use rilgate_protocol::{PowAlgorithm, PowEnforcement};
#[derive(Debug, Clone)]
pub struct ChallengeConfig {
  pub enforce: PowEnforcement,
  pub default_algorithm: PowAlgorithm,
  pub signing_key: Secret,
  pub auth_signing_key: Secret,
  pub signing_keys: rilgate_crypto::token::KeyRotation,
  pub auth_signing_keys: rilgate_crypto::token::KeyRotation,
  pub challenge_ttl_seconds: u64,
  pub receipt_ttl_seconds: u64,
  pub auth_ttl_seconds: u64,
  pub auth_binding: AuthBinding,
  pub policy: Definition,
}
impl ChallengeConfig {
  pub fn validate(&self) -> anyhow::Result<()> {
    self.signing_keys.validate()?;
    self.auth_signing_keys.validate()?;
    for ttl in [
      self.challenge_ttl_seconds,
      self.receipt_ttl_seconds,
      self.auth_ttl_seconds,
    ] {
      anyhow::ensure!(
        (1..=31_536_000).contains(&ttl),
        "Token TTLs must be between 1 second and 1 year"
      );
    }
    for key in [&self.signing_key, &self.auth_signing_key] {
      anyhow::ensure!(!key.expose().is_empty(), "PoW signing keys cannot be empty");
    }
    self.auth_binding.validate()?;
    rilgate_policy::PolicyEngine::from_definition(&self.policy)?;
    Ok(())
  }
}
