use rilgate_crypto::Secret;
#[derive(Debug, Clone)]
pub struct StratumConfig {
  pub host: String,
  pub port: u16,
  pub password: Secret,
  pub agent: String,
  pub connect_timeout_seconds: u64,
  pub submit_timeout_seconds: u64,
}
