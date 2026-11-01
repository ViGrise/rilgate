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
    let defaults = crate::schema::pow::RateLimit::default();
    Self {
      enabled: defaults.enabled,
      client_burst: defaults.client.burst,
      client_refill_per_second: defaults.client.refill_per_second,
      global_burst: defaults.global.burst,
      global_refill_per_second: defaults.global.refill_per_second,
      max_clients: defaults.max_clients,
    }
  }
}

impl RateLimitConfig {
  pub fn validate(&self) -> anyhow::Result<()> {
    for (path, value) in [
      ("client.burst", self.client_burst),
      ("global.burst", self.global_burst),
    ] {
      anyhow::ensure!(
        (1..=1_000_000).contains(&value),
        "pow.rate_limit.{path}: expected a burst between 1 and 1000000"
      );
    }
    for (path, rate) in [
      ("client.refill_per_second", self.client_refill_per_second),
      ("global.refill_per_second", self.global_refill_per_second),
    ] {
      anyhow::ensure!(
        rate.is_finite() && (0.001..=1_000_000.0).contains(&rate),
        "pow.rate_limit.{path}: expected a finite refill rate between 0.001 and 1000000 requests per second"
      );
    }
    anyhow::ensure!(
      (1..=65_536).contains(&self.max_clients),
      "pow.rate_limit.max_clients must be between 1 and 65536"
    );
    Ok(())
  }
}
