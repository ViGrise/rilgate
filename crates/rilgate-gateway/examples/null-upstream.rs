use axum::{Router, body::Body, http::StatusCode};
use http_body_util::BodyExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let port = std::env::args()
    .nth(1)
    .unwrap_or_else(|| "3300".into())
    .parse::<u16>()?;
  let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
  axum::serve(listener, Router::new().fallback(discard)).await?;
  Ok(())
}

async fn discard(mut body: Body) -> (StatusCode, [(&'static str, &'static str); 1]) {
  while let Some(frame) = body.frame().await {
    if frame.is_err() {
      return (StatusCode::BAD_REQUEST, [("x-perf-upstream", "dev-null")]);
    }
  }
  (StatusCode::OK, [("x-perf-upstream", "dev-null")])
}
