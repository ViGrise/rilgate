use crate::{
  ChallengeService,
  authorization::{AuthGrant, binding::BindingInput},
};
use anyhow::{Result, ensure};
use rilgate_protocol::PowAlgorithm;

/// The outcome of following a refresh URL.
pub enum RefreshRedemption {
  /// The delay has passed. `path` is the original request path, safe to use as a same-site
  /// redirect target.
  Granted {
    auth_token: String,
    path: String,
    network_merged: bool,
  },
  /// The delay has not passed yet.
  Wait { seconds: u64 },
}

impl ChallengeService {
  /// Redeems a presentation token for a refresh challenge once `difficulty` seconds have passed
  /// since it was issued.
  pub fn redeem_refresh(
    &self,
    token: &str,
    host: &str,
    binding: &BindingInput<'_>,
    now: i64,
    previous: Option<&str>,
  ) -> Result<RefreshRedemption> {
    let claims = self.verify_presentation(token, host)?;
    ensure!(
      claims.algorithm == PowAlgorithm::Refresh,
      "Presentation token is not a refresh challenge"
    );
    let ready_at = claims.issued_at.saturating_add(claims.difficulty as i64);
    if now < ready_at {
      return Ok(RefreshRedemption::Wait {
        seconds: (ready_at - now) as u64,
      });
    }
    let issued = self.issue_auth_token(
      AuthGrant {
        host: claims.host,
        algorithm: claims.algorithm,
        difficulty: claims.difficulty,
        verification: claims.verification,
        issued_at: now,
      },
      binding,
      previous,
    )?;
    Ok(RefreshRedemption::Granted {
      auth_token: issued.token,
      network_merged: issued.network_merged,
      path: same_site_path(claims.path),
    })
  }
}

impl ChallengeService {
  /// The same-site path a refresh token was issued for, when the token is a genuine refresh
  /// token for `host`. The token lifetime does not matter, so an expired or redeemed URL can
  /// send the browser back to the protected page for a new challenge.
  pub fn refresh_return_path(&self, token: &str, host: &str) -> Option<String> {
    let claims: rilgate_protocol::claims::PresentationClaims =
      self.tokens.presentation.verify_json(token).ok()?;
    (claims.host == host && claims.algorithm == PowAlgorithm::Refresh)
      .then(|| same_site_path(claims.path))
  }
}

/// Keeps a redirect target on the current host: it must start with exactly one `/`.
fn same_site_path(path: String) -> String {
  if path.starts_with('/') && !path.starts_with("//") && !path.contains(['\\', '\r', '\n']) {
    path
  } else {
    "/".into()
  }
}
