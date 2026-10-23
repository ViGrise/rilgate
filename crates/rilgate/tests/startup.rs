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

fn command(contents: &str, args: &[&str], variables: &[(&str, &str)]) -> std::process::Output {
  let directory = tempfile::tempdir().unwrap();
  let config = directory.path().join("config.yml");
  std::fs::write(&config, contents).unwrap();
  std::process::Command::new(env!("CARGO_BIN_EXE_rilgate"))
    .current_dir(directory.path())
    .env_clear()
    .envs(variables.iter().copied())
    .args(["--config", config.to_str().unwrap()])
    .args(args)
    .output()
    .unwrap()
}
const SECRET: &str = "startup-test-secret-with-at-least-32-bytes";

#[test]
fn every_distribution_template_validates_without_network_dependencies() {
  for contents in [
    include_str!("../../../config/rilgate.example.yml"),
    include_str!("../../../config/rilgate.production.yml"),
    include_str!("../../../config/rilgate.custom.example.yml"),
    include_str!("../../../config/rilgate.randomx.example.yml"),
  ] {
    let result = command(
      contents,
      &["config", "check"],
      &[("RILGATE_SECURITY_SECRET", SECRET)],
    );
    assert!(
      result.status.success(),
      "{}",
      String::from_utf8_lossy(&result.stderr)
    );
    assert!(!String::from_utf8_lossy(&result.stdout).contains(SECRET));
    assert!(!String::from_utf8_lossy(&result.stderr).contains(SECRET));
  }
}

#[test]
fn configuration_show_is_valid_yaml_and_redacts_expanded_secrets() {
  let template = include_str!("../../../config/rilgate.production.yml");
  for args in [vec!["config", "show"], vec!["config", "show", "--defaults"]] {
    let result = command(template, &args, &[("RILGATE_SECURITY_SECRET", SECRET)]);
    assert!(
      result.status.success(),
      "{}",
      String::from_utf8_lossy(&result.stderr)
    );
    let output = String::from_utf8(result.stdout).unwrap();
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&output).unwrap();
    assert_eq!(value["security"]["secret"], "[redacted]");
    assert!(!output.contains(SECRET));
    if args.contains(&"--defaults") {
      assert_eq!(value["pow"]["default_algorithm"], "fast");
      assert_eq!(value["security"]["auth"]["ttl"], "6h");
      assert_eq!(value["environment"], "production");
    }
  }
}

#[test]
fn configuration_warnings_are_reported_and_fail_strict_checks() {
  let contents = "server:\n  upstream:\n    url: http://127.0.0.1:3000\n  metrics_enabled: true\nsecurity:\n  secret: \"${RILGATE_SECURITY_SECRET}\"\n";
  let variables = [("RILGATE_SECURITY_SECRET", SECRET)];
  let result = command(contents, &["config", "check"], &variables);
  let stderr = String::from_utf8_lossy(&result.stderr);
  assert!(result.status.success(), "{stderr}");
  assert!(
    stderr.contains("warning: server.metrics_enabled:") && stderr.contains("[metrics-unreachable]"),
    "{stderr}"
  );
  let strict = command(contents, &["config", "check", "--strict"], &variables);
  let stderr = String::from_utf8_lossy(&strict.stderr);
  assert!(!strict.status.success(), "{stderr}");
  assert!(stderr.contains("1 warning"), "{stderr}");
  let clean = contents.replace("  metrics_enabled: true\n", "");
  assert!(
    command(&clean, &["config", "check", "--strict"], &variables)
      .status
      .success()
  );
}

#[test]
fn weak_signing_keys_fail_in_every_environment() {
  let strong = "explicit-test-key-with-at-least-32-bytes";
  for environment in ["development", "production"] {
    for (challenge, auth, previous) in [
      ("short", strong, strong),
      (strong, "change-me-to-a-key-of-at-least-32-bytes", strong),
      (strong, strong, "short"),
    ] {
      let contents = format!(
        "environment: {environment}\nserver:\n  upstream:\n    url: http://127.0.0.1:3000\nsecurity:\n  keys:\n    challenge: {challenge}\n    auth: {auth}\n    challenge_rotation:\n      active_id: next\n      previous:\n        current: {previous}\n"
      );
      let result = command(&contents, &["config", "check"], &[]);
      let error = String::from_utf8_lossy(&result.stderr);
      assert!(!result.status.success(), "{environment}: {contents}");
      assert!(error.contains("security.keys"), "{error}");
    }
  }
}

#[test]
fn invalid_configuration_fails_without_revealing_keys() {
  let template = include_str!("../../../config/rilgate.production.yml");
  for (contents, expected) in [
    (template.to_string(), "RILGATE_SECURITY_SECRET"),
    (template.replace("mode: balanced", "mode: balanced\n  rules: []"), "policy"),
    (template.replace("upstream:", "upsteam:"), "server"),
    (template.replace("mode: balanced", "mode: custom\n  rules:\n    - name: bad-regex\n      when: {path: {regex: '['}}\n      action: deny"), "regex"),
  ] {
    let variables = if expected == "RILGATE_SECURITY_SECRET" { vec![] } else { vec![("RILGATE_SECURITY_SECRET", SECRET)] };
    let result = command(&contents, &["config", "check"], &variables);
    assert!(!result.status.success());
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(error.contains(expected), "{error}");
    assert!(!error.contains(SECRET));
    assert!(!error.contains("panicked"), "{error}");
  }
}

#[test]
fn environment_expansion_cannot_inject_yaml_structure() {
  let template = include_str!("../../../config/rilgate.production.yml");
  let secret = "secure-value-with-32-bytes\npolicy: {mode: off}";
  let result = command(
    template,
    &["config", "show", "--defaults"],
    &[("RILGATE_SECURITY_SECRET", secret)],
  );
  assert!(
    result.status.success(),
    "{}",
    String::from_utf8_lossy(&result.stderr)
  );
  let value: serde_yaml_ng::Value = serde_yaml_ng::from_slice(&result.stdout).unwrap();
  assert_eq!(value["policy"]["mode"], "balanced");
}

#[test]
fn executable_dispatches_web_commands_without_binding_a_socket() {
  let directory = tempfile::tempdir().unwrap();
  let resources = directory.path().join("resources");
  let template = include_str!("../../../config/rilgate.production.yml");
  for operation in ["init", "validate", "diff"] {
    let result = command(
      template,
      &["web", operation, resources.to_str().unwrap()],
      &[("RILGATE_SECURITY_SECRET", SECRET)],
    );
    assert!(
      result.status.success(),
      "{operation}: {}",
      String::from_utf8_lossy(&result.stderr)
    );
  }
}

#[test]
fn environment_only_configuration_and_overrides_work_without_a_file() {
  let directory = tempfile::tempdir().unwrap();
  let result = std::process::Command::new(env!("CARGO_BIN_EXE_rilgate"))
    .current_dir(directory.path())
    .env_clear()
    .env("RILGATE_SECURITY_SECRET", SECRET)
    .env("RILGATE_SERVER_UPSTREAM_URL", "http://127.0.0.1:3000")
    .env("RILGATE_SERVER_LISTEN", "127.0.0.1:8081")
    .env(
      "RILGATE_SERVER_OPERATIONS_LISTEN",
      "127.0.0.1:9091,[::1]:9092",
    )
    .env("RILGATE_PORT", "9090")
    .args(["config", "show", "--defaults"])
    .output()
    .unwrap();
  assert!(
    result.status.success(),
    "{}",
    String::from_utf8_lossy(&result.stderr)
  );
  let value: serde_yaml_ng::Value = serde_yaml_ng::from_slice(&result.stdout).unwrap();
  assert_eq!(value["server"]["listen"], "127.0.0.1:9090");
  assert_eq!(value["server"]["operations"]["listen"][0], "127.0.0.1:9091");
  assert_eq!(value["server"]["operations"]["listen"][1], "[::1]:9092");
}

fn show(
  directory: &std::path::Path,
  variables: &[(&str, &str)],
  args: &[&str],
) -> std::process::Output {
  std::process::Command::new(env!("CARGO_BIN_EXE_rilgate"))
    .current_dir(directory)
    .env_clear()
    .env("RILGATE_SECURITY_SECRET", SECRET)
    .envs(variables.iter().copied())
    .args(args)
    .args(["config", "show", "--defaults"])
    .output()
    .unwrap()
}

#[test]
fn flags_override_variables_which_override_yaml_with_schema_types() {
  let directory = tempfile::tempdir().unwrap();
  std::fs::write(
    directory.path().join("rilgate.yml"),
    "server:\n  upstream:\n    url: http://127.0.0.1:3000\n  listen: 127.0.0.1:7000\n  max_connections: 10\nsecurity:\n  secret: \"${RILGATE_SECURITY_SECRET}\"\n",
  )
  .unwrap();
  let result = show(
    directory.path(),
    &[
      ("RILGATE_SERVER_LISTEN", "127.0.0.1:7001"),
      ("RILGATE_SERVER_OPERATIONS_LISTEN", "127.0.0.1:9091"),
      ("RILGATE_SERVER_MAX_CONNECTIONS", "20"),
    ],
    &[
      "--config",
      "rilgate.yml",
      "--server-operations-listen",
      "127.0.0.1:9092,[::1]:9093",
      "--server-listen",
      "127.0.0.1:7002",
      "--security-auth-ttl",
      "2h",
      "--server-trusted-proxies",
      "10.0.0.0/8,192.0.2.1/32",
      "--server-metrics-enabled",
      "--pow-p2pool-address",
      "127.0.0.1:3334",
    ],
  );
  assert!(
    result.status.success(),
    "{}",
    String::from_utf8_lossy(&result.stderr)
  );
  let value: serde_yaml_ng::Value = serde_yaml_ng::from_slice(&result.stdout).unwrap();
  assert_eq!(value["server"]["listen"], "127.0.0.1:7002");
  assert_eq!(value["server"]["operations"]["listen"][0], "127.0.0.1:9092");
  assert_eq!(value["server"]["operations"]["listen"][1], "[::1]:9093");
  assert_eq!(value["server"]["max_connections"], 20);
  assert_eq!(value["server"]["metrics_enabled"], true);
  assert_eq!(value["server"]["trusted_proxies"][1], "192.0.2.1/32");
  assert_eq!(value["security"]["auth"]["ttl"], "2h");
  assert_eq!(value["pow"]["p2pool"]["address"], "127.0.0.1:3334");
  assert_eq!(value["pow"]["p2pool"]["sessions"]["max"], 64);
}

fn listen_file(listen: &str) -> String {
  format!("server:\n  upstream:\n    url: http://127.0.0.1:3000\n  listen: {listen}\n")
}

#[test]
fn default_configuration_is_found_in_the_working_directory() {
  let directory = tempfile::tempdir().unwrap();
  std::fs::write(
    directory.path().join("rilgate.yml"),
    listen_file("127.0.0.1:7010"),
  )
  .unwrap();
  let result = show(directory.path(), &[], &[]);
  assert!(
    result.status.success(),
    "{}",
    String::from_utf8_lossy(&result.stderr)
  );
  let value: serde_yaml_ng::Value = serde_yaml_ng::from_slice(&result.stdout).unwrap();
  assert_eq!(value["server"]["listen"], "127.0.0.1:7010");
}

#[test]
fn default_configuration_prefers_the_working_directory_and_warns() {
  let directory = tempfile::tempdir().unwrap();
  std::fs::create_dir(directory.path().join("config")).unwrap();
  std::fs::write(
    directory.path().join("config/rilgate.yml"),
    listen_file("127.0.0.1:7011"),
  )
  .unwrap();
  std::fs::write(
    directory.path().join("rilgate.yml"),
    listen_file("127.0.0.1:7012"),
  )
  .unwrap();
  let result = show(directory.path(), &[], &[]);
  assert!(
    result.status.success(),
    "{}",
    String::from_utf8_lossy(&result.stderr)
  );
  let value: serde_yaml_ng::Value = serde_yaml_ng::from_slice(&result.stdout).unwrap();
  assert_eq!(value["server"]["listen"], "127.0.0.1:7012");
  let stderr = String::from_utf8_lossy(&result.stderr);
  assert!(
    stderr.contains("ignoring configuration file config/rilgate.yml; using rilgate.yml"),
    "{stderr}"
  );
}

#[test]
fn a_single_default_configuration_loads_without_warnings() {
  let directory = tempfile::tempdir().unwrap();
  std::fs::write(
    directory.path().join("rilgate.yml"),
    listen_file("127.0.0.1:7013"),
  )
  .unwrap();
  let result = show(directory.path(), &[], &[]);
  assert!(result.status.success());
  assert!(
    result.stderr.is_empty(),
    "{}",
    String::from_utf8_lossy(&result.stderr)
  );
}

#[test]
fn a_binding_prefix_flag_enables_client_ip_binding() {
  let directory = tempfile::tempdir().unwrap();
  let upstream = ("RILGATE_SERVER_UPSTREAM_URL", "http://127.0.0.1:3000");
  let result = show(
    directory.path(),
    &[upstream],
    &["--security-auth-bind-client-ip-ipv4-prefix", "16"],
  );
  assert!(
    result.status.success(),
    "{}",
    String::from_utf8_lossy(&result.stderr)
  );
  let value: serde_yaml_ng::Value = serde_yaml_ng::from_slice(&result.stdout).unwrap();
  let bind = &value["security"]["auth"]["bind"];
  assert_eq!(bind["client_ip"]["ipv4_prefix"], 16);
  assert_eq!(bind["client_ip"]["ipv6_prefix"], 64);

  let result = show(
    directory.path(),
    &[
      upstream,
      ("RILGATE_SECURITY_AUTH_BIND_USER_AGENT", "family"),
      ("RILGATE_SECURITY_AUTH_BIND_HEADERS", "accept-language"),
    ],
    &["--security-auth-bind-proxy-headers", "x-ja4"],
  );
  assert!(result.status.success());
  let value: serde_yaml_ng::Value = serde_yaml_ng::from_slice(&result.stdout).unwrap();
  let bind = &value["security"]["auth"]["bind"];
  assert_eq!(bind["user_agent"], "family");
  assert_eq!(bind["headers"][0], "accept-language");
  assert_eq!(bind["proxy_headers"][0], "x-ja4");
  assert!(bind.get("client_ip").is_none() || bind["client_ip"].is_null());

  let result = show(
    directory.path(),
    &[upstream],
    &["--security-auth-bind-client-ip-ipv4-prefix", "40"],
  );
  let error = String::from_utf8_lossy(&result.stderr);
  assert!(!result.status.success());
  assert!(
    error.contains("security.auth.bind.client_ip.ipv4_prefix"),
    "{error}"
  );
}

#[test]
fn invalid_override_values_name_their_flag_or_variable() {
  let directory = tempfile::tempdir().unwrap();
  let upstream = ("RILGATE_SERVER_UPSTREAM_URL", "http://127.0.0.1:3000");
  let result = show(
    directory.path(),
    &[upstream, ("RILGATE_SERVER_MAX_CONNECTIONS", "many")],
    &[],
  );
  let error = String::from_utf8_lossy(&result.stderr);
  assert!(!result.status.success());
  assert!(
    error.contains("server.max_connections (from RILGATE_SERVER_MAX_CONNECTIONS)"),
    "{error}"
  );
  let result = show(
    directory.path(),
    &[upstream],
    &["--pow-verification", "remote"],
  );
  let error = String::from_utf8_lossy(&result.stderr);
  assert!(
    error.contains("pow.verification (from --pow-verification)"),
    "{error}"
  );
}

#[test]
fn env_file_supplies_settings_before_flags_are_read() {
  let directory = tempfile::tempdir().unwrap();
  std::fs::write(
    directory.path().join("gateway.env"),
    "RILGATE_SERVER_UPSTREAM_URL=http://127.0.0.1:3100\nRILGATE_SITE_NAME=Edge\n",
  )
  .unwrap();
  let result = show(directory.path(), &[], &["--env-file", "gateway.env"]);
  assert!(
    result.status.success(),
    "{}",
    String::from_utf8_lossy(&result.stderr)
  );
  let value: serde_yaml_ng::Value = serde_yaml_ng::from_slice(&result.stdout).unwrap();
  assert_eq!(value["server"]["upstream"]["url"], "http://127.0.0.1:3100");
  assert_eq!(value["site"]["name"], "Edge");
  let missing = show(directory.path(), &[], &["--env-file", "absent.env"]);
  assert!(String::from_utf8_lossy(&missing.stderr).contains("loading environment file absent.env"));
}

#[test]
fn environment_references_follow_schema_types_without_coercing_literal_yaml() {
  let source = r#"
server:
  max_connections: "${CONNECTIONS}"
  metrics_enabled: "${METRICS}"
  upstream:
    url: http://127.0.0.1:3000
    timeout:
      connect: "${CONNECT_TIMEOUT}"
security:
  secret: "${RILGATE_SECURITY_SECRET}"
pow:
  rate_limit:
    client:
      refill_per_second: "${REFILL}"
policy:
  mode: custom
  score:
    initial: "${SCORE}"
    thresholds:
      - min: 0
        action: challenge
        challenge:
          difficulty: "${DIFFICULTY}"
"#;
  let variables = [
    (
      "RILGATE_SECURITY_SECRET",
      "12345678901234567890123456789012",
    ),
    ("CONNECTIONS", "123"),
    ("METRICS", "true"),
    ("CONNECT_TIMEOUT", "300ms"),
    ("REFILL", "0.2"),
    ("SCORE", "-10"),
    ("DIFFICULTY", "2"),
  ];
  let result = command(source, &["config", "show", "--defaults"], &variables);
  assert!(
    result.status.success(),
    "{}",
    String::from_utf8_lossy(&result.stderr)
  );
  let value: serde_yaml_ng::Value = serde_yaml_ng::from_slice(&result.stdout).unwrap();
  assert_eq!(value["server"]["max_connections"], 123);
  assert_eq!(value["server"]["metrics_enabled"], true);
  assert_eq!(value["server"]["upstream"]["timeout"]["connect"], "300ms");
  assert_eq!(value["policy"]["score"]["initial"], -10);
  for source in [
    source.replace("${CONNECTIONS}", "123"),
    source.replace("${METRICS}", "true"),
  ] {
    let result = command(&source, &["config", "check"], &variables);
    assert!(!result.status.success());
  }
  let mut invalid = variables;
  invalid[1].1 = "not-a-number-sensitive-value";
  let result = command(source, &["config", "check"], &invalid);
  let error = String::from_utf8_lossy(&result.stderr);
  assert!(!result.status.success());
  assert!(error.contains("server.max_connections"), "{error}");
  assert!(!error.contains("not-a-number-sensitive-value"));
}

#[test]
fn remote_order_and_endpoint_environment_and_flag_overrides_resolve() {
  let contents = "environment: development\nserver: {upstream: {url: 'http://127.0.0.1:3000'}}\nsecurity: {secret: '${RILGATE_SECURITY_SECRET}'}\npow: {algorithms: {randomx: {verify: {order: [local]}}}}\n";
  let vars = [
    ("RILGATE_SECURITY_SECRET", SECRET),
    (
      "RILGATE_POW_ALGORITHMS_RANDOMX_VERIFY_ORDER",
      "remote,local",
    ),
    (
      "RILGATE_POW_ALGORITHMS_RANDOMX_VERIFY_REMOTE_ENDPOINT",
      "http://env.test:9443",
    ),
  ];
  for (flags, endpoint, order) in [
    (vec![], "http://env.test:9443", vec!["remote", "local"]),
    (
      vec![
        "--pow-algorithms-randomx-verify-order",
        "local,remote",
        "--pow-algorithms-randomx-verify-remote-endpoint",
        "http://flag.test:9443",
      ],
      "http://flag.test:9443",
      vec!["local", "remote"],
    ),
  ] {
    let mut args = vec!["config", "show", "--defaults"];
    args.extend(flags);
    let output = command(contents, &args, &vars);
    assert!(
      output.status.success(),
      "{}",
      String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_slice(&output.stdout).unwrap();
    let verify = &value["pow"]["algorithms"]["randomx"]["verify"];
    assert_eq!(verify["remote"]["endpoint"], endpoint);
    assert_eq!(verify["order"], serde_yaml_ng::to_value(order).unwrap());
  }
}
