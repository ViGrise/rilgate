mod server;
use serde::{Deserialize, Serialize};
pub use server::ServerConfig;
use std::str::FromStr;
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Environment {
  Development,
  Production,
}

impl FromStr for Environment {
  type Err = String;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    match value {
      "development" => Ok(Self::Development),
      "production" => Ok(Self::Production),
      other => Err(format!(
        "Invalid environment {other:?}; expected development or production"
      )),
    }
  }
}

#[derive(Debug, Clone)]
pub struct UpstreamConfig {
  pub url: String,
  pub preserve_host: bool,
  pub max_body_bytes: usize,
  pub connect_timeout_seconds: u64,
  pub read_timeout_seconds: u64,
}

impl UpstreamConfig {
  pub fn validate(&self) -> anyhow::Result<()> {
    use anyhow::ensure;
    let url = url::Url::parse(&self.url)?;
    ensure!(
      matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
      "upstream.url must be an HTTP or HTTPS URL with a host"
    );
    ensure!(
      url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none(),
      "upstream.url cannot contain credentials, a query, or a fragment"
    );
    ensure!(
      (1..=31_536_000).contains(&self.connect_timeout_seconds)
        && (1..=31_536_000).contains(&self.read_timeout_seconds),
      "upstream timeouts must be between 1 second and 1 year"
    );
    Ok(())
  }
}
