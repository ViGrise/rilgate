use axum::{
  body::Body,
  http::{Request, StatusCode},
};
use tower::ServiceExt;

#[tokio::test]
async fn application_composes_backends_and_serves_configured_namespace() {
  let config = rilgate_test_utils::config::config();
  let path = format!("{}/api/client-config", config.web.base_path);
  let router =
    rilgate_test_utils::gateway::TestGateway::start(rilgate::app::build(config).await.unwrap())
      .await
      .unwrap();
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

fn check_production(contents: &str) -> std::process::Output {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("production.yml");
  std::fs::write(&path, contents).unwrap();
  std::process::Command::new(env!("CARGO_BIN_EXE_rilgate"))
    .current_dir(directory.path())
    .env_clear()
    .env(
      "POW_SIGNING_KEY",
      "test-proof-secret-with-at-least-32-bytes",
    )
    .env(
      "POW_AUTH_SIGNING_KEY",
      "test-auth-secret-with-at-least-32-bytes",
    )
    .args(["--config", path.to_str().unwrap(), "check-config"])
    .output()
    .unwrap()
}

#[test]
fn production_template_validates_without_external_dependencies() {
  let config = include_str!("../../../config/rilgate.production.yml");
  let result = check_production(config);
  assert!(
    result.status.success(),
    "{}",
    String::from_utf8_lossy(&result.stderr)
  );
  let logs = format!(
    "{}{}",
    String::from_utf8_lossy(&result.stdout),
    String::from_utf8_lossy(&result.stderr)
  );
  assert!(!logs.contains("test-proof-secret"));
  assert!(!logs.contains("test-auth-secret"));
}

#[test]
fn production_rejects_missing_policy_typos_invalid_limits_and_debug_headers() {
  let production = include_str!("../../../config/rilgate.production.yml");
  for contents in [
    "env: production\n".to_owned(),
    "env: production\npow:\n  thresholds: []\n".to_owned(),
    production.replace("max_connections: 1024", "max_connections: 0"),
    production.replace("h2_max_streams: 64", "h2_max_streams: 0"),
    production.replace("max_requests: 512", "max_requets: 512"),
    production.replace("allow_debug_header: false", "allow_debug_header: true"),
    production.replace("trusted_proxies: []", "trusted_proxies: [not-a-network]"),
    production.replace("trusted_proxies: []", "trusted_proxys: []"),
    production.replace("active_id: primary-1", "active_id: bad.id"),
  ] {
    let result = check_production(&contents);
    assert!(
      !result.status.success(),
      "Unexpectedly accepted: {contents}"
    );
  }
}

#[test]
fn production_requires_strong_secrets_and_a_policy_file() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("config.yml");
  std::fs::write(
    &path,
    include_str!("../../../config/rilgate.production.yml"),
  )
  .unwrap();
  for key in [
    "short",
    "change-me-placeholder-key-with-32-bytes",
    "replace-me-placeholder-key-with-32-bytes",
  ] {
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_rilgate"))
      .current_dir(directory.path())
      .env_clear()
      .env("POW_SIGNING_KEY", key)
      .env(
        "POW_AUTH_SIGNING_KEY",
        "test-auth-secret-with-at-least-32-bytes",
      )
      .args(["--config", path.to_str().unwrap(), "check-config"])
      .output()
      .unwrap();
    assert!(!result.status.success());
  }
  let result = std::process::Command::new(env!("CARGO_BIN_EXE_rilgate"))
    .current_dir(directory.path())
    .env_clear()
    .env("ENV", "production")
    .env(
      "POW_SIGNING_KEY",
      "test-proof-secret-with-at-least-32-bytes",
    )
    .env(
      "POW_AUTH_SIGNING_KEY",
      "test-auth-secret-with-at-least-32-bytes",
    )
    .arg("check-config")
    .output()
    .unwrap();
  assert!(!result.status.success());
}
