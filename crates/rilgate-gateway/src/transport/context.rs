use axum::http::HeaderMap;
pub fn request_host(headers: &HeaderMap, uri: &http::Uri) -> String {
  headers
    .get(http::header::HOST)
    .and_then(|value| value.to_str().ok())
    .or_else(|| uri.authority().map(|authority| authority.as_str()))
    .unwrap_or_default()
    .to_ascii_lowercase()
}
