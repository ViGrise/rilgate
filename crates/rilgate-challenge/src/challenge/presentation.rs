use crate::{
  ChallengeService,
  tokens::validation::{unix_now, validate_lifetime},
};
use anyhow::{Context, Result, anyhow};
use rilgate_policy::{PolicyAction, PolicyDecision};
use rilgate_protocol::claims::*;
impl ChallengeService {
  pub fn issue_presentation(
    &self,
    host: String,
    path: String,
    decision: &PolicyDecision<'_>,
  ) -> Result<String> {
    Ok(
      self
        .issue_presentation_with_claims(host, path, decision)?
        .token,
    )
  }

  pub fn issue_presentation_with_claims(
    &self,
    host: String,
    path: String,
    decision: &PolicyDecision<'_>,
  ) -> Result<crate::IssuedToken<PresentationClaims>> {
    if decision.action != PolicyAction::Challenge {
      return Err(anyhow!(
        "Presentation tokens are only valid for CHALLENGE decisions"
      ));
    }

    let algorithm = decision
      .algorithm
      .ok_or_else(|| anyhow!("Challenge decision has no algorithm"))?;

    let difficulty = decision
      .difficulty
      .ok_or_else(|| anyhow!("Challenge decision has no difficulty"))?;

    self
      .algorithms
      .get(algorithm)?
      .validate_difficulty(difficulty)?;

    let now = unix_now()?;
    let claims = PresentationClaims {
      v: 1,
      host,
      path,
      rule: decision.rule.to_owned(),
      weight: decision.weight,
      algorithm,
      difficulty,
      enforce: self.cfg.enforce,
      issued_at: now,
      expires_at: now + self.cfg.challenge_ttl_seconds as i64,
    };

    let token = self.tokens.presentation.sign_json(&claims)?;
    Ok(crate::IssuedToken { token, claims })
  }

  pub fn verify_presentation(&self, token: &str, request_host: &str) -> Result<PresentationClaims> {
    let claims: PresentationClaims = self
      .tokens
      .presentation
      .verify_json(token)
      .context("Presentation signature validation failed")?;

    validate_lifetime(
      claims.v,
      claims.issued_at,
      claims.expires_at,
      self.cfg.challenge_ttl_seconds,
    )?;

    if claims.enforce != self.cfg.enforce {
      return Err(anyhow!(
        "Presentation token uses a different enforcement mode"
      ));
    }

    if claims.host != request_host {
      return Err(anyhow!("Presentation host does not match request host"));
    }

    self
      .algorithms
      .get(claims.algorithm)?
      .validate_difficulty(claims.difficulty)?;

    Ok(claims)
  }
}
