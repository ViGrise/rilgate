use crate::{
  runtime::state::AppState,
  transport::{
    errors::ApiError,
    response::{Response, response},
  },
};
use http::{HeaderMap, Method, StatusCode};
pub fn static_asset(
  state: &AppState,
  path: &str,
  headers: &HeaderMap,
  method: &Method,
) -> Result<Response, ApiError> {
  if rilgate_web::resources::loader::logical_path(path).is_err() {
    return Ok(response(StatusCode::NOT_FOUND, ""));
  }
  let snapshot = state.web.current();
  let Some(asset) = snapshot
    .assets
    .get(rilgate_web::assets::canonical_asset(path))
  else {
    return Ok(response(StatusCode::NOT_FOUND, ""));
  };
  asset
    .response(headers, method == Method::HEAD)
    .map_err(ApiError::Internal)
}
