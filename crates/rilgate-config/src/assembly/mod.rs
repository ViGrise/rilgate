mod overrides;
mod resolve;
use crate::{Environment, PowConfig, ServerConfig, UpstreamConfig, schema::server::Listen};
use anyhow::{Context, Result, ensure};
pub use overrides::{Overrides, Setting, SettingValue};
pub use rilgate_web::WebConfig;
use std::{net::SocketAddr, sync::Arc};
pub type Config = Arc<Configuration>;

#[derive(Debug)]
pub struct Configuration {
  pub env: Environment,
  pub listen_addresses: Vec<SocketAddr>,
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
pub(crate) const MAX_LISTEN_ADDRESSES: usize = 16;

fn listen_path(listen: &Listen, index: usize) -> String {
  if listen.0.len() == 1 {
    "server.listen".into()
  } else {
    format!("server.listen[{index}]")
  }
}
fn listen_address(text: &str, path: &str) -> Result<SocketAddr> {
  let value = if text.starts_with(':') {
    format!("0.0.0.0{text}")
  } else {
    text.to_string()
  };
  value
    .parse()
    .with_context(|| format!("{path}: expected :PORT, IPv4:PORT or [IPv6]:PORT"))
}
/// Parses every entry without applying list-level limits.
pub(crate) fn parse_listen(listen: &Listen) -> Result<Vec<SocketAddr>> {
  listen
    .0
    .iter()
    .enumerate()
    .map(|(index, text)| listen_address(text, &listen_path(listen, index)))
    .collect()
}
/// Parses and validates the listen list: size limit and no duplicate addresses.
pub(crate) fn listen_addresses(listen: &Listen) -> Result<Vec<SocketAddr>> {
  ensure!(
    (1..=MAX_LISTEN_ADDRESSES).contains(&listen.0.len()),
    "server.listen: expected 1 to {MAX_LISTEN_ADDRESSES} addresses, found {}",
    listen.0.len()
  );
  let addresses = parse_listen(listen)?;
  for (index, address) in addresses.iter().enumerate() {
    ensure!(
      !addresses[..index].contains(address),
      "{}: duplicate address {address}",
      listen_path(listen, index)
    );
  }
  Ok(addresses)
}
