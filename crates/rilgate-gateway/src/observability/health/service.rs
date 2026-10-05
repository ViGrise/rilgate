use super::dto::{HealthResponse, WebHealth};

pub fn status(web: rilgate_web::runtime::manager::WebDiagnostics) -> HealthResponse {
  HealthResponse {
    status: "ok",
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
