pub mod dto;
pub mod handlers;
pub mod service;

use axum::{Router, routing::get};

use crate::runtime::state::AppState;

pub fn router() -> Router<AppState> {
  Router::new().route("/api/health", get(handlers::index))
}
