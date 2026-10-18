mod common;
use axum::{
  body::{Body, to_bytes},
  http::{Request, StatusCode},
};
use rilgate_test_utils::gateway as app;
use serde_json::Value;
use std::{fs, sync::Arc};
use tower::ServiceExt;

#[tokio::test]
async fn ajax_challenge_has_a_valid_same_origin_html_page_and_signed_token() {
  let router = app::router(common::config()).await.unwrap();
  for (method, header, value) in [
    ("GET", "accept", "application/json"),
    ("POST", "content-type", "application/json; charset=utf-8"),
    ("GET", "x-requested-with", "XMLHttpRequest"),
  ] {
    let response = router
      .clone()
      .oneshot(
        Request::builder()
          .method(method)
          .uri("/protected/data")
          .header("host", "example.test")
          .header(header, value)
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["rilgate-challenge"], "required");
    let url = response.headers()["rilgate-challenge-url"]
      .to_str()
      .unwrap()
      .to_string();
    let body: Value =
      serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    assert_eq!(body["challenge_url"], url);
    assert_eq!(body["code"], "proof_required");
    assert_eq!(
      body["endpoints"]["config"],
      "/.well-known/test-pow/api/client-config"
    );
    let page = router
      .clone()
      .oneshot(
        Request::builder()
          .uri(&url)
          .header("host", "example.test")
          .header("accept", "application/json")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert_eq!(page.headers()["content-type"], "text/html; charset=utf-8");
    assert_eq!(
      page.headers()["content-security-policy"],
      "frame-ancestors 'self'"
    );
    assert_eq!(page.headers()["referrer-policy"], "no-referrer");
    let html =
      String::from_utf8(to_bytes(page.into_body(), 100000).await.unwrap().to_vec()).unwrap();
    assert!(html.contains("window.POW_EMBEDDED = true"));
    assert!(html.contains(body["challenge"]["token"].as_str().unwrap()));
    let wrong_host = router
      .clone()
      .oneshot(
        Request::builder()
          .uri(&url)
          .header("host", "different.test")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(wrong_host.status(), StatusCode::UNAUTHORIZED);
  }
}

#[tokio::test]
async fn operator_templates_can_use_an_application_specific_json_shape() {
  let directory = tempfile::tempdir().unwrap();
  fs::create_dir_all(directory.path().join("templates/pages")).unwrap();
  fs::write(
    directory
      .path()
      .join("templates/pages/challenge.json.jinja"),
    r#"{"success":false,"error":{"kind":"verification","url":{{ challenge.url | tojson }}}}"#,
  )
  .unwrap();
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().web.root = Some(directory.path().into());
  let router = app::router(cfg).await.unwrap();
  let response = router
    .oneshot(
      Request::builder()
        .uri("/protected")
        .header("accept", "application/json")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.headers()["rilgate-challenge"], "required");
  let body: Value =
    serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
  assert_eq!(body["success"], false);
  assert_eq!(body["error"]["kind"], "verification");
  assert!(
    body["error"]["url"]
      .as_str()
      .unwrap()
      .starts_with("/.well-known/test-pow/challenge?")
  );
}

#[tokio::test]
async fn json_head_deny_and_forced_path_selection_preserve_policy_semantics() {
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().web.responses.json_paths = vec!["/protected".into()];
  let router = app::router(cfg).await.unwrap();
  let response = router
    .clone()
    .oneshot(
      Request::builder()
        .method("HEAD")
        .uri("/protected")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::FORBIDDEN);
  assert_eq!(response.headers()["content-type"], "application/json");
  assert!(
    to_bytes(response.into_body(), 65536)
      .await
      .unwrap()
      .is_empty()
  );
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.pow.policy.rules.as_mut().unwrap()[0].action =
    rilgate_config::schema::policy::Action::Deny;
  config.pow.policy.rules.as_mut().unwrap()[0].challenge = None;
  let router = app::router(cfg).await.unwrap();
  let response = router
    .oneshot(
      Request::builder()
        .uri("/protected")
        .header("accept", "application/json")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::FORBIDDEN);
  assert!(!response.headers().contains_key("rilgate-challenge"));
  let body: Value =
    serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
  assert_eq!(body["code"], "access_denied");
}
