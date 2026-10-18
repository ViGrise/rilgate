use rilgate_test_utils::gateway as app;
mod common;

use axum::{
  body::Body,
  extract::ConnectInfo,
  http::{Request, StatusCode},
};
use std::{net::SocketAddr, sync::Arc};
use tower::ServiceExt;

fn request(path: &str, ip: Option<&str>, forwarded: &str) -> Request<Body> {
  let mut request = Request::builder()
    .uri(path)
    .method("POST")
    .header("content-type", "application/json")
    .header("x-forwarded-for", forwarded)
    .header("forwarded", format!("for={forwarded}"))
    .body(Body::from("{}"))
    .unwrap();
  if let Some(ip) = ip {
    request
      .extensions_mut()
      .insert(ConnectInfo(SocketAddr::new(ip.parse().unwrap(), 12345)));
  }
  request
}

#[tokio::test]
async fn client_limits_use_socket_ips_and_ignore_forwarded_headers() {
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().pow.rate_limit.client_burst = 1;
  let router = app::router(cfg).await.unwrap();
  let path = "/.well-known/test-pow/api/verify";
  let first = router
    .clone()
    .oneshot(request(path, Some("127.0.0.1"), "203.0.113.1"))
    .await
    .unwrap();
  assert_eq!(first.status(), StatusCode::UNPROCESSABLE_ENTITY);
  let blocked = router
    .clone()
    .oneshot(request(path, Some("127.0.0.1"), "203.0.113.2"))
    .await
    .unwrap();
  assert_eq!(blocked.status(), StatusCode::TOO_MANY_REQUESTS);
  assert_eq!(blocked.headers()["cache-control"], "no-store");
  let retry: u64 = blocked.headers()["retry-after"]
    .to_str()
    .unwrap()
    .parse()
    .unwrap();
  assert!((1..=10).contains(&retry));
  let other = router
    .oneshot(request(path, Some("::1"), "203.0.113.1"))
    .await
    .unwrap();
  assert_eq!(other.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn proof_endpoints_share_quota_but_assets_and_health_remain_available() {
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().pow.rate_limit.client_burst = 1;
  let router = app::router(cfg).await.unwrap();
  router
    .clone()
    .oneshot(request(
      "/.well-known/test-pow/api/challenge",
      None,
      "127.0.0.1",
    ))
    .await
    .unwrap();
  for path in ["api/verify", "api/exchange", "api/ws"] {
    let response = router
      .clone()
      .oneshot(request(
        &format!("/.well-known/test-pow/{path}"),
        None,
        "::1",
      ))
      .await
      .unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
  }
  for path in ["api/client-config", "api/health", "static/challenge.js"] {
    let response = router
      .clone()
      .oneshot(
        Request::builder()
          .uri(format!("/.well-known/test-pow/{path}"))
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
  }
}

#[tokio::test]
async fn a_global_limit_cannot_be_bypassed_by_rotating_client_ips() {
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().pow.rate_limit.global_burst = 1;
  let router = app::router(cfg).await.unwrap();
  let path = "/.well-known/test-pow/api/verify";
  router
    .clone()
    .oneshot(request(path, Some("127.0.0.1"), ""))
    .await
    .unwrap();
  assert_eq!(
    router
      .oneshot(request(path, Some("::1"), ""))
      .await
      .unwrap()
      .status(),
    StatusCode::TOO_MANY_REQUESTS
  );
}

#[tokio::test]
async fn rate_limiting_can_be_disabled_explicitly() {
  let mut cfg = common::config();
  let limit = &mut Arc::get_mut(&mut cfg).unwrap().pow.rate_limit;
  limit.enabled = false;
  limit.client_burst = 1;
  limit.global_burst = 1;
  let router = app::router(cfg).await.unwrap();
  for _ in 0..3 {
    assert_eq!(
      router
        .clone()
        .oneshot(request("/.well-known/test-pow/api/verify", None, ""))
        .await
        .unwrap()
        .status(),
      StatusCode::UNPROCESSABLE_ENTITY
    );
  }
}

#[tokio::test]
async fn server_connection_info_populates_the_client_bucket() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().pow.rate_limit.client_burst = 1;
  let gateway = app::build(cfg).await.unwrap();
  let server = tokio::spawn(async move {
    gateway
      .serve(vec![listener], std::future::pending())
      .await
      .unwrap();
  });
  let client = reqwest::Client::new();
  let url = format!("http://{address}/.well-known/test-pow/api/verify");
  for (forwarded, expected) in [
    ("203.0.113.1", StatusCode::UNPROCESSABLE_ENTITY),
    ("203.0.113.2", StatusCode::TOO_MANY_REQUESTS),
  ] {
    let response = client
      .post(&url)
      .header("content-type", "application/json")
      .header("x-forwarded-for", forwarded)
      .body("{}")
      .send()
      .await
      .unwrap();
    assert_eq!(response.status(), expected);
  }
  server.abort();
}

#[tokio::test]
async fn trusted_proxy_clients_have_independent_quotas_and_spoofed_prefixes_do_not_reset_them() {
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.pow.rate_limit.client_burst = 1;
  config.server.trusted_proxies = vec!["::1/128".parse().unwrap()];
  let gateway = app::router(cfg).await.unwrap();
  let path = "/.well-known/test-pow/api/verify";
  for (chain, expected) in [
    ("192.0.2.1", StatusCode::UNPROCESSABLE_ENTITY),
    ("192.0.2.2", StatusCode::UNPROCESSABLE_ENTITY),
    ("203.0.113.9, 192.0.2.1", StatusCode::TOO_MANY_REQUESTS),
    ("invalid", StatusCode::BAD_REQUEST),
  ] {
    assert_eq!(
      gateway
        .clone()
        .oneshot(request(path, None, chain))
        .await
        .unwrap()
        .status(),
      expected
    );
  }
}
