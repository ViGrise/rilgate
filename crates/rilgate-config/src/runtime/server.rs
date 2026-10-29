use ipnet::IpNet;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionLog {
  Off,
  Deny,
  #[default]
  Challenge,
  All,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
  pub operations_reload: bool,
  pub watch_config: bool,
  pub max_connections: usize,
  pub max_requests: usize,
  pub h2_max_streams: u32,
  pub trusted_proxies: Vec<IpNet>,
  pub client_ip_header: Option<http::HeaderName>,
  pub decision_log: DecisionLog,
  pub metrics_enabled: bool,
  pub readiness_timeout: std::time::Duration,
}

impl Default for ServerConfig {
  fn default() -> Self {
    let defaults = crate::schema::server::Server::default();
    Self {
      operations_reload: defaults.operations.reload,
      watch_config: defaults.watch_config,
      max_connections: defaults.max_connections,
      max_requests: defaults.max_requests,
      h2_max_streams: defaults.h2_max_streams,
      trusted_proxies: vec![],
      client_ip_header: None,
      decision_log: defaults.decision_log,
      metrics_enabled: defaults.metrics_enabled,
      readiness_timeout: defaults.readiness_timeout.0,
    }
  }
}

impl ServerConfig {
  pub fn validate(&self) -> anyhow::Result<()> {
    anyhow::ensure!(
      (1..=1_000_000).contains(&self.max_connections),
      "server.max_connections must be between 1 and 1000000"
    );
    anyhow::ensure!(
      (1..=1_000_000).contains(&self.max_requests),
      "server.max_requests must be between 1 and 1000000"
    );
    anyhow::ensure!(
      (1..=1024).contains(&self.h2_max_streams),
      "server.h2_max_streams must be between 1 and 1024"
    );
    anyhow::ensure!(
      (!self.readiness_timeout.is_zero()
        && self.readiness_timeout <= std::time::Duration::from_secs(30)),
      "server.readiness_timeout must be positive and at most 30s"
    );
    anyhow::ensure!(
      self.trusted_proxies.len() <= 64,
      "At most 64 trusted proxy networks are supported"
    );
    Ok(())
  }
}
