use bytes::Bytes;
use http::{StatusCode, header::CONTENT_TYPE};
pub use rilgate_web::assets::Body;
use serde::Serialize;
pub type Response = http::Response<Body>;

pub fn response(status: StatusCode, bytes: impl Into<Bytes>) -> Response {
  http::Response::builder()
    .status(status)
    .body(rilgate_web::assets::body(bytes))
    .expect("valid response")
}
pub fn json(status: StatusCode, value: &impl Serialize) -> Response {
  let mut out = response(
    status,
    serde_json::to_vec(value).expect("serializable API response"),
  );
  out.headers_mut().insert(
    CONTENT_TYPE,
    http::HeaderValue::from_static("application/json"),
  );
  out
}

/// A `303 See Other` to a same-site path that clients must not store.
pub fn see_other(path: &str) -> Response {
  let mut out = response(StatusCode::SEE_OTHER, "");
  out.headers_mut().insert(
    http::header::LOCATION,
    http::HeaderValue::from_str(path).unwrap_or_else(|_| http::HeaderValue::from_static("/")),
  );
  out
    .headers_mut()
    .insert(http::header::CACHE_CONTROL, super::headers::NO_STORE);
  out
}
