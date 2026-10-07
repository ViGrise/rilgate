use super::dto::{HealthResponse, PoolHealth, WebHealth};

pub fn status(
  web: rilgate_web::runtime::manager::WebDiagnostics,
  pool_failures: (u64, u64),
) -> HealthResponse {
  HealthResponse {
    status: "ok",
    pool: PoolHealth {
      fetch_failures_total: pool_failures.0,
      capacity_failures_total: pool_failures.1,
    },
    web: WebHealth {
      generation: web.generation,
      last_successful_reload: web.last_successful_reload,
      last_reload_failed: web.last_reload_error.is_some(),
      active_locales: web.active_locales,
      template_count: web.template_count,
      watch_enabled: web.watch_enabled,
      reload_total: web.reload_total,
      reload_failures_total: web.reload_failures_total,
    },
  }
}
