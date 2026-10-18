#[derive(Debug, Clone, Copy, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobSessionMode {
  Pooled,
  PerChallenge,
}

use rilgate_crypto::Secret;
#[derive(Debug, Clone)]
pub struct P2PoolConfig {
  pub host: String,
  pub port: u16,
  pub password: Secret,
  pub agent: String,
  pub connect_timeout: std::time::Duration,
  pub submit_timeout: std::time::Duration,
  pub max_sessions: usize,
  pub session_ttl: std::time::Duration,
  pub job_session_mode: JobSessionMode,
  pub max_job_sessions: usize,
  /// How long a pooled session may sit unused before it is closed.
  pub job_session_idle: std::time::Duration,
}

impl P2PoolConfig {
  pub fn validate(&self) -> anyhow::Result<()> {
    anyhow::ensure!(
      (1..=4096).contains(&self.max_sessions),
      "pow.p2pool.sessions.max must be between 1 and 4096"
    );
    anyhow::ensure!(
      (1..=4096).contains(&self.max_job_sessions)
        && (self.job_session_mode == JobSessionMode::PerChallenge
          || self.max_job_sessions <= self.max_sessions),
      "pow.p2pool.job_sessions.max must not exceed pow.p2pool.sessions.max, because \
       pooled job sessions hold permits from the same budget as WebSocket sessions"
    );
    for (path, duration) in [
      ("timeout.connect", self.connect_timeout),
      ("timeout.submit", self.submit_timeout),
      ("sessions.ttl", self.session_ttl),
      ("job_sessions.idle", self.job_session_idle),
    ] {
      anyhow::ensure!(
        !duration.is_zero() && duration <= std::time::Duration::from_secs(31_536_000),
        "pow.p2pool.{path}: expected a positive duration of at most one year"
      );
    }
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
      connect_timeout: self.connect_timeout,
      submit_timeout: self.submit_timeout,
    }
  }
}
