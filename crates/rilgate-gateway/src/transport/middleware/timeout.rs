use std::time::Duration;

use hyper::StatusCode;
use tower_http::timeout::TimeoutLayer;

pub fn timeout_layer() -> TimeoutLayer {
  TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, Duration::from_secs(30))
}
