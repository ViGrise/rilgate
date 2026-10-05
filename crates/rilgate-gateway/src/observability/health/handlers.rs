use super::{dto::HealthResponse, service};
use crate::runtime::state::AppState;
use axum::{Json, extract::State};

pub async fn index(State(state): State<AppState>) -> Json<HealthResponse> {
  Json(service::status(state.web.diagnostics()))
}
