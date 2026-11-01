mod common;
use axum::{
  body::{Body, to_bytes},
  http::Request,
};
use rilgate_test_utils::gateway as app;
use std::{fs, sync::Arc};
use tower::ServiceExt;

fn directive_hashes(policy: &str, name: &str) -> Vec<String> {
  policy
    .split(';')
    .find_map(|directive| {
      let mut words = directive.split_whitespace();
      (words.next() == Some(name)).then(|| {
        words
          .filter(|value| value.starts_with("'sha256-"))
          .map(str::to_owned)
          .collect()
      })
    })
    .unwrap()
}

#[tokio::test]
async fn challenge_policy_matches_body_and_can_be_disabled() {
  for enabled in [true, false] {
    let mut cfg = common::config();
    Arc::get_mut(&mut cfg)
      .unwrap()
      .web
      .content_security_policy
      .enabled = enabled;
    let router = app::router(cfg).await.unwrap();
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
    let policy = response
      .headers()
      .get("content-security-policy")
      .map(|v| v.to_str().unwrap().to_string());
    let html = String::from_utf8(
      to_bytes(response.into_body(), 100_000)
        .await
        .unwrap()
        .to_vec(),
    )
    .unwrap();
    if enabled {
      let policy = policy.unwrap();
      let hashes = rilgate_web::csp::inline_hashes(&html);
      assert_eq!(directive_hashes(&policy, "script-src"), hashes.scripts);
      assert_eq!(directive_hashes(&policy, "style-src"), hashes.styles);
      assert!(!hashes.styles.is_empty());
      assert!(policy.contains("'wasm-unsafe-eval'"));
      assert!(policy.contains("worker-src 'self'"));
    } else {
      assert!(policy.is_none());
    }
    let json = router
      .clone()
      .oneshot(
        Request::builder()
          .uri("/protected")
          .header("accept", "application/json")
          .header("host", "example.test")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    assert!(!json.headers().contains_key("content-security-policy"));
    let data = html
      .split("id=\"pow-bootstrap\">")
      .nth(1)
      .unwrap()
      .split("</script>")
      .next()
      .unwrap();
    let bootstrap: serde_json::Value = serde_json::from_str(data).unwrap();
    let response = router
      .oneshot(
        Request::builder()
          .uri(format!(
            "/.well-known/test-pow/challenge?presentation_token={}",
            bootstrap["presentation_token"].as_str().unwrap()
          ))
          .header("host", "example.test")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(response.headers()["x-frame-options"], "SAMEORIGIN");
    if !enabled {
      assert_eq!(
        response.headers()["content-security-policy"],
        "frame-ancestors 'self'"
      );
    }
  }
}

#[tokio::test]
async fn operator_inline_script_and_external_stylesheet_sources_are_allowed() {
  let root = tempfile::tempdir().unwrap();
  fs::create_dir_all(root.path().join("templates/pages")).unwrap();
  fs::write(
    root.path().join("templates/pages/denied.html.jinja"),
    "<html><script>window.operator = true;</script></html>",
  )
  .unwrap();
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.web.root = Some(root.path().into());
  config.web.stylesheet_path = "https://styles.example.test/theme.css".into();
  config.web.content_security_policy.extra_sources.insert(
    "connect-src".into(),
    vec!["https://api.example.test".into()],
  );
  config.pow.policy.policy.default_action = Some(rilgate_gateway::config::PolicyAction::Deny);
  let router = app::router(cfg).await.unwrap();
  let response = router
    .oneshot(
      Request::builder()
        .uri("/denied")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  let policy = response.headers()["content-security-policy"]
    .to_str()
    .unwrap()
    .to_string();
  let html = String::from_utf8(
    to_bytes(response.into_body(), 100_000)
      .await
      .unwrap()
      .to_vec(),
  )
  .unwrap();
  assert!(html.contains("window.operator = true;"));
  assert_eq!(
    directive_hashes(&policy, "script-src"),
    rilgate_web::csp::inline_hashes(&html).scripts
  );
  assert!(policy.contains("style-src 'self' https://styles.example.test"));
  assert!(policy.contains("connect-src 'self' https://api.example.test"));
}
