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
