mod overrides;
mod resolve;
use crate::{Environment, PowConfig, ServerConfig, UpstreamConfig};
use anyhow::{Context, Result, ensure};
pub use overrides::{Overrides, Setting, SettingValue};
pub use rilgate_web::WebConfig;
use std::{net::SocketAddr, sync::Arc};
pub type Config = Arc<Configuration>;

#[derive(Debug)]
pub struct Configuration {
  pub env: Environment,
  pub listen_address: SocketAddr,
  pub app_port: u16,
  pub server: ServerConfig,
  pub upstream: UpstreamConfig,
  pub pow: PowConfig,
  pub web: WebConfig,
}
impl Configuration {
  pub fn validate(&self) -> Result<()> {
    self.server.validate().context("server")?;
    self.pow.validate()?;
    self.web.validate().context("site")?;
    self.upstream.validate().context("server.upstream")?;
    let production = matches!(self.env, Environment::Production);
    for key in [&self.pow.signing_key, &self.pow.auth_signing_key]
      .into_iter()
      .chain(self.pow.signing_keys.previous.values())
      .chain(self.pow.auth_signing_keys.previous.values())
    {
      ensure!(
        !key.expose().is_empty(),
        "security.keys: signing keys must not be empty"
      );
      if production {
        validate_secret(key, "security.keys")?;
      }
    }
    if production
      && self
        .pow
        .policy
        .score
        .as_ref()
        .is_some_and(|s| s.debug_header.enabled)
    {
      anyhow::bail!(
        "policy.score.debug_header.enabled: debug score headers are disabled in production"
      );
    }
    if production {
      for (name, set) in &self.pow.policy.ip_sets {
        if let Some(url) = &set.url {
          ensure!(
            url
              .get(..8)
              .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://")),
            "policy.ip_sets.{name}.url: production requires https"
          );
        }
      }
    }
    Ok(())
  }
  pub fn load(args: Overrides) -> Result<Config> {
    crate::loader::load(&args)?.resolve()
  }
}
pub(crate) fn validate_secret(secret: &rilgate_crypto::Secret, path: &str) -> Result<()> {
  ensure!(
    secret.expose().len() >= 32
      && !secret.expose().starts_with("change-me")
      && !secret.expose().starts_with("replace-me"),
    "{path}: expected at least 32 bytes of high-entropy secret material; placeholders are unsupported"
  );
  Ok(())
}
pub(crate) fn listen_address(text: &str) -> Result<SocketAddr> {
  let value = if text.starts_with(':') {
    format!("0.0.0.0{text}")
  } else {
    text.to_string()
  };
  value
    .parse()
    .context("server.listen: expected :PORT, IPv4:PORT or [IPv6]:PORT")
}
