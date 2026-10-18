use axum::{
  Router,
  body::Body,
  http::{Request, StatusCode},
};
use rilgate_config::loader::parse;
use rilgate_test_utils::gateway::TestGateway;
use tower::ServiceExt;

#[tokio::test]
async fn policy_uses_request_method_and_trusted_ip_without_intercepting_internal_routes() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let upstream = listener.local_addr().unwrap();
  let origin = tokio::spawn(async move {
    axum::serve(listener, Router::new().fallback(|| async { "origin" }))
      .await
      .unwrap();
  });
  let source = format!(
    r#"
server:
  upstream:
    url: http://{upstream}
  trusted_proxies: ['127.0.0.1/32', '::1/128']
security:
  secret: trusted-policy-flow-secret-with-at-least-32-bytes
policy:
  mode: custom
  rules:
    - name: well-known
      when: {{path: {{prefix: '/.well-known/'}}}}
      action: allow
    - name: suspicious-post
      when:
        method: {{exact: POST}}
        client_ip: {{cidr: 198.51.100.0/24}}
      action: deny
"#
  );
  for (trusted, expected) in [(true, StatusCode::FORBIDDEN), (false, StatusCode::OK)] {
    let document = parse(&if trusted {
      source.clone()
    } else {
      source.replace("['127.0.0.1/32', '::1/128']", "[]")
    })
    .unwrap();
    let config = document.resolve().unwrap();
    let gateway = TestGateway::start(rilgate::app::build(config).await.unwrap())
      .await
      .unwrap();
    for (method, path, status) in [
      ("POST", "/protected", expected),
      ("GET", "/protected", StatusCode::OK),
      ("GET", "/.well-known/rilgate/unknown", StatusCode::NOT_FOUND),
      (
        "POST",
        "/.well-known/rilgate/api/challenge",
        StatusCode::UNPROCESSABLE_ENTITY,
      ),
    ] {
      let response = gateway
        .clone()
        .oneshot(
          Request::builder()
            .method(method)
            .uri(path)
            .header("host", "example.test")
            .header("x-forwarded-for", "198.51.100.10")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap(),
        )
        .await
        .unwrap();
      assert_eq!(
        response.status(),
        status,
        "trusted={trusted}, {method} {path}"
      );
    }
    let response = gateway
      .oneshot(
        Request::builder()
          .uri("/protected")
          .header("x-forwarded-for", "invalid")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(
      response.status(),
      if trusted {
        StatusCode::BAD_REQUEST
      } else {
        StatusCode::OK
      }
    );
  }
  origin.abort();
}

#[tokio::test]
async fn hkdf_secret_supports_portable_proofs_and_rejects_different_instances_secrets() {
  let source = "server:\n  upstream:\n    url: http://127.0.0.1:3000\nsecurity:\n  secret: shared-secret-for-portable-proofs-with-32-bytes\n";
  let a = rilgate_test_utils::gateway::service(&parse(source).unwrap().resolve().unwrap()).unwrap();
  let b = rilgate_test_utils::gateway::service(&parse(source).unwrap().resolve().unwrap()).unwrap();
  let c = rilgate_test_utils::gateway::service(
    &parse(&source.replace("shared-secret", "different-secret"))
      .unwrap()
      .resolve()
      .unwrap(),
  )
  .unwrap();
  let decision = a.evaluate_request(&http::HeaderMap::new(), "/protected");
  let presentation = a
    .issue_presentation("example.test".into(), "/protected".into(), &decision)
    .unwrap();
  assert!(
    c.verify_presentation(&presentation, "example.test")
      .is_err()
  );
  let challenge = serde_yaml_ng::to_value(
    b.issue_challenge(&presentation, "example.test")
      .await
      .unwrap(),
  )
  .unwrap();
  let data = challenge["challenge"].as_str().unwrap();
  let difficulty = challenge["difficulty"].as_u64().unwrap();
  let nonce = (0..1_000_000)
    .map(|n| n.to_string())
    .find(|n| rilgate_pow_hash::sha256::verifier::verify_fast(data, n, difficulty).unwrap())
    .unwrap();
  let receipt = a
    .verify_challenge(
      challenge["challenge_token"].as_str().unwrap(),
      &nonce,
      "example.test",
    )
    .await
    .unwrap();
  let auth = b.exchange_receipt(&receipt, "example.test").unwrap();
  assert!(a.verify_auth_token(&auth, "example.test").is_ok());
  assert!(c.verify_auth_token(&auth, "example.test").is_err());
  assert!(a.verify_auth_token(&auth, "other.test").is_err());
}

#[tokio::test]
async fn remote_ip_sets_load_in_the_background_and_drive_policy() {
  let feed_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let feed_address = feed_listener.local_addr().unwrap();
  let feed = tokio::spawn(async move {
    let app = Router::new().fallback(|| async {
      (
        [("content-type", "application/json")],
        r#"{"prefixes":[{"ipv4Prefix":"198.51.100.0/24"}]}"#,
      )
    });
    axum::serve(feed_listener, app).await.unwrap();
  });

  let origin_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let origin_address = origin_listener.local_addr().unwrap();
  let origin = tokio::spawn(async move {
    axum::serve(
      origin_listener,
      Router::new().fallback(|| async { "origin" }),
    )
    .await
    .unwrap();
  });

  let source = format!(
    r#"
environment: development
server:
  upstream:
    url: http://{origin_address}
  trusted_proxies: ['127.0.0.1/32', '::1/128']
security:
  secret: remote-ip-set-flow-secret-with-at-least-32-bytes
policy:
  mode: custom
  ip_sets:
    feed:
      url: http://{feed_address}/ips.json
      format: ip_prefixes_json
  rules:
    - name: blocked
      when: {{client_ip: {{in_sets: [feed]}}}}
      action: deny
"#
  );
  let config = parse(&source).unwrap().resolve().unwrap();
  let gateway = TestGateway::start(rilgate::app::build(config).await.unwrap())
    .await
    .unwrap();

  let mut status = None;
  for _ in 0..100 {
    let response = gateway
      .clone()
      .oneshot(
        Request::builder()
          .uri("/protected")
          .header("host", "example.test")
          .header("x-forwarded-for", "198.51.100.10")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    status = Some(response.status());
    if status == Some(StatusCode::FORBIDDEN) {
      break;
    }
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
  }
  assert_eq!(status, Some(StatusCode::FORBIDDEN));

  let response = gateway
    .clone()
    .oneshot(
      Request::builder()
        .uri("/protected")
        .header("host", "example.test")
        .header("x-forwarded-for", "192.0.2.10")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::OK);
  let body = axum::body::to_bytes(response.into_body(), usize::MAX)
    .await
    .unwrap();
  assert_eq!(&body[..], b"origin");
  feed.abort();
  origin.abort();
}
