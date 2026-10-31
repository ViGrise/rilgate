use crate::{
  runtime::state::AppState,
  transport::{
    errors::ApiError,
    response::{Response, response},
  },
};
use http::{HeaderMap, Method, StatusCode};
use rilgate_web::assets::AssetCache;
pub fn static_asset(
  state: &AppState,
  path: &str,
  headers: &HeaderMap,
  method: &Method,
) -> Result<Response, ApiError> {
  let (revision, path) = split_revision(path);
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
  let cache = if revision == Some(snapshot.asset_revision.as_str()) {
    AssetCache::Immutable
  } else {
    AssetCache::Revalidate
  };
  asset
    .response(headers, method == Method::HEAD, cache)
    .map_err(ApiError::Internal)
}

/// Splits a leading `v-<12 lowercase hex>` revision segment from an asset path.
fn split_revision(path: &str) -> (Option<&str>, &str) {
  let Some((segment, rest)) = path.split_once('/') else {
    return (None, path);
  };
  match segment.strip_prefix("v-") {
    Some(revision)
      if revision.len() == 12
        && revision
          .bytes()
          .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) =>
    {
      (Some(revision), rest)
    }
    _ => (None, path),
  }
}
