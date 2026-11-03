use crate::{
  ChallengeService, IssuedToken,
  tokens::validation::{unix_now, validate_lifetime},
};
use anyhow::{Context, Result, ensure};
use rilgate_protocol::{ChallengeStep, PowStep, claims::*};

#[derive(Debug, Clone)]
pub struct Presentation {
  pub version: u8,
  pub host: String,
  pub path: String,
  pub rule: String,
  pub weight: i32,
  pub steps: Vec<ChallengeStep>,
  pub step: usize,
  pub issued_at: i64,
  pub expires_at: i64,
}
impl Presentation {
  pub fn presented(&self) -> &PowStep {
    let ChallengeStep::Pow(step) = &self.steps[self.step];
    step
  }
}
impl ChallengeService {
  #[allow(clippy::too_many_arguments)]
  pub fn issue_presentation(
    &self,
    host: String,
    path: String,
    rule: String,
    weight: i32,
    steps: Vec<ChallengeStep>,
    step: usize,
  ) -> Result<IssuedToken<Presentation>> {
    ensure!(
      (1..=4).contains(&steps.len()) && step < steps.len(),
      "Presentation steps are invalid"
    );
    for ChallengeStep::Pow(step) in &steps {
      self
        .algorithms
        .get(step.algorithm)?
        .validate_difficulty(step.difficulty)?;
    }
    let now = unix_now()?;
    let claims = Presentation {
      version: if steps.len() == 1 { 1 } else { 2 },
      host,
      path,
      rule,
      weight,
      steps,
      step,
      issued_at: now,
      expires_at: now + self.cfg.challenge_ttl_seconds as i64,
    };
    let token = if claims.version == 1 {
      let step = claims.presented();
      self.tokens.presentation.sign_json(&PresentationClaims {
        v: 1,
        host: claims.host.clone(),
        path: claims.path.clone(),
        rule: claims.rule.clone(),
        weight,
        algorithm: step.algorithm,
        difficulty: step.difficulty,
        verification: step.verification,
        issued_at: now,
        expires_at: claims.expires_at,
      })?
    } else {
      self.tokens.presentation.sign_json(&PresentationClaimsV2 {
        v: 2,
        host: claims.host.clone(),
        path: claims.path.clone(),
        rule: claims.rule.clone(),
        weight,
        steps: claims.steps.clone(),
        step: step as u8,
        issued_at: now,
        expires_at: claims.expires_at,
      })?
    };
    Ok(IssuedToken { token, claims })
  }
  pub fn verify_presentation(&self, token: &str, request_host: &str) -> Result<Presentation> {
    let claims: AnyPresentationClaims = self
      .tokens
      .presentation
      .verify_json(token)
      .context("Presentation signature validation failed")?;
    let claims = match claims {
      AnyPresentationClaims::V1(c) => {
        ensure!(c.v == 1, "Token version or lifetime is invalid or expired");
        Presentation {
          version: c.v,
          host: c.host,
          path: c.path,
          rule: c.rule,
          weight: c.weight,
          steps: vec![ChallengeStep::Pow(PowStep {
            algorithm: c.algorithm,
            difficulty: c.difficulty,
            verification: c.verification,
          })],
          step: 0,
          issued_at: c.issued_at,
          expires_at: c.expires_at,
        }
      }
      AnyPresentationClaims::V2(c) => {
        ensure!(c.v == 2, "Token version or lifetime is invalid or expired");
        Presentation {
          version: c.v,
          host: c.host,
          path: c.path,
          rule: c.rule,
          weight: c.weight,
          steps: c.steps,
          step: c.step as usize,
          issued_at: c.issued_at,
          expires_at: c.expires_at,
        }
      }
    };
    validate_lifetime(
      claims.version,
      &[1, 2],
      claims.issued_at,
      claims.expires_at,
      self.cfg.challenge_ttl_seconds,
    )?;
    ensure!(
      claims.host == request_host,
      "Presentation host does not match request host"
    );
    ensure!(
      (1..=4).contains(&claims.steps.len()) && claims.step < claims.steps.len(),
      "Presentation steps are invalid"
    );
    for ChallengeStep::Pow(step) in &claims.steps {
      ensure!(
        step.verification == self.cfg.verification.of(step.algorithm),
        "Presentation token uses a different verification mode"
      );
      self
        .algorithms
        .get(step.algorithm)?
        .validate_difficulty(step.difficulty)?;
    }
    Ok(claims)
  }
}
