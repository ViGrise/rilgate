mod audit;
mod overrides;
mod resolve;
pub mod verifier;
use crate::{Environment, PowConfig, ServerConfig, UpstreamConfig, schema::server::Listen};
use anyhow::{Context, Result, ensure};
pub use audit::{Finding, Severity};
pub use overrides::{Overrides, Setting, SettingValue};
pub use rilgate_web::WebConfig;
use std::{net::SocketAddr, sync::Arc};
pub type Config = Arc<Configuration>;

#[derive(Debug)]
pub struct Configuration {
  pub env: Environment,
  pub listen_addresses: Vec<SocketAddr>,
  pub operations_listen_addresses: Vec<SocketAddr>,
  /// True when no secret or keys were configured and the signing keys derive from a random secret.
  pub secret_generated: bool,
  pub app_port: u16,
  pub server: ServerConfig,
  pub upstream: UpstreamConfig,
  pub pow: PowConfig,
  pub web: WebConfig,
  pub open_graph: crate::OpenGraphConfig,
}
impl Configuration {
  pub fn validate(&self) -> Result<()> {
    self.server.validate().context("server")?;
    self.pow.validate()?;
    self.open_graph.validate()?;
    self.open_graph.compile_condition(
      &rilgate_policy::PolicySets {
        ip: rilgate_policy::IpSets::compile(&self.pow.policy.ip_sets)?,
        asn: rilgate_policy::AsnSets::compile_with(
          "policy.asn_sets",
          &self.pow.policy.asn_sets,
          None,
        )?,
      },
      &rilgate_policy::GeoDatabases::compile(self.pow.policy.geoip.as_ref())?,
    )?;
    self.web.validate().context("site")?;
    self.upstream.validate().context("server.upstream")?;
    for key in [&self.pow.signing_key, &self.pow.auth_signing_key]
      .into_iter()
      .chain(self.pow.signing_keys.previous.values())
      .chain(self.pow.auth_signing_keys.previous.values())
    {
      validate_secret(key, "security.keys")?;
    }
    let errors: Vec<String> = audit::audit(self)
      .into_iter()
      .filter(|finding| finding.severity == Severity::Error)
      .map(|finding| finding.to_string())
      .collect();
    ensure!(errors.is_empty(), "{}", errors.join("\n"));
    Ok(())
  }
  /// Findings that are reported without stopping startup.
  pub fn warnings(&self) -> Vec<Finding> {
    audit::audit(self)
      .into_iter()
      .filter(|finding| finding.severity == Severity::Warning)
      .collect()
  }
  pub fn load(args: Overrides) -> Result<Config> {
    crate::loader::load(&args)?.document.resolve()
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

fn listen_path(listen: &Listen, index: usize, path: &str) -> String {
  if listen.0.len() == 1 {
    path.into()
  } else {
    format!("{path}[{index}]")
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
pub(crate) fn parse_listen(listen: &Listen, path: &str) -> Result<Vec<SocketAddr>> {
  listen
    .0
    .iter()
    .enumerate()
    .map(|(index, text)| listen_address(text, &listen_path(listen, index, path)))
    .collect()
}
/// Parses and validates the listen list: size limit and no duplicate addresses.
pub(crate) fn listen_addresses(
  listen: &Listen,
  path: &str,
  minimum: usize,
) -> Result<Vec<SocketAddr>> {
  ensure!(
    (minimum..=MAX_LISTEN_ADDRESSES).contains(&listen.0.len()),
    "{path}: expected {minimum} to {MAX_LISTEN_ADDRESSES} addresses, found {}",
    listen.0.len()
  );
  let addresses = parse_listen(listen, path)?;
  for (index, address) in addresses.iter().enumerate() {
    ensure!(
      !addresses[..index].contains(address),
      "{}: duplicate address {address}",
      listen_path(listen, index, path)
    );
  }
  Ok(addresses)
}
