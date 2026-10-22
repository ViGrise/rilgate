use rilgate_test_utils::gateway as app;
mod common;
use axum::{
  body::{Body, to_bytes},
  http::{Request, StatusCode},
};
use rilgate_gateway::config::PolicyAction;
use std::{fs, sync::Arc};
use tower::ServiceExt;

#[tokio::test]
async fn branded_pages_negotiate_locales_and_serve_resources_from_configured_paths() {
  let root = tempfile::tempdir().unwrap();
  fs::create_dir_all(root.path().join("i18n")).unwrap();
  fs::write(
    root.path().join("i18n/vi.json"),
    r#"{"challenge.title":"Kiểm tra trình duyệt"}"#,
  )
  .unwrap();
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.web.name = "Example Shield".into();
  config.web.website_url = "https://example.test".into();
  config.web.root = Some(root.path().into());
  config.web.stylesheet_path = "css/shield.css".into();
  config.web.i18n.language_cookie_name = "site_lang".into();
  // Vietnamese needs no declaration: i18n/vi.json is enough to serve it.
  config.web.base_path = "/shield".into();
  config.pow.auth_cookie_name = "site_access".into();
  let router = app::router(cfg).await.unwrap();
  for (uri, cookie, language, expected) in [
    ("/protected", "", "vi-VN, en;q=0.8", "vi"),
    ("/protected", "site_lang=en", "vi", "en"),
    ("/protected?lang=vi", "site_lang=en", "en", "vi"),
  ] {
    let response = router
      .clone()
      .oneshot(
        Request::builder()
          .uri(uri)
          .header("host", "example.test")
          .header("cookie", cookie)
          .header("accept-language", language)
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(response.headers()["content-language"], expected);
    assert_eq!(
      response.headers()["vary"],
      "Accept, Content-Type, X-Requested-With, Accept-Language, Cookie"
    );
    let html = String::from_utf8(
      to_bytes(response.into_body(), 100_000)
        .await
        .unwrap()
        .to_vec(),
    )
    .unwrap();
    assert!(!html.to_lowercase().contains("rilgate"));
    assert!(html.contains("Example Shield"));
    if expected == "vi" {
      assert!(html.contains("Kiểm tra trình duyệt"));
    }
  }
  for path in [
    "/shield/static/css/shield.css",
    "/shield/static/js/challenge.js",
    "/shield/static/js/challenge/runtime.js",
    "/shield/static/js/challenge/registry.js",
    "/shield/static/js/challenge/p2pool.js",
    "/shield/static/js/solvers/fast/adapter.js",
    "/shield/static/js/solvers/fast/solver.js",
    "/shield/static/js/solvers/fast/worker.js",
    "/shield/static/js/solvers/randomx/adapter.js",
    "/shield/static/js/solvers/randomx/solver.js",
    "/shield/static/randomx/randomx-worker.js",
  ] {
    let response = router
      .clone()
      .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
      .await
      .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key("etag"));
    let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("Rilgate"));
  }
  let response = router
    .clone()
    .oneshot(
      Request::builder()
        .uri("/shield/static/%2e%2e%2fsecret")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::NOT_FOUND);
  let response = reqwest::get(format!(
    "http://{}/shield/api/health",
    router.operations_address()
  ))
  .await
  .unwrap();
  assert_eq!(response.status(), StatusCode::OK);
  let health: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
  assert_eq!(health["web"]["generation"], 1);
  assert_eq!(health["web"]["watch_enabled"], false);
}

#[tokio::test]
async fn denied_and_upstream_error_pages_preserve_http_status_and_hide_internal_errors() {
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg)
    .unwrap()
    .pow
    .policy
    .rules
    .as_mut()
    .unwrap()[0]
    .action = PolicyAction::Deny;
  Arc::get_mut(&mut cfg)
    .unwrap()
    .pow
    .policy
    .rules
    .as_mut()
    .unwrap()[0]
    .challenge = None;
  let router = app::router(cfg).await.unwrap();
  let response = router
    .oneshot(
      Request::builder()
        .uri("/protected")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::FORBIDDEN);
  let html = String::from_utf8(
    to_bytes(response.into_body(), 100_000)
      .await
      .unwrap()
      .to_vec(),
  )
  .unwrap();
  assert!(html.contains("Access denied"));
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
  let html = String::from_utf8(
    to_bytes(response.into_body(), 100_000)
      .await
      .unwrap()
      .to_vec(),
  )
  .unwrap();
  assert!(html.contains("Unable to continue"));
  assert!(!html.contains(&address.to_string()));
}
