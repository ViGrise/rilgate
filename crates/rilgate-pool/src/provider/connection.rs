use crate::{StratumPoolConfig, StratumSession};
use anyhow::{Result, anyhow};
use std::{
  collections::{BTreeMap, HashMap},
  fmt::Write,
  sync::Mutex,
};
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
  proxies: Mutex<HashMap<(usize, usize), PoolState>>,
}

impl Connections {
  pub fn new(mut config: StratumPoolConfig) -> Self {
    for pool in &mut config.pools {
      pool.difficulty_mismatches = Default::default();
      pool.agent = config.agent.clone();
      pool.connect_timeout = config.connect_timeout;
      pool.submit_timeout = config.submit_timeout;
    }
    Self {
      proxies: Mutex::new(HashMap::new()),
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

  fn proxy_list(&self, index: usize) -> (usize, &[rilgate_stratum::config::Proxy]) {
    match &self.config.pools[index].proxies {
      Some(proxies) => (index + 1, proxies),
      None => (0, &self.config.proxies),
    }
  }

  async fn attempt(&self, index: usize, difficulty: Option<u64>) -> Result<Option<StratumSession>> {
    let pool = &self.config.pools[index];
    let (scope, proxies) = self.proxy_list(index);
    let mut order: Vec<Option<usize>> = {
      let states = self.proxies.lock().unwrap_or_else(|e| e.into_inner());
      (0..proxies.len())
        .filter(|i| {
          states.get(&(scope, *i)).is_none_or(|state| {
            state
              .failed_at
              .is_none_or(|time| time.elapsed() >= self.config.cooldown)
          })
        })
        .map(Some)
        .collect()
    };
    if order.is_empty() {
      order = if proxies.is_empty() {
        vec![None]
      } else {
        (0..proxies.len()).map(Some).collect()
      };
    }
    for proxy_index in order {
      let proxy = proxy_index.map(|i| &proxies[i]);
      let result = tokio::time::timeout(pool.connect_timeout, async {
        if let Some(difficulty) = difficulty {
          StratumSession::connect_via(pool, proxy, difficulty)
            .await
            .map(Some)
        } else {
          rilgate_stratum::transport::connect_via(pool, proxy)
            .await
            .map(|_| None)
        }
      })
      .await;
      let connected = matches!(&result, Ok(Ok(_)));
      if let Some(i) = proxy_index {
        let mut states = self.proxies.lock().unwrap_or_else(|e| e.into_inner());
        let state = states.entry((scope, i)).or_default();
        if connected {
          state.connected += 1;
          state.failed_at = None;
        } else {
          state.failed += 1;
          state.failed_at = Some(Instant::now());
        }
      }
      match result {
        Ok(Ok(session)) => return Ok(session),
        Ok(Err(error)) => {
          tracing::warn!(pool = %pool.name, proxy = proxy.map(|proxy| proxy.name.as_str()), error = %error, "Stratum connection attempt failed")
        }
        Err(_) => {
          tracing::warn!(pool = %pool.name, proxy = proxy.map(|proxy| proxy.name.as_str()), "Stratum connection attempt timed out")
        }
      }
    }
    Err(anyhow!("Pool connection attempts failed"))
  }

  pub async fn connect(&self, difficulty: u64) -> Result<StratumSession> {
    let order = self.order();
    for (position, &index) in order.iter().enumerate() {
      let pool = &self.config.pools[index];
      let result = self.attempt(index, Some(difficulty)).await;
      let mut states = self.states.lock().unwrap_or_else(|e| e.into_inner());
      let state = &mut states[index];
      match result {
        Ok(Some(session)) => {
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
    let order = self.order();
    for (position, &index) in order.iter().enumerate() {
      let connected = self.attempt(index, None).await.is_ok();
      {
        let mut states = self.states.lock().unwrap_or_else(|e| e.into_inner());
        let state = &mut states[index];
        if connected {
          state.connected += 1;
          state.failed_at = None;
        } else {
          state.failed += 1;
          state.failed_at = Some(Instant::now());
          if position + 1 < order.len() {
            state.failover += 1;
          }
        }
      }
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
    text.push_str("# TYPE rilgate_stratum_pool_difficulty_mismatch_total counter\n");
    for pool in &self.config.pools {
      let _ = writeln!(
        text,
        "rilgate_stratum_pool_difficulty_mismatch_total{{pool=\"{}\"}} {}",
        pool.name,
        pool
          .difficulty_mismatches
          .load(std::sync::atomic::Ordering::Relaxed)
      );
    }
    text.push_str("# TYPE rilgate_stratum_proxy_connections_total counter\n");
    let states = self.proxies.lock().unwrap_or_else(|e| e.into_inner());
    let mut counters: BTreeMap<&str, (u64, u64)> = BTreeMap::new();
    let lists = std::iter::once((0, self.config.proxies.as_slice())).chain(
      self
        .config
        .pools
        .iter()
        .enumerate()
        .filter_map(|(i, pool)| pool.proxies.as_deref().map(|proxies| (i + 1, proxies))),
    );
    for (scope, proxies) in lists {
      for (index, proxy) in proxies.iter().enumerate() {
        let counter = counters.entry(&proxy.name).or_default();
        if let Some(state) = states.get(&(scope, index)) {
          counter.0 += state.connected;
          counter.1 += state.failed;
        }
      }
    }
    for (proxy, (connected, failed)) in counters {
      for (outcome, value) in [("connected", connected), ("failed", failed)] {
        let _ = writeln!(
          text,
          "rilgate_stratum_proxy_connections_total{{proxy=\"{proxy}\",outcome=\"{outcome}\"}} {value}"
        );
      }
    }
    text
  }
}
