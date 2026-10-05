use crate::{runtime::state::AppState, transport::errors::ApiError};
use axum::{
  extract::State,
  http::{HeaderMap, StatusCode},
  response::{IntoResponse, Response},
};
pub async fn static_asset(
  State(state): State<AppState>,
  axum::extract::Path(path): axum::extract::Path<String>,
  headers: HeaderMap,
  method: axum::http::Method,
) -> Result<Response, ApiError> {
  if rilgate_web::resources::loader::logical_path(&path).is_err() {
    return Ok(StatusCode::NOT_FOUND.into_response());
  }
  let snapshot = state.web.current();
  let Some(asset) = snapshot
    .assets
    .get(rilgate_web::assets::canonical_asset(&path))
  else {
    return Ok(StatusCode::NOT_FOUND.into_response());
  };
  asset
    .response(&headers, method == axum::http::Method::HEAD)
    .map_err(ApiError::Internal)
}
