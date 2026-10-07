use serde::Serialize;

#[derive(Serialize)]
pub struct HealthResponse {
  pub status: &'static str,
  pub web: WebHealth,
  pub pool: PoolHealth,
}
#[derive(Serialize)]
pub struct WebHealth {
  pub generation: u64,
  pub last_successful_reload: Option<u64>,
  pub last_reload_failed: bool,
  pub active_locales: Vec<String>,
  pub template_count: usize,
  pub watch_enabled: bool,
  pub reload_total: u64,
  pub reload_failures_total: u64,
}

#[derive(Serialize)]
pub struct PoolHealth {
  pub fetch_failures_total: u64,
  pub capacity_failures_total: u64,
}
