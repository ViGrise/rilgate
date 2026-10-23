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
    let backend = self.algorithms.get(presentation.algorithm)?;
    backend.validate_enforcement(presentation.enforce)?;
    let prepared = backend
      .issue(WorkRequest {
        difficulty: presentation.difficulty,
        enforce: presentation.enforce,
      })
      .await?;
    ensure!(
      prepared.algorithm() == presentation.algorithm,
      "Work algorithm does not match presentation"
    );
    let claims = work::claims(&prepared, presentation.host, presentation.enforce);
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
      metadata.issued_at,
      metadata.expires_at,
      backend.challenge_ttl(),
    )?;
    ensure!(
      metadata.enforce == self.cfg.enforce,
      "Challenge uses a different enforcement mode"
    );
    ensure!(
      metadata.host == request_host,
      "Challenge host does not match request host"
    );
    backend.validate_enforcement(metadata.enforce)?;
    backend.validate_difficulty(metadata.difficulty)?;
    backend
      .verify(&work::prepared(&claims), Proof { nonce })
      .await?;
    validate_lifetime(
      metadata.version,
      metadata.issued_at,
      metadata.expires_at,
      backend.challenge_ttl(),
    )?;
    self.issue_receipt(
      metadata.host.to_owned(),
      metadata.challenge_id.to_owned(),
      metadata.algorithm,
      metadata.difficulty,
      metadata.enforce,
    )
  }
}
