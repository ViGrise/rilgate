#[derive(Debug, Clone)]
pub struct RateLimitConfig {
  pub enabled: bool,
  pub client_burst: u32,
  pub client_refill_per_second: f64,
  pub global_burst: u32,
  pub global_refill_per_second: f64,
  pub max_clients: usize,
}

impl Default for RateLimitConfig {
  fn default() -> Self {
    Self {
      enabled: true,
      client_burst: 10,
      client_refill_per_second: 0.1,
      global_burst: 64,
      global_refill_per_second: 16.0,
      max_clients: 4096,
    }
  }
}

impl RateLimitConfig {
  pub fn validate(&self) -> anyhow::Result<()> {
    for value in [self.client_burst, self.global_burst] {
      anyhow::ensure!(
        (1..=1_000_000).contains(&value),
        "Rate-limit bursts must be between 1 and 1000000"
      );
    }
    for rate in [self.client_refill_per_second, self.global_refill_per_second] {
      anyhow::ensure!(
        rate.is_finite() && (0.001..=1_000_000.0).contains(&rate),
        "Rate-limit refill rates must be between 0.001 and 1000000 requests per second"
      );
    }
    anyhow::ensure!(
      (1..=65_536).contains(&self.max_clients),
      "pow.rate_limit.max_clients must be between 1 and 65536"
    );
    Ok(())
  }
}
