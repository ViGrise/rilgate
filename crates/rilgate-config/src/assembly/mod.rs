mod overrides;
pub use overrides::Overrides;
mod defaults;
mod pow;
mod upstream;
mod web;
use crate::{loader::file::FileConfig, resolution::resolve, runtime::*};
use crate::{model::PowConfig, runtime::UpstreamConfig};
pub use rilgate_web::WebConfig;
use std::{
  net::{Ipv6Addr, SocketAddr},
  sync::Arc,
};
use tracing::info;
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
  pub fn validate(&self) -> anyhow::Result<()> {
    use anyhow::{anyhow, ensure};
    if matches!(self.env, Environment::Production) {
      ensure!(
        !self.pow.thresholds.is_empty(),
        "Production requires nonempty explicit pow.thresholds"
      );
    }
    self.server.validate()?;
    self.pow.validate()?;
    self.web.validate()?;
    self.upstream.validate()?;
    for key in [&self.pow.signing_key, &self.pow.auth_signing_key]
      .into_iter()
      .chain(self.pow.signing_keys.previous.values())
      .chain(self.pow.auth_signing_keys.previous.values())
    {
      if matches!(self.env, Environment::Production) {
        ensure!(
          key.expose().len() >= 32
            && !key.expose().starts_with("change-me")
            && !key.expose().starts_with("replace-me"),
          "Production signing keys must contain at least 32 bytes and cannot use placeholder values"
        );
      }
    }
    if self.pow.risk.allow_debug_header && matches!(self.env, Environment::Production) {
      return Err(anyhow!("Debug weight headers are disabled in production"));
    }
    Ok(())
  }

  pub fn load(args: Overrides) -> Config {
    let file = FileConfig::load(args.config.as_deref());

    let env = resolve(
      &file,
      "env",
      &["APP_ENV", "ENV"],
      args.env,
      Environment::Development,
    );

    let app_port = resolve(&file, "serve.port", &["PORT"], args.port, 8080);

    let defaults = ServerConfig::default();
    let networks: Vec<String> = file.section("serve.trusted_proxies").unwrap_or_default();
    let server = ServerConfig {
      max_connections: resolve(
        &file,
        "serve.max_connections",
        &[],
        None,
        defaults.max_connections,
      ),
      max_requests: resolve(
        &file,
        "serve.max_requests",
        &[],
        None,
        defaults.max_requests,
      ),
      h2_max_streams: resolve(
        &file,
        "serve.h2_max_streams",
        &[],
        None,
        defaults.h2_max_streams,
      ),
      metrics_enabled: resolve(&file, "serve.metrics_enabled", &[], None, false),
      readiness_timeout_seconds: resolve(
        &file,
        "serve.readiness_timeout_seconds",
        &[],
        None,
        defaults.readiness_timeout_seconds,
      ),
      trusted_proxies: networks
        .iter()
        .map(|network| network.parse().expect("Invalid trusted proxy CIDR"))
        .collect(),
    };
    let upstream = upstream::load(&file);

    let pow = pow::load(&file, &args);

    let web = web::load(&file);

    let host = resolve(
      &file,
      "serve.host",
      &[],
      None,
      std::net::IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    );

    if matches!(env, Environment::Production) {
      assert!(
        file
          .section::<serde_yaml_ng::Value>("pow.thresholds")
          .is_some(),
        "Production requires explicit pow.thresholds in a configuration file"
      );
      file.reject_unknown_keys();
    } else {
      file.warn_unknown_keys();
    }

    let listen_address = SocketAddr::from((host, app_port));
    let config = Arc::new(Configuration {
      env,
      listen_address,
      app_port,
      server,
      upstream,
      pow,
      web,
    });

    info!(?config, "Application configuration loaded");
    config
  }
}
