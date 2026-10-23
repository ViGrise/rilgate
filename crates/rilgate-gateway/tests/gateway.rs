use rilgate_test_utils::binding::unbound;
use rilgate_test_utils::gateway as app;
use rilgate_test_utils::gateway::service as make_service;
mod common;

use axum::{
  body::{Body, to_bytes},
  http::{HeaderMap, Request, StatusCode},
};
use rilgate_crypto::token::{self as crypto, TokenPurpose};
use rilgate_gateway::{
  config::{PowAlgorithm, PowEnforcement},
  proxy::client::ProxyService,
};
use rilgate_pow_hash::sha256::verifier::verify_fast;
use rilgate_protocol::{
  ChallengeResponse,
  claims::{ChallengeClaims, ReceiptClaims},
};
use std::{
  sync::Arc,
  time::{SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

#[tokio::test]
async fn policy_and_proxy_reject_paths_that_require_url_normalization() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
  let upstream_calls = calls.clone();
  let upstream = tokio::spawn(async move {
    let router = axum::Router::new().fallback(move |uri: axum::http::Uri| {
      upstream_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
      async move { uri.to_string() }
    });
    axum::serve(listener, router).await.unwrap();
  });
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().upstream.url = format!("http://{address}/base/");
  let proxy = ProxyService::new(cfg.upstream.clone()).unwrap();
  let router = app::router(cfg).await.unwrap();
  for path in [
    "/public/../protected",
    "/./protected",
    "/public/%2e%2e/protected",
    "/public/.%2E/protected",
    "/public/%2E./protected",
    "/%2e/protected",
    "/public\\..\\protected",
  ] {
    let request = || Request::builder().uri(path).body(Body::empty()).unwrap();
    let response = router.clone().oneshot(request()).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
    assert!(proxy.request_url(&path.parse().unwrap()).is_err());
  }
  let response = router
    .clone()
    .oneshot(
      Request::builder()
        .uri("/protected")
        .header("host", "example.test")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::OK);
  assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);

  let path = "/public/a%2Eb//file?next=/../protected&encoded=%2F";
  let response = router
    .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::OK);
  assert_eq!(
    to_bytes(response.into_body(), 1024).await.unwrap(),
    format!("/base{path}")
  );
  assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
  upstream.abort();
}

#[tokio::test]
async fn fast_proof_is_portable_between_gateway_instances() {
  let cfg = common::config();
  let issuer = make_service(&cfg).unwrap();
  let verifier = make_service(&cfg).unwrap();
  let decision = issuer.evaluate_request(&HeaderMap::new(), "/protected");
  let presentation = issuer
    .issue_presentation("example.test".into(), "/protected".into(), &decision)
    .unwrap();
  let ChallengeResponse::Fast {
    challenge_token,
    challenge,
    difficulty,
  } = verifier
    .issue_challenge(&presentation, "example.test")
    .await
    .unwrap()
  else {
    panic!("expected fast challenge")
  };
  let nonce = (0..100_000)
    .map(|n| n.to_string())
    .find(|n| verify_fast(&challenge, n, difficulty).unwrap())
    .unwrap();
  assert!(
    issuer
      .verify_challenge(&challenge_token, &nonce, "other.test")
      .await
      .is_err()
  );
  let receipt = issuer
    .verify_challenge(&challenge_token, &nonce, "example.test")
    .await
    .unwrap();
  let auth = verifier
    .exchange_receipt(&receipt, "example.test", &unbound())
    .unwrap();
  let claims = issuer
    .verify_auth_token(&auth, "example.test", &unbound())
    .unwrap();
  assert_eq!(claims.algorithm, PowAlgorithm::Fast);
  assert_eq!(claims.difficulty, 1);
  let cookie = rilgate_gateway::auth::cookies::auth_cookie_header(
    &verifier,
    &common::config(),
    &auth,
    "example.test",
    &unbound(),
  )
  .unwrap();
  assert!(cookie.to_str().unwrap().contains("HttpOnly; SameSite=Lax"));
}

#[tokio::test]
async fn unsolved_randomx_challenge_cannot_be_exchanged_or_used_as_auth() {
  let mut cfg = common::config();
  // Separate token purposes remain necessary when deployments share a key.
  let pow = &mut Arc::get_mut(&mut cfg).unwrap().pow;
  pow.auth_signing_key = pow.signing_key.clone();
  let now = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap()
    .as_secs() as i64;
  let claims = ChallengeClaims::Randomx {
    v: 1,
    challenge_id: "test".into(),
    enforce: PowEnforcement::LocalVerify,
    host: "example.test".into(),
    blob: "00".repeat(64),
    seed_hash: "00".repeat(32),
    target: "ffffffff".into(),
    difficulty: 1000,
    nonce_offset: 39,
    issued_at: now,
    expires_at: now + 60,
  };
  let token = crypto::sign_json(
    &claims,
    cfg.pow.signing_key.expose().as_bytes(),
    TokenPurpose::Challenge,
  )
  .unwrap();
  // Claim shapes overlap, so deserialization alone does not establish purpose.
  let receipt_shape: ReceiptClaims =
    serde_json::from_value(serde_json::to_value(&claims).unwrap()).unwrap();
  assert_eq!(receipt_shape.difficulty, 1000);
  let service = make_service(&cfg).unwrap();
  assert!(
    service
      .exchange_receipt(&token, "example.test", &unbound())
      .is_err()
  );
  assert!(
    service
      .verify_auth_token(&token, "example.test", &unbound())
      .is_err()
  );
  let receipt = service
    .issue_stratum_pool_receipt(1000, "example.test".into())
    .unwrap();
  assert!(
    service
      .exchange_receipt(&receipt, "example.test", &unbound())
      .is_err()
  );
}

#[tokio::test]
async fn expired_receipt_is_rejected_at_its_expiry_second() {
  let cfg = common::config();
  let now = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap()
    .as_secs() as i64;
  let claims = ReceiptClaims {
    host: "example.test".into(),
    v: 1,
    challenge_id: "expired".into(),
    algorithm: PowAlgorithm::Fast,
    difficulty: 1,
    enforce: PowEnforcement::LocalVerify,
    issued_at: now - 30,
    expires_at: now,
  };
  let token = crypto::sign_json(
    &claims,
    cfg.pow.signing_key.expose().as_bytes(),
    TokenPurpose::Receipt,
  )
  .unwrap();
  assert!(
    make_service(&cfg)
      .unwrap()
      .exchange_receipt(&token, "example.test", &unbound())
      .is_err()
  );
}

#[tokio::test]
async fn challenge_stays_at_original_url_and_reserves_only_configured_namespace() {
  let router = app::router(common::config()).await.unwrap();
  let response = router
    .clone()
    .oneshot(
      Request::builder()
        .uri("/protected/article?q=1")
        .header("host", "example.test")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::OK);
  assert!(!response.headers().contains_key("location"));
  assert!(
    response.headers()["cache-control"]
      .to_str()
      .unwrap()
      .contains("no-store")
  );
  let html = String::from_utf8(
    to_bytes(response.into_body(), 100_000)
      .await
      .unwrap()
      .to_vec(),
  )
  .unwrap();
  assert!(
    html
      .replace("&#x2f;", "/")
      .contains("/.well-known/test-pow/static/js/challenge.bundle.js")
  );
  assert!(!html.contains("__PRESENTATION_TOKEN__"));
  let response = router
    .clone()
    .oneshot(
      Request::builder()
        .method("POST")
        .uri("/protected")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
  let response = router
    .oneshot(
      Request::builder()
        .uri("/.well-known/test-pow/missing")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn proxy_streams_response_and_removes_connection_scoped_headers() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let (release, wait) = tokio::sync::oneshot::channel::<()>();
  let upstream = tokio::spawn(async move {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    loop {
      let mut byte = [0];
      socket.read_exact(&mut byte).await.unwrap();
      request.push(byte[0]);
      if request.ends_with(b"\r\n\r\n") {
        break;
      }
    }
    let request = String::from_utf8(request).unwrap().to_lowercase();
    assert!(request.starts_with("get /file?q=1 http/1.1"));
    assert!(request.contains("x-end-to-end: keep"));
    assert!(!request.contains("x-private:"));
    assert!(!request.contains("keep-alive:"));
    assert!(!request.contains("proxy-authorization:"));
    socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: X-Private, close\r\nX-Private: secret\r\nKeep-Alive: timeout=5\r\nX-End-To-End: keep\r\n\r\n").await.unwrap();
    wait.await.unwrap();
    socket.write_all(b"hello").await.unwrap();
  });
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().upstream.url = format!("http://{address}");
  let proxy = app::router(cfg).await.unwrap();
  let response = tokio::time::timeout(
    std::time::Duration::from_secs(2),
    proxy.oneshot(
      Request::builder()
        .uri("/file?q=1")
        .header("connection", "X-Private")
        .header("connection", "Keep-Alive")
        .header("x-private", "secret")
        .header("keep-alive", "timeout=5")
        .header("proxy-authorization", "secret")
        .header("x-end-to-end", "keep")
        .body(Body::empty())
        .unwrap(),
    ),
  )
  .await
  .expect("headers should arrive before the upstream body")
  .unwrap();
  assert!(!response.headers().contains_key("x-private"));
  if let Some(connection) = response.headers().get("connection") {
    assert!(matches!(
      connection.to_str().unwrap(),
      "keep-alive" | "close"
    ));
  }
  assert!(!response.headers().contains_key("keep-alive"));
  assert_eq!(response.headers()["x-end-to-end"], "keep");
  release.send(()).unwrap();
  assert_eq!(to_bytes(response.into_body(), 100).await.unwrap(), "hello");
  upstream.await.unwrap();
}

#[tokio::test]
async fn proxy_rejects_large_request_bodies_without_contacting_upstream() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().upstream.url =
    format!("http://{}", listener.local_addr().unwrap());
  let gateway = app::router(cfg).await.unwrap();
  let response = gateway
    .oneshot(
      Request::builder()
        .uri("/")
        .body(Body::from(vec![0; 1025]))
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
  assert!(
    tokio::time::timeout(std::time::Duration::from_millis(20), listener.accept())
      .await
      .is_err()
  );
}

#[tokio::test]
async fn unavailable_upstream_returns_bad_gateway() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  drop(listener);
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().upstream.url = format!("http://{address}");
  let router = app::router(cfg).await.unwrap();
  let response = router
    .oneshot(
      Request::builder()
        .uri("/public")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn upstream_header_timeout_returns_gateway_timeout() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let upstream = tokio::spawn(async move {
    let (_socket, _) = listener.accept().await.unwrap();
    std::future::pending::<()>().await;
  });
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.upstream.url = format!("http://{address}");
  config.upstream.read_timeout = std::time::Duration::from_secs(1);
  let router = app::router(cfg).await.unwrap();
  let response = router
    .oneshot(
      Request::builder()
        .uri("/public")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
  upstream.abort();
}

#[test]
fn invalid_or_unsafe_configuration_is_rejected_before_startup() {
  let mut cfg = common::config();
  assert!(cfg.validate().is_ok());
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.env = rilgate_gateway::config::Environment::Production;
  assert!(config.validate().is_ok());
  config.pow.signing_key.0 = "short".into();
  assert!(config.validate().is_err());
  config.pow.signing_key.0 = "a".repeat(32);
  config.pow.auth_signing_key.0 = "b".repeat(32);
  assert!(config.validate().is_ok());
  config.pow.policy.score = Some(rilgate_config::schema::policy::Scoring {
    debug_header: rilgate_config::schema::policy::DebugHeader {
      enabled: true,
      ..Default::default()
    },
    ..Default::default()
  });
  assert!(config.validate().is_err());
  config
    .pow
    .policy
    .score
    .as_mut()
    .unwrap()
    .debug_header
    .enabled = false;
  config.pow.verifier_queue_capacity = 0;
  assert!(config.validate().is_err());
  config.pow.verifier_queue_capacity = 8;
  config.upstream.url = "http://example.test?query=1".into();
  assert!(config.validate().is_err());
  config.upstream.url = "http://example.test".into();
  config.web.base_path = "/gateway/../api".into();
  assert!(config.validate().is_err());
}

#[tokio::test]
async fn invalid_policy_difficulty_is_rejected_at_startup() {
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg)
    .unwrap()
    .pow
    .policy
    .rules
    .as_mut()
    .unwrap()[0]
    .challenge
    .as_mut()
    .unwrap()
    .difficulty = Some(0);
  assert!(make_service(&cfg).is_err());
}

#[test]
fn authentication_cookie_names_must_be_http_tokens() {
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  for name in [
    "",
    "access token",
    "access=token",
    "access; Secure",
    "access\r\n",
    "accès",
    "access/token",
  ] {
    config.pow.auth_cookie_name = name.into();
    assert!(config.validate().is_err(), "accepted {name:?}");
  }
  for name in ["rilgate_auth", "custom-access", "access.v2"] {
    config.pow.auth_cookie_name = name.into();
    assert!(config.validate().is_ok(), "rejected {name:?}");
  }
}

#[test]
fn authentication_cookie_can_arrive_in_a_later_cookie_header() {
  let mut headers = HeaderMap::new();
  headers.append("cookie", "other=value".parse().unwrap());
  headers.append(
    "cookie",
    "rilgate_auth=token; preference=light".parse().unwrap(),
  );
  assert_eq!(
    rilgate_gateway::auth::cookies::cookie_value(&headers, "rilgate_auth"),
    Some("token")
  );
}

#[tokio::test]
async fn unavailable_p2pool_returns_service_unavailable_after_valid_presentation() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let port = listener.local_addr().unwrap().port();
  drop(listener);
  let mut cfg = common::config();
  let pow = &mut Arc::get_mut(&mut cfg).unwrap().pow;
  pow.stratum_pool.pools[0].port = port;
  pow.policy.rules.as_mut().unwrap()[0]
    .challenge
    .as_mut()
    .unwrap()
    .algorithm = Some(PowAlgorithm::RandomX);
  pow.policy.rules.as_mut().unwrap()[0]
    .challenge
    .as_mut()
    .unwrap()
    .difficulty = Some(1000);
  let service = make_service(&cfg).unwrap();
  let decision = service.evaluate_request(&HeaderMap::new(), "/protected");
  let presentation = service
    .issue_presentation("example.test".into(), "/protected".into(), &decision)
    .unwrap();
  let router = app::router(cfg).await.unwrap();
  let response = router
    .oneshot(
      Request::builder()
        .uri("/.well-known/test-pow/api/challenge")
        .method("POST")
        .header("content-type", "application/json")
        .header("host", "example.test")
        .body(Body::from(
          serde_json::json!({"presentation_token": presentation}).to_string(),
        ))
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
  assert_eq!(response.headers()["cache-control"], "no-store");
}

#[tokio::test]
async fn solver_versions_worker_and_wasm_urls_with_the_embedded_artifact_digest() {
  use sha2::{Digest, Sha256};
  let router = app::router(common::config()).await.unwrap();
  let response = router
    .clone()
    .oneshot(
      Request::builder()
        .uri("/.well-known/test-pow/static/randomx/solver.js")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::OK);
  assert_eq!(
    response.headers()["cache-control"],
    "public, max-age=0, must-revalidate"
  );
  let source = String::from_utf8(
    to_bytes(response.into_body(), 100_000)
      .await
      .unwrap()
      .to_vec(),
  )
  .unwrap();
  let wasm = include_bytes!("../../../web/static/js/solvers/randomx/randomx_wasm.wasm");
  let revision = hex::encode(Sha256::digest(wasm));
  assert!(source.contains(&format!("randomx_wasm.wasm?v={revision}")));
  assert!(source.contains(&format!("randomx-worker.js?v={revision}")));
  assert!(!source.contains("__WASM_REVISION__"));
  let response = router
    .oneshot(
      Request::builder()
        .uri(format!(
          "/.well-known/test-pow/static/randomx/randomx_wasm.wasm?v={revision}"
        ))
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::OK);
  assert_eq!(response.headers()["content-type"], "application/wasm");
  assert_eq!(
    to_bytes(response.into_body(), 1_000_000)
      .await
      .unwrap()
      .as_ref(),
    wasm
  );
}
