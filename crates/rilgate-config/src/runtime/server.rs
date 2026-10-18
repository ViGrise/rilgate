use ipnet::IpNet;

#[derive(Debug, Clone)]
pub struct ServerConfig {
  pub max_connections: usize,
  pub max_requests: usize,
  pub h2_max_streams: u32,
  pub trusted_proxies: Vec<IpNet>,
  pub metrics_enabled: bool,
  pub readiness_timeout_seconds: u64,
}

impl Default for ServerConfig {
  fn default() -> Self {
    Self {
      max_connections: 1024,
      max_requests: 512,
      h2_max_streams: 64,
      trusted_proxies: Vec::new(),
      metrics_enabled: false,
      readiness_timeout_seconds: 2,
    }
  }
}

impl ServerConfig {
  pub fn validate(&self) -> anyhow::Result<()> {
    anyhow::ensure!(
      (1..=1_000_000).contains(&self.max_connections),
      "serve.max_connections must be between 1 and 1000000"
    );
    anyhow::ensure!(
      (1..=1_000_000).contains(&self.max_requests),
      "serve.max_requests must be between 1 and 1000000"
    );
    anyhow::ensure!(
      (1..=1024).contains(&self.h2_max_streams),
      "serve.h2_max_streams must be between 1 and 1024"
    );
    anyhow::ensure!(
      (1..=30).contains(&self.readiness_timeout_seconds),
      "serve.readiness_timeout_seconds must be between 1 and 30"
    );
    anyhow::ensure!(
      self.trusted_proxies.len() <= 64,
      "At most 64 trusted proxy networks are supported"
    );
    Ok(())
  }
}
