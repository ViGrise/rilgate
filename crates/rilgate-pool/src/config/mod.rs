use rilgate_crypto::Secret;
#[derive(Debug, Clone)]
pub struct P2PoolConfig {
  pub host: String,
  pub port: u16,
  pub password: Secret,
  pub agent: String,
  pub connect_timeout_seconds: u64,
  pub submit_timeout_seconds: u64,
  pub max_sessions: usize,
  pub session_ttl_seconds: u64,
  /// Maximum pooled issuance sessions; zero uses one connection per challenge.
  pub max_job_sessions: usize,
  /// How long a pooled session may sit unused before it is closed.
  pub job_session_idle_seconds: u64,
}

impl P2PoolConfig {
  pub fn validate(&self) -> anyhow::Result<()> {
    anyhow::ensure!(
      (1..=4096).contains(&self.max_sessions),
      "pow.p2pool.max_sessions must be between 1 and 4096"
    );
    anyhow::ensure!(
      self.max_job_sessions <= self.max_sessions,
      "pow.p2pool.max_job_sessions must not exceed pow.p2pool.max_sessions, because \
       pooled job sessions hold permits from the same budget as WebSocket sessions"
    );
    anyhow::ensure!(
      (1..=31_536_000).contains(&self.connect_timeout_seconds)
        && (1..=31_536_000).contains(&self.submit_timeout_seconds)
        && (1..=31_536_000).contains(&self.session_ttl_seconds)
        && (1..=31_536_000).contains(&self.job_session_idle_seconds),
      "P2Pool timeouts and session TTL must be between 1 second and 1 year"
    );
    Ok(())
  }
}

impl P2PoolConfig {
  pub fn stratum(&self) -> rilgate_stratum::StratumConfig {
    rilgate_stratum::StratumConfig {
      host: self.host.clone(),
      port: self.port,
      password: self.password.clone(),
      agent: self.agent.clone(),
      connect_timeout_seconds: self.connect_timeout_seconds,
      submit_timeout_seconds: self.submit_timeout_seconds,
    }
  }
}
