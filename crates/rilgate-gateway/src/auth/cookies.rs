use anyhow::{Context, Result};
use http::{HeaderMap, HeaderValue};
use rilgate_challenge::{ChallengeService, authorization::binding::BindingInput};
use rilgate_config::{Config, Environment};
pub fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
  headers
    .get_all(http::header::COOKIE)
    .iter()
    .filter_map(|header| header.to_str().ok())
    .flat_map(|raw| raw.split(';'))
    .find_map(|pair| {
      let (key, value) = pair.trim().split_once('=')?;
      (key == name).then_some(value)
    })
}

pub fn auth_cookie_header(
  service: &ChallengeService,
  cfg: &Config,
  token: &str,
  host: &str,
  binding: &BindingInput<'_>,
) -> Result<HeaderValue> {
  let claims = service.verify_auth_token(token, host, binding)?;
  issued_cookie_header(cfg, token, claims.expires_at)
}

pub(crate) fn issued_cookie_header(
  cfg: &Config,
  token: &str,
  expires_at: i64,
) -> Result<HeaderValue> {
  let now = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)?
    .as_secs() as i64;
  let secure = if matches!(cfg.env, Environment::Production) {
    "; Secure"
  } else {
    ""
  };
  HeaderValue::from_str(&format!(
    "{}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
    cfg.pow.auth_cookie_name,
    (expires_at - now).max(0),
    secure
  ))
  .context("Unable to build authentication cookie")
}
