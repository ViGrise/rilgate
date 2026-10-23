use rilgate_crypto::Secret;
#[derive(Debug, Clone)]
pub struct StratumConfig {
  pub name: String,
  pub user: Secret,
  pub host: String,
  pub port: u16,
  pub password: Secret,
  pub agent: String,
  pub connect_timeout: std::time::Duration,
  pub submit_timeout: std::time::Duration,
}
