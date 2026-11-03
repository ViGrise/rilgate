use super::work;
use crate::{ChallengeService, tokens::validation::validate_lifetime};
use anyhow::{Context, Result, ensure};
use rilgate_pow::{Proof, WorkRequest};
use rilgate_protocol::{ChallengeResponse, claims::ChallengeClaims};
impl ChallengeService {
  pub async fn issue_challenge(
    &self,
    presentation_token: &str,
    request_host: &str,
  ) -> Result<ChallengeResponse> {
    let presentation = self.verify_presentation(presentation_token, request_host)?;
    let backend = self
      .algorithms
      .get(presentation.presented_pow()?.algorithm)?;
    backend.validate_verification(presentation.presented_pow()?.verification)?;
    let prepared = backend
      .issue(WorkRequest {
        difficulty: presentation.presented_pow()?.difficulty,
        verification: presentation.presented_pow()?.verification,
      })
      .await?;
    ensure!(
      prepared.algorithm() == presentation.presented_pow()?.algorithm,
      "Work algorithm does not match presentation"
    );
    let claims = work::claims(
      &prepared,
      presentation.host.clone(),
      presentation.presented_pow()?.verification,
    );
    let token = self.tokens.challenge.sign_json(&claims)?;
    work::response(prepared, token)
  }
  pub async fn verify_challenge(
    &self,
    challenge_token: &str,
    nonce: &str,
    request_host: &str,
  ) -> Result<String> {
    let claims: ChallengeClaims = self
      .tokens
      .challenge
      .verify_json(challenge_token)
      .context("Challenge signature validation failed")?;
    let metadata = claims.metadata();
    let backend = self.algorithms.get(metadata.algorithm)?;
    validate_lifetime(
      metadata.version,
      &[1],
      metadata.issued_at,
      metadata.expires_at,
      backend.challenge_ttl(),
    )?;
    ensure!(
      metadata.verification == self.cfg.verification.of(metadata.algorithm),
      "Challenge uses a different verification mode"
    );
    ensure!(
      metadata.host == request_host,
      "Challenge host does not match request host"
    );
    backend.validate_verification(metadata.verification)?;
    backend.validate_difficulty(metadata.difficulty)?;
    backend
      .verify(&work::prepared(&claims), Proof { nonce })
      .await?;
    validate_lifetime(
      metadata.version,
      &[1],
      metadata.issued_at,
      metadata.expires_at,
      backend.challenge_ttl(),
    )?;
    self.issue_receipt(
      metadata.host.to_owned(),
      metadata.challenge_id.to_owned(),
      metadata.algorithm,
      metadata.difficulty,
      metadata.verification,
    )
  }
}
