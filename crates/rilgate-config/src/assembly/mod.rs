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
  pub upstream: UpstreamConfig,
  pub pow: PowConfig,
  pub web: WebConfig,
}

impl Configuration {
  pub fn validate(&self) -> anyhow::Result<()> {
    use anyhow::{anyhow, ensure};
    self.pow.validate()?;
    self.web.validate()?;
    self.upstream.validate()?;
    for key in [&self.pow.signing_key, &self.pow.auth_signing_key] {
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

    let upstream = upstream::load(&file);

    let pow = pow::load(&file, &args);

    let web = web::load(&file);

    file.warn_unknown_keys();

    let listen_address = SocketAddr::from((Ipv6Addr::UNSPECIFIED, app_port));
    let config = Arc::new(Configuration {
      env,
      listen_address,
      app_port,
      upstream,
      pow,
      web,
    });

    info!(?config, "Application configuration loaded");
    config
  }
}
