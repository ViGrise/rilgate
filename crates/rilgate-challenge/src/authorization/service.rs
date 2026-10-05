use crate::{
  ChallengeService,
  tokens::validation::{unix_now, validate_lifetime},
};
use anyhow::{Context, Result, anyhow};
use rilgate_crypto::token::{self as crypto, TokenPurpose};
use rilgate_protocol::{PowAlgorithm, PowEnforcement, claims::*};
use uuid::Uuid;
impl ChallengeService {
  pub fn issue_p2pool_receipt(&self, difficulty: u64, host: String) -> Result<String> {
    self.issue_receipt(
      host,
      Uuid::now_v7().to_string(),
      PowAlgorithm::RandomX,
      difficulty,
      PowEnforcement::P2poolAccepted,
    )
  }

  pub(crate) fn issue_receipt(
    &self,
    host: String,
    challenge_id: String,
    algorithm: PowAlgorithm,
    difficulty: u64,
    enforce: PowEnforcement,
  ) -> Result<String> {
    let now = unix_now()?;
    let claims = ReceiptClaims {
      host,
      v: 1,
      challenge_id,
      algorithm,
      difficulty,
      enforce,
      issued_at: now,
      expires_at: now + self.cfg.receipt_ttl_seconds as i64,
    };

    crypto::sign_json(
      &claims,
      self.cfg.signing_key.expose().as_bytes(),
      TokenPurpose::Receipt,
    )
  }

  pub fn exchange_receipt(&self, receipt: &str, request_host: &str) -> Result<String> {
    let receipt: ReceiptClaims = crypto::verify_json(
      receipt,
      self.cfg.signing_key.expose().as_bytes(),
      TokenPurpose::Receipt,
    )
    .context("Receipt signature validation failed")?;

    validate_lifetime(
      receipt.v,
      receipt.issued_at,
      receipt.expires_at,
      self.cfg.receipt_ttl_seconds,
    )?;
    if receipt.host != request_host {
      return Err(anyhow!("Proof receipt host does not match request host"));
    }

    if receipt.enforce != self.cfg.enforce {
      return Err(anyhow!("Proof receipt uses a different enforcement mode"));
    }

    self
      .algorithms
      .get(receipt.algorithm)?
      .validate_difficulty(receipt.difficulty)?;

    let claims = AuthClaims {
      host: receipt.host,
      v: 1,
      algorithm: receipt.algorithm,
      difficulty: receipt.difficulty,
      enforce: receipt.enforce,
      issued_at: receipt.issued_at,
      expires_at: receipt.issued_at + self.cfg.auth_ttl_seconds as i64,
    };

    validate_lifetime(
      claims.v,
      claims.issued_at,
      claims.expires_at,
      self.cfg.auth_ttl_seconds,
    )?;
    crypto::sign_json(
      &claims,
      self.cfg.auth_signing_key.expose().as_bytes(),
      TokenPurpose::Auth,
    )
  }

  pub fn verify_auth_token(&self, token: &str, request_host: &str) -> Result<AuthClaims> {
    let claims: AuthClaims = crypto::verify_json(
      token,
      self.cfg.auth_signing_key.expose().as_bytes(),
      TokenPurpose::Auth,
    )?;

    validate_lifetime(
      claims.v,
      claims.issued_at,
      claims.expires_at,
      self.cfg.auth_ttl_seconds,
    )?;
    if claims.host != request_host || claims.enforce != self.cfg.enforce {
      return Err(anyhow!("Access token scope does not match request"));
    }
    self
      .algorithms
      .get(claims.algorithm)?
      .validate_difficulty(claims.difficulty)?;

    Ok(claims)
  }
}
