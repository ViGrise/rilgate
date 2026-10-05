use std::sync::Arc;

use anyhow::Result;
use axum::Router;

use super::state::AppState;
use crate::{
  observability::health, proof, proxy::client::ProxyService, transport::middleware as middlewares,
};
use rilgate_config::Config;

pub async fn router(
  cfg: Config,
  pow: Arc<rilgate_challenge::ChallengeService>,
  pool: Arc<rilgate_pool::P2PoolProvider>,
) -> Result<Router> {
  cfg.validate()?;
  let proxy = Arc::new(ProxyService::new(cfg.upstream.clone())?);

  let web_config = cfg.web.clone();
  let web = tokio::task::spawn_blocking(move || rilgate_web::WebManager::new(web_config)).await??;
  let web_runtime = Arc::new(rilgate_web::runtime::watcher::runtime(&web));
  let app_state = AppState {
    cfg: cfg.clone(),
    pow,
    pool,
    proxy,
    web,
    web_runtime,
  };

  let internal_prefix = cfg.web.base_path.clone();
  let limiter = Arc::new(crate::transport::rate_limit::RateLimiter::new(
    cfg.pow.rate_limit.clone(),
  )?);
  let internal = proof::routes::router(limiter).merge(health::router());

  let app = Router::new()
    .nest(&internal_prefix, internal)
    .fallback(crate::proxy::gate::gate);

  Ok(
    app
      .layer(middlewares::timeout_layer())
      .layer(middlewares::propagate_request_id_layer())
      .layer(rilgate_telemetry::trace_layer())
      .layer(middlewares::request_id_layer())
      .with_state(app_state),
  )
}
