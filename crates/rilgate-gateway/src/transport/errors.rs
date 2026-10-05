use axum::{
  Json,
  http::StatusCode,
  response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

pub use rilgate_core::error::{CapacityExceeded, PowBackendUnavailable};

#[derive(Error, Debug)]
pub enum ApiError {
  #[error("Invalid request: {0}")]
  InvalidRequest(String),

  #[error("Unauthorized: {0}")]
  Unauthorized(String),

  #[error("Service unavailable: {0}")]
  ServiceUnavailable(String),

  #[error("Too many proof-of-work requests")]
  RateLimited(u64),

  #[error("Request body is too large")]
  PayloadTooLarge,

  #[error("Bad gateway")]
  BadGateway,

  #[error("Upstream timed out")]
  GatewayTimeout,

  #[error("Internal server error")]
  Internal(#[from] anyhow::Error),
}

#[derive(Serialize)]
struct ApiErrorResponse {
  status: u16,
  message: String,
}

impl IntoResponse for ApiError {
  fn into_response(self) -> Response {
    let status = match self {
      Self::InvalidRequest(_) => StatusCode::BAD_REQUEST,
      Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
      Self::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
      Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
      Self::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
      Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
      Self::BadGateway => StatusCode::BAD_GATEWAY,
      Self::GatewayTimeout => StatusCode::GATEWAY_TIMEOUT,
    };

    if let Self::Internal(ref error) = self {
      tracing::error!(?error, "Unhandled API error");
    }

    let retry_after = if let Self::RateLimited(seconds) = &self {
      Some(*seconds)
    } else {
      None
    };
    let mut response = (
      status,
      Json(ApiErrorResponse {
        status: status.as_u16(),
        message: self.to_string(),
      }),
    )
      .into_response();
    if let Some(seconds) = retry_after {
      response.headers_mut().insert(
        axum::http::header::RETRY_AFTER,
        seconds
          .to_string()
          .parse()
          .expect("numeric Retry-After header"),
      );
    }
    response
  }
}

impl ApiError {
  pub fn proof(error: anyhow::Error) -> Self {
    if error.is::<CapacityExceeded>() || error.is::<PowBackendUnavailable>() {
      Self::ServiceUnavailable(error.to_string())
    } else {
      Self::Unauthorized(error.to_string())
    }
  }
}
