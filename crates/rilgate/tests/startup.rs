use axum::{
  body::Body,
  http::{Request, StatusCode},
};
use tower::ServiceExt;

#[tokio::test]
async fn application_composes_backends_and_serves_configured_namespace() {
  let config = rilgate_test_utils::config::config();
  let path = format!("{}/api/client-config", config.web.base_path);
  let router = rilgate::app::router(config).await.unwrap();
  let response = router
    .oneshot(
      Request::builder()
        .uri(path)
        .header("host", "example.test")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::OK);
  let body = axum::body::to_bytes(response.into_body(), 1_048_576)
    .await
    .unwrap();
  let body = String::from_utf8(body.to_vec()).unwrap();
  assert!(body.contains("fast"));
  assert!(body.contains("randomx"));
}

#[test]
fn executable_dispatches_web_commands_without_binding_a_socket() {
  let directory = tempfile::tempdir().unwrap();
  let config = directory.path().join("config.yml");
  std::fs::write(&config, "web:\n  name: Startup Test\n").unwrap();
  let resources = directory.path().join("web");
  for operation in ["init", "validate", "diff"] {
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_rilgate"))
      .env_clear()
      .arg("--config")
      .arg(&config)
      .arg("web")
      .arg(operation)
      .arg(&resources)
      .output()
      .unwrap();
    assert!(
      result.status.success(),
      "{operation}: {}",
      String::from_utf8_lossy(&result.stderr)
    );
  }
}
