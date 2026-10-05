use std::{
  collections::HashMap,
  net::IpAddr,
  sync::Mutex,
  time::{Duration, Instant},
};

use rilgate_config::RateLimitConfig;

struct Bucket {
  tokens: f64,
  updated: Instant,
}

impl Bucket {
  fn new(burst: u32, now: Instant) -> Self {
    Self {
      tokens: f64::from(burst),
      updated: now,
    }
  }

  fn refill(&mut self, burst: u32, rate: f64, now: Instant) {
    self.tokens = (self.tokens + now.saturating_duration_since(self.updated).as_secs_f64() * rate)
      .min(f64::from(burst));
    self.updated = self.updated.max(now);
  }

  fn wait_for(&self, tokens: f64, rate: f64) -> u64 {
    ((tokens - self.tokens).max(0.0) / rate).ceil().max(1.0) as u64
  }
}

struct LimiterState {
  global: Bucket,
  clients: HashMap<Option<IpAddr>, Bucket>,
  next_cleanup: Option<Instant>,
}

pub struct RateLimiter {
  config: RateLimitConfig,
  state: Mutex<LimiterState>,
}

impl RateLimiter {
  pub fn new(config: RateLimitConfig) -> anyhow::Result<Self> {
    config.validate()?;
    Ok(Self {
      state: Mutex::new(LimiterState {
        global: Bucket::new(config.global_burst, Instant::now()),
        clients: HashMap::new(),
        next_cleanup: None,
      }),
      config,
    })
  }

  pub fn check(&self, client: Option<IpAddr>) -> Result<(), u64> {
    self.check_at(client, Instant::now())
  }

  fn check_at(&self, client: Option<IpAddr>, now: Instant) -> Result<(), u64> {
    if !self.config.enabled {
      return Ok(());
    }
    let cfg = &self.config;
    let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
    // Concurrent callers can acquire the lock in a different order from their timestamps.
    let now = now.max(state.global.updated);
    state
      .global
      .refill(cfg.global_burst, cfg.global_refill_per_second, now);
    if state.global.tokens < 1.0 {
      return Err(state.global.wait_for(1.0, cfg.global_refill_per_second));
    }
    let client = client.map(normalize_ip);
    if !state.clients.contains_key(&client) {
      if state.clients.len() >= cfg.max_clients {
        if let Some(deadline) = state.next_cleanup
          && now < deadline
        {
          return Err(deadline.duration_since(now).as_secs_f64().ceil().max(1.0) as u64);
        }
        // Fully refilled buckets can be removed without granting additional quota.
        state.clients.retain(|_, bucket| {
          bucket.refill(cfg.client_burst, cfg.client_refill_per_second, now);
          bucket.tokens < f64::from(cfg.client_burst)
        });
      }
      if state.clients.len() >= cfg.max_clients {
        let wait = state
          .clients
          .values()
          .map(|bucket| bucket.wait_for(f64::from(cfg.client_burst), cfg.client_refill_per_second))
          .min()
          .unwrap_or(1);
        // The earliest refill bounds when another full-table scan can free a slot.
        state.next_cleanup = Some(now + Duration::from_secs(wait));
        return Err(wait);
      }
      state.next_cleanup = None;
      state
        .clients
        .insert(client, Bucket::new(cfg.client_burst, now));
    }
    let bucket = state
      .clients
      .get_mut(&client)
      .expect("client bucket initialized");
    bucket.refill(cfg.client_burst, cfg.client_refill_per_second, now);
    if bucket.tokens < 1.0 {
      return Err(bucket.wait_for(1.0, cfg.client_refill_per_second));
    }
    bucket.tokens -= 1.0;
    state.global.tokens -= 1.0;
    Ok(())
  }
}

fn normalize_ip(ip: IpAddr) -> IpAddr {
  match ip {
    IpAddr::V6(ip) => ip.to_ipv4_mapped().map_or(IpAddr::V6(ip), IpAddr::V4),
    other => other,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::Arc;

  fn config() -> RateLimitConfig {
    RateLimitConfig {
      enabled: true,
      client_burst: 2,
      client_refill_per_second: 1.0,
      global_burst: 4,
      global_refill_per_second: 2.0,
      max_clients: 2,
    }
  }

  #[test]
  fn refill_rates_and_tracking_capacity_are_validated() {
    for rate in [0.0, -1.0, f64::NAN, f64::INFINITY] {
      let mut cfg = config();
      cfg.client_refill_per_second = rate;
      assert!(RateLimiter::new(cfg).is_err());
    }
    let mut cfg = config();
    cfg.max_clients = 0;
    assert!(RateLimiter::new(cfg).is_err());
  }

  #[test]
  fn out_of_order_timestamps_cannot_refill_the_same_time_twice() {
    let now = Instant::now();
    let mut bucket = Bucket::new(2, now);
    bucket.tokens = 0.0;
    bucket.refill(2, 1.0, now + Duration::from_millis(500));
    bucket.refill(2, 1.0, now);
    bucket.refill(2, 1.0, now + Duration::from_secs(1));
    assert_eq!(bucket.tokens, 1.0);
  }

  #[test]
  fn concurrent_clients_cannot_exceed_the_global_burst() {
    let mut cfg = config();
    cfg.global_burst = 32;
    cfg.client_burst = 1;
    cfg.max_clients = 64;
    let limiter = Arc::new(RateLimiter::new(cfg).unwrap());
    let now = Instant::now();
    let threads: Vec<_> = (0..4)
      .map(|group| {
        let limiter = limiter.clone();
        std::thread::spawn(move || {
          (1..=16)
            .filter(|index| {
              let ip = IpAddr::V4(std::net::Ipv4Addr::new(192, 0, group, *index));
              limiter.check_at(Some(ip), now).is_ok()
            })
            .count()
        })
      })
      .collect();
    let accepted: usize = threads
      .into_iter()
      .map(|thread| thread.join().unwrap())
      .sum();
    assert_eq!(accepted, 32);
  }

  #[test]
  fn fractional_refill_rates_report_the_required_wait() {
    let mut cfg = config();
    cfg.client_burst = 1;
    cfg.client_refill_per_second = 0.1;
    let limiter = RateLimiter::new(cfg).unwrap();
    let now = Instant::now();
    assert!(limiter.check_at(None, now).is_ok());
    assert_eq!(limiter.check_at(None, now), Err(10));
    assert_eq!(limiter.check_at(None, now + Duration::from_secs(5)), Err(5));
    assert!(
      limiter
        .check_at(None, now + Duration::from_secs(10))
        .is_ok()
    );
  }

  #[test]
  fn client_and_global_quotas_refill_without_charging_rejected_requests() {
    let limiter = RateLimiter::new(config()).unwrap();
    let now = Instant::now();
    let first = Some("192.0.2.1".parse().unwrap());
    let second = Some("192.0.2.2".parse().unwrap());
    assert!(limiter.check_at(first, now).is_ok());
    assert!(limiter.check_at(first, now).is_ok());
    assert_eq!(limiter.check_at(first, now), Err(1));
    assert!(limiter.check_at(second, now).is_ok());
    assert!(limiter.check_at(second, now).is_ok());
    assert_eq!(limiter.check_at(None, now), Err(1));
    assert!(
      limiter
        .check_at(first, now + Duration::from_secs(1))
        .is_ok()
    );
  }

  #[test]
  fn client_table_is_bounded_and_only_refilled_entries_are_evicted() {
    let mut cfg = config();
    cfg.global_burst = 100;
    let limiter = RateLimiter::new(cfg).unwrap();
    let now = Instant::now();
    for ip in ["192.0.2.1", "192.0.2.2"] {
      assert!(limiter.check_at(Some(ip.parse().unwrap()), now).is_ok());
    }
    let third = Some("192.0.2.3".parse().unwrap());
    assert_eq!(limiter.check_at(third, now), Err(1));
    assert_eq!(limiter.state.lock().unwrap().clients.len(), 2);
    assert!(
      limiter
        .check_at(third, now + Duration::from_secs(1))
        .is_ok()
    );
    assert_eq!(limiter.state.lock().unwrap().clients.len(), 1);
  }

  #[test]
  fn ipv4_mapped_ipv6_uses_the_same_client_quota() {
    let limiter = RateLimiter::new(config()).unwrap();
    let now = Instant::now();
    assert!(
      limiter
        .check_at(Some("192.0.2.1".parse().unwrap()), now)
        .is_ok()
    );
    assert!(
      limiter
        .check_at(Some("::ffff:192.0.2.1".parse().unwrap()), now)
        .is_ok()
    );
    assert_eq!(
      limiter.check_at(Some("192.0.2.1".parse().unwrap()), now),
      Err(1)
    );
  }
}
