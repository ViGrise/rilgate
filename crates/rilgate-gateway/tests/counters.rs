mod common;
use rilgate_crypto::token::{self, TokenPurpose};
use rilgate_gateway::config::{PowAlgorithm, PowEnforcement};
use rilgate_protocol::claims::ReceiptClaims;
use rilgate_test_utils::{binding::unbound, gateway as app};
use std::{
  sync::Arc,
  time::{Duration, SystemTime, UNIX_EPOCH},
};

#[tokio::test]
async fn cookies_follow_the_required_rate_difficulty_in_both_directions() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.upstream.url = format!("http://{}", listener.local_addr().unwrap());
  config.pow.policy = serde_json::from_value(serde_json::json!({
    "mode": "custom",
    "counters": {"login": {"key": ["client_ip"], "window": "1s", "count_when": {"path": {"prefix": "/login"}}}},
    "rules": [
      {"name": "low", "when": {"rate": {"counter": "login", "gte": 1}}, "action": "score", "score": 10},
      {"name": "high", "when": {"rate": {"counter": "login", "gte": 4}}, "action": "score", "score": 20}
    ],
    "score": {"thresholds": [
      {"min": 30, "action": "challenge", "challenge": {"algorithm": "fast", "difficulty": 6}},
      {"min": 10, "action": "challenge", "challenge": {"algorithm": "fast", "difficulty": 4}}
    ]}
  }))
  .unwrap();
  let service = app::service(&cfg).unwrap();
  let cookie = |difficulty| {
    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_secs() as i64;
    let receipt = token::sign_json(
      &ReceiptClaims {
        host: "example.test".into(),
        v: 1,
        challenge_id: format!("proof-{difficulty}"),
        algorithm: PowAlgorithm::Fast,
        difficulty,
        enforce: PowEnforcement::LocalVerify,
        issued_at: now,
        expires_at: now + 30,
      },
      cfg.pow.signing_key.expose().as_bytes(),
      TokenPurpose::Receipt,
    )
    .unwrap();
    let auth = service
      .exchange_receipt(&receipt, "example.test", &unbound())
      .unwrap();
    format!("{}={auth}", cfg.pow.auth_cookie_name)
  };
  let low = cookie(4);
  let high = cookie(6);
  let upstream = tokio::spawn(async move {
    axum::serve(
      listener,
      axum::Router::new().fallback(|| async { "upstream-ok" }),
    )
    .await
    .unwrap();
  });
  let gateway = app::router(cfg).await.unwrap();
  let client = reqwest::Client::new();
  let get = |path: &str, cookie: String| {
    client
      .get(format!("http://{}{path}", gateway.address()))
      .header("host", "example.test")
      .header("accept", "application/json")
      .header("cookie", cookie)
      .send()
  };
  for _ in 0..8 {
    assert_eq!(get("/page", low.clone()).await.unwrap().status(), 200);
  }
  for _ in 0..3 {
    let response = get("/login", low.clone()).await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "upstream-ok");
  }
  let response = get("/login", low.clone()).await.unwrap();
  assert_eq!(response.status(), 403);
  assert_eq!(response.headers()["rilgate-challenge"], "required");
  assert_eq!(get("/login", high.clone()).await.unwrap().status(), 200);
  tokio::time::sleep(Duration::from_secs(2)).await;
  assert_eq!(get("/login", high).await.unwrap().status(), 200);
  assert_eq!(get("/login", low).await.unwrap().status(), 200);
  upstream.abort();
}
