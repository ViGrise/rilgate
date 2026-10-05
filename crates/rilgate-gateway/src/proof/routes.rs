use super::handlers;
use crate::transport::rate_limit::RateLimiter;
use crate::{runtime::state::AppState, transport::middleware::rate_limit, web::assets};
use axum::{
  Router,
  extract::DefaultBodyLimit,
  middleware,
  routing::{get, post},
};
use std::sync::Arc;
pub fn router(limiter: Arc<RateLimiter>) -> Router<AppState> {
  Router::new()
    .route("/api/challenge", post(handlers::challenge))
    .route("/api/verify", post(handlers::verify))
    .route("/api/exchange", post(handlers::exchange))
    .route("/api/ws", get(handlers::websocket))
    .route_layer(middleware::from_fn_with_state(limiter, rate_limit::enforce))
    .route_layer(DefaultBodyLimit::max(16 * 1024))
    .route("/api/client-config", get(handlers::client_config))
    .route_layer(middleware::from_fn(handlers::no_store))
    .route("/static/{*path}", get(assets::static_asset))
}
