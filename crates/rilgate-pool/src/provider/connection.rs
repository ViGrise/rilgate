use crate::{StratumPoolConfig, StratumSession};
use anyhow::{Result, anyhow};
use std::{fmt::Write, sync::Mutex};
use tokio::time::Instant;

#[derive(Default)]
struct PoolState {
  failed_at: Option<Instant>,
  connected: u64,
  failed: u64,
  failover: u64,
}

pub(super) struct Connections {
  config: StratumPoolConfig,
  states: Mutex<Vec<PoolState>>,
}

impl Connections {
  pub fn new(mut config: StratumPoolConfig) -> Self {
    for pool in &mut config.pools {
      pool.agent = config.agent.clone();
      pool.connect_timeout = config.connect_timeout;
      pool.submit_timeout = config.submit_timeout;
    }
    Self {
      states: Mutex::new(
        (0..config.pools.len())
          .map(|_| PoolState::default())
          .collect(),
      ),
      config,
    }
  }

  fn order(&self) -> Vec<usize> {
    let states = self.states.lock().unwrap_or_else(|e| e.into_inner());
    let eligible: Vec<_> = states
      .iter()
      .enumerate()
      .filter(|(_, state)| {
        state
          .failed_at
          .is_none_or(|time| time.elapsed() >= self.config.cooldown)
      })
      .map(|(index, _)| index)
      .collect();
    if eligible.is_empty() {
      (0..states.len()).collect()
    } else {
      eligible
    }
  }

  pub async fn connect(&self, difficulty: u64) -> Result<StratumSession> {
    let order = self.order();
    for (position, &index) in order.iter().enumerate() {
      let pool = &self.config.pools[index];
      let result = tokio::time::timeout(
        pool.connect_timeout,
        StratumSession::connect(pool, difficulty),
      )
      .await;
      let mut states = self.states.lock().unwrap_or_else(|e| e.into_inner());
      let state = &mut states[index];
      match result {
        Ok(Ok(session)) => {
          state.connected += 1;
          state.failed_at = None;
          return Ok(session);
        }
        _ => {
          state.failed += 1;
          state.failed_at = Some(Instant::now());
          if position + 1 < order.len() {
            state.failover += 1;
          }
          tracing::warn!(pool = %pool.name, "Stratum pool connection or login failed");
        }
      }
    }
    Err(anyhow!("All Stratum pools are unavailable"))
  }

  pub async fn readiness(&self) -> Vec<(String, bool)> {
    let mut results: Vec<_> = self
      .config
      .pools
      .iter()
      .map(|pool| (pool.name.clone(), false))
      .collect();
    for index in self.order() {
      let pool = &self.config.pools[index];
      let connected = tokio::time::timeout(
        pool.connect_timeout,
        rilgate_stratum::transport::connect_transport(pool),
      )
      .await
      .is_ok_and(|result| result.is_ok());
      results[index].1 = connected;
      if connected {
        break;
      }
    }
    results
  }

  pub fn metrics(&self) -> String {
    let states = self.states.lock().unwrap_or_else(|e| e.into_inner());
    let mut text = String::from("# TYPE rilgate_stratum_pool_connections_total counter\n");
    for (pool, state) in self.config.pools.iter().zip(states.iter()) {
      for (outcome, value) in [("connected", state.connected), ("failed", state.failed)] {
        let _ = writeln!(
          text,
          "rilgate_stratum_pool_connections_total{{pool=\"{}\",outcome=\"{outcome}\"}} {value}",
          pool.name
        );
      }
    }
    text.push_str("# TYPE rilgate_stratum_pool_failover_total counter\n");
    for (pool, state) in self.config.pools.iter().zip(states.iter()) {
      let _ = writeln!(
        text,
        "rilgate_stratum_pool_failover_total{{from=\"{}\"}} {}",
        pool.name, state.failover
      );
    }
    text
  }
}
