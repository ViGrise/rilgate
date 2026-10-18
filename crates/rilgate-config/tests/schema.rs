use rilgate_config::{PowAlgorithm, loader::parse};

const BASE: &str = "server:\n  upstream:\n    url: http://127.0.0.1:3000\nsecurity:\n  secret: '0123456789abcdef0123456789abcdef'\n";
fn resolve(text: &str) -> anyhow::Result<rilgate_config::Config> {
  parse(text)?.resolve()
}

#[test]
fn minimal_configuration_has_safe_pool_independent_defaults() {
  let cfg = resolve(BASE).unwrap();
  assert!(matches!(cfg.env, rilgate_config::Environment::Production));
  assert_eq!(cfg.app_port, 8080);
  assert_eq!(cfg.pow.default_algorithm, PowAlgorithm::Fast);
  assert!(!cfg.pow.randomx_enabled);
  assert_eq!(cfg.pow.auth_ttl_seconds, 21_600);
  assert_eq!(cfg.pow.challenge_ttl_seconds, 60);
  assert_eq!(cfg.pow.receipt_ttl_seconds, 30);
  assert_eq!(cfg.web.base_path, "/.well-known/rilgate");
  assert_ne!(
    cfg.pow.signing_key.expose(),
    cfg.pow.auth_signing_key.expose()
  );
  assert_eq!(
    cfg.pow.signing_key.expose(),
    resolve(BASE).unwrap().pow.signing_key.expose()
  );
}

#[test]
fn durations_sizes_ipv6_and_resource_settings_resolve_without_rounding() {
  let text = BASE
    .replace(
      "  upstream:",
      "  listen: '[::1]:9090'\n  readiness_timeout: 300ms\n  upstream:",
    )
    .replace(
      "    url:",
      "    max_body_size: 16MiB\n    timeout:\n      connect: 300ms\n      read: 5s\n    url:",
    );
  let cfg = resolve(&text).unwrap();
  assert_eq!(cfg.listen_address.to_string(), "[::1]:9090");
  assert_eq!(cfg.upstream.connect_timeout.as_millis(), 300);
  assert_eq!(cfg.server.readiness_timeout.as_millis(), 300);
  assert_eq!(cfg.upstream.max_body_bytes, 16 * 1024 * 1024);
  assert!(resolve(&format!("{BASE}pow:\n  challenge:\n    ttl: 300ms\n")).is_err());
}

#[test]
fn invalid_schema_fields_report_the_configuration_path() {
  for (extra, path) in [
    ("environment: staging", "environment"),
    ("env: development", "env"),
    ("version: 1", "version"),
    ("serve: {port: 8080}", "serve"),
    ("web: {}", "web"),
    ("pow: {enforce: local_verify}", "pow"),
    ("pow: {verification: unsupported}", "pow.verification"),
    ("pow: {default_algorithm: md5}", "pow.default_algorithm"),
    ("pow: {challenge: {ttl: 10}}", "pow.challenge.ttl"),
    ("pow: {challenge: {ttl: bananas}}", "pow.challenge.ttl"),
    (
      "pow: {rate_limit: {client: {burst: zero, refill_per_second: 1}}}",
      "pow.rate_limit.client.burst",
    ),
    ("site: {resources: {seed: always}}", "site.resources.seed"),
    ("policy: {mode: aggressive}", "policy.mode"),
    ("policy: []", "policy"),
    ("site: []", "site"),
  ] {
    let error = format!("{:#}", resolve(&format!("{BASE}{extra}\n")).unwrap_err());
    assert!(error.contains(path), "expected {path}: {error}");
    assert!(!error.contains("0123456789abcdef"), "{error}");
  }
}

#[test]
fn conflicting_secrets_and_invalid_runtime_settings_are_rejected() {
  let conflict = BASE.replace("  secret:", "  keys: {challenge: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa, auth: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb}\n  secret:");
  assert!(
    resolve(&conflict)
      .unwrap_err()
      .to_string()
      .contains("mutually exclusive")
  );
  for text in [
    BASE.replace("0123456789abcdef0123456789abcdef", "short"),
    BASE.replace("  upstream:", "  max_connections: 0\n  upstream:"),
    BASE.replace("  upstream:", "  trusted_proxies: [invalid]\n  upstream:"),
    BASE.replace("    url:", "    max_body_size: 0MiB\n    url:"),
    BASE.replace(
      "    url:",
      "    max_body_size: 99999999999999999999999999GiB\n    url:",
    ),
    BASE.replace("    url:", "    max_body_size: 16\n    url:"),
    format!("{BASE}policy:\n  mode: custom\n  score:\n    debug_header:\n      enabled: true\n"),
  ] {
    assert!(resolve(&text).is_err(), "{text}");
  }
}

#[test]
fn required_environment_variables_and_expansion_limits_are_enforced() {
  let text = BASE.replace(
    "0123456789abcdef0123456789abcdef",
    "${RILGATE_TEST_MISSING_SECRET_78A29}",
  );
  let error = parse(&text).unwrap_err().to_string();
  assert!(error.contains("security.secret"));
  assert!(error.contains("RILGATE_TEST_MISSING_SECRET_78A29"));
  for secret in ["${BAD", "${BAD:-default}", "${}"] {
    assert!(parse(&BASE.replace("0123456789abcdef0123456789abcdef", secret)).is_err());
  }
  assert!(parse(&" ".repeat(1_048_577)).is_err());
}

#[test]
fn randomx_and_p2pool_verification_require_explicit_infrastructure() {
  assert!(
    resolve(&format!("{BASE}pow:\n  default_algorithm: randomx\n"))
      .unwrap_err()
      .to_string()
      .contains("pow.p2pool.address")
  );
  let config = format!(
    "{BASE}pow:\n  default_algorithm: randomx\n  verification: p2pool_accepted\n  p2pool:\n    address: '[::1]:3333'\n    timeout:\n      connect: 300ms\n    job_sessions:\n      mode: per_challenge\npolicy:\n  mode: custom\n  default_action: challenge\n"
  );
  let cfg = resolve(&config).unwrap();
  assert!(cfg.pow.randomx_enabled);
  assert_eq!(cfg.pow.p2pool.host, "::1");
  assert_eq!(cfg.pow.p2pool.connect_timeout.as_millis(), 300);
  assert_eq!(
    cfg.pow.p2pool.job_session_mode,
    rilgate_pool::JobSessionMode::PerChallenge
  );
  assert!(resolve(&config.replace("per_challenge", "unknown")).is_err());
  assert!(resolve(&config.replace("mode: per_challenge", "mode: pooled\n      max: 0")).is_err());
  assert!(resolve(&config.replace("'[::1]:3333'", "'localhost'")).is_err());
  assert!(
    resolve(&config.replace(
      "mode: custom\n  default_action: challenge",
      "mode: balanced"
    ))
    .is_err()
  );
}

#[test]
fn resolved_output_redacts_every_key_and_password() {
  let config = BASE.replace("  secret: '0123456789abcdef0123456789abcdef'", "  keys:\n    challenge: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n    auth: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n    challenge_rotation:\n      active_id: current\n      previous:\n        old: cccccccccccccccccccccccccccccccc");
  let document = parse(&format!(
    "{config}pow:\n  p2pool:\n    password: private-pool-password\n"
  ))
  .unwrap();
  let cfg = document.resolve().unwrap();
  assert_eq!(
    cfg.pow.signing_key.expose(),
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  );
  assert_eq!(
    cfg.pow.signing_keys.previous["old"].expose(),
    "cccccccccccccccccccccccccccccccc"
  );
  for output in [
    document.display(true).unwrap(),
    document.display(false).unwrap(),
    format!("{cfg:?}"),
  ] {
    for secret in ["aaaaaaaa", "bbbbbbbb", "cccccccc", "private-pool-password"] {
      assert!(!output.contains(secret), "{output}");
    }
    assert!(output.contains("redacted"));
  }
}

#[test]
fn all_rule_challenges_are_validated_before_runtime_construction() {
  for (algorithm, difficulty) in [("fast", 0), ("fast", 65), ("randomx", 999)] {
    let config = format!(
      "{BASE}pow:\n  p2pool: {{address: '127.0.0.1:3333'}}\npolicy:\n  mode: custom\n  rules:\n    - name: invalid\n      when: {{path: {{prefix: /}}}}\n      action: challenge\n      challenge:\n        algorithm: {algorithm}\n        difficulty: {difficulty}\n"
    );
    let error = format!("{:#}", resolve(&config).unwrap_err());
    assert!(error.contains("difficulty"), "{error}");
  }
}

#[test]
fn response_selection_validates_modes_and_path_prefixes() {
  let cfg = resolve(&format!(
    "{BASE}site:\n  responses:\n    format: auto\n    json_paths: [/api]\n"
  ))
  .unwrap();
  assert_eq!(cfg.web.responses.json_paths, vec!["/api"]);
  for extra in [
    "site: {responses: {format: xml}}",
    "site: {responses: {format: html, json_paths: [/api]}}",
    "site: {responses: {json_paths: [relative]}}",
    "site: {responses: {json_paths: [/api/../secret]}}",
  ] {
    assert!(resolve(&format!("{BASE}{extra}\n")).is_err(), "{extra}");
  }
}
