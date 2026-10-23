#[derive(Debug, Clone, Copy, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobSessionMode {
  Pooled,
  PerChallenge,
}

#[derive(Debug, Clone)]
pub struct StratumPoolConfig {
  pub pools: Vec<rilgate_stratum::StratumConfig>,
  pub cooldown: std::time::Duration,
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

impl StratumPoolConfig {
  pub fn validate(&self) -> anyhow::Result<()> {
    anyhow::ensure!(
      (1..=4096).contains(&self.max_sessions),
      "pow.stratum_pool.sessions.max must be between 1 and 4096"
    );
    anyhow::ensure!(
      (1..=4096).contains(&self.max_job_sessions)
        && (self.job_session_mode == JobSessionMode::PerChallenge
          || self.max_job_sessions <= self.max_sessions),
      "pow.stratum_pool.job_sessions.max must not exceed pow.stratum_pool.sessions.max, because \
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
        "pow.stratum_pool.{path}: expected a positive duration of at most one year"
      );
    }
    anyhow::ensure!(
      (1..=8).contains(&self.pools.len()),
      "pow.stratum_pool.pools: expected 1 to 8 pools"
    );
    anyhow::ensure!(
      (std::time::Duration::from_secs(1)..=std::time::Duration::from_secs(600))
        .contains(&self.cooldown),
      "pow.stratum_pool.failover.cooldown: expected 1s to 10m"
    );
    Ok(())
  }
}
