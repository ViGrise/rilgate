use crate::transport::errors::ApiError;
use crate::transport::rate_limit::RateLimiter;
use axum::{
  extract::{ConnectInfo, Request, State},
  middleware::Next,
  response::{IntoResponse, Response},
};
use std::{net::SocketAddr, sync::Arc};
pub async fn enforce(
  State(limiter): State<Arc<RateLimiter>>,
  request: Request,
  next: Next,
) -> Response {
  let peer = request
    .extensions()
    .get::<ConnectInfo<SocketAddr>>()
    .map(|peer| peer.0.ip());
  match limiter.check(peer) {
    Ok(()) => next.run(request).await,
    Err(seconds) => ApiError::RateLimited(seconds).into_response(),
  }
}
