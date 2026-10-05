use anyhow::{Context, Result};
use axum::http::{HeaderMap, HeaderValue};
use rilgate_challenge::ChallengeService;
use rilgate_config::{Config, Environment};
pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
  headers
    .get_all(http::header::COOKIE)
    .iter()
    .filter_map(|header| header.to_str().ok())
    .flat_map(|raw| raw.split(';'))
    .find_map(|pair| {
      let (key, value) = pair.trim().split_once('=')?;
      (key == name).then(|| value.to_string())
    })
}

pub fn auth_cookie_header(
  service: &ChallengeService,
  cfg: &Config,
  token: &str,
  host: &str,
) -> Result<HeaderValue> {
  let claims = service.verify_auth_token(token, host)?;
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
    (claims.expires_at - now).max(0),
    secure
  ))
  .context("Unable to build authentication cookie")
}
