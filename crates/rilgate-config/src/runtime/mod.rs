mod open_graph;
pub use open_graph::OpenGraphConfig;
mod server;
use serde::{Deserialize, Serialize};
pub use server::{DecisionLog, ServerConfig};
use std::str::FromStr;
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
  Development,
  #[default]
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

impl std::fmt::Display for Environment {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.write_str(match self {
      Self::Development => "development",
      Self::Production => "production",
    })
  }
}

#[derive(Debug, Clone)]
pub struct UpstreamConfig {
  pub url: String,
  pub preserve_host: bool,
  pub max_body_bytes: usize,
  pub connect_timeout: std::time::Duration,
  pub read_timeout: std::time::Duration,
}

impl UpstreamConfig {
  pub fn validate(&self) -> anyhow::Result<()> {
    use anyhow::ensure;
    let url = url::Url::parse(&self.url)?;
    ensure!(
      matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
      "server.upstream.url must be an HTTP or HTTPS URL with a host"
    );
    ensure!(
      url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none(),
      "server.upstream.url cannot contain credentials, a query, or a fragment"
    );
    ensure!(
      (!self.connect_timeout.is_zero()
        && self.connect_timeout <= std::time::Duration::from_secs(31_536_000))
        && (!self.read_timeout.is_zero()
          && self.read_timeout <= std::time::Duration::from_secs(31_536_000)),
      "server.upstream.timeout: timeouts must be positive and at most one year"
    );
    Ok(())
  }
}
