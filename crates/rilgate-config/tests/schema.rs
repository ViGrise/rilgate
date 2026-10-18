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
  assert_eq!(cfg.listen_addresses, ["[::1]:9090".parse().unwrap()]);
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

const FEED: &str = "policy:\n  mode: custom\n  ip_sets:\n    feed: {url: 'http://127.0.0.1:9/ips.json', format: ip_prefixes_json}\n  rules:\n    - {name: f, when: {client_ip: {in_sets: [feed]}}, action: allow}\n";

#[test]
fn remote_ip_sets_require_https_in_production() {
  let error = format!("{:#}", resolve(&format!("{BASE}{FEED}")).unwrap_err());
  assert!(
    error.contains("policy.ip_sets.feed.url: production requires https"),
    "{error}"
  );
  assert!(resolve(&format!("environment: development\n{BASE}{FEED}")).is_ok());
  assert!(resolve(&format!("{BASE}{}", FEED.replace("http://", "https://"))).is_ok());
  assert!(resolve(&format!("{BASE}{}", FEED.replace("http://", "HTTPS://"))).is_ok());
}

#[test]
fn resolved_ip_sets_round_trip_through_serialization() {
  let text = FEED.replace("http://", "https://").replace(
    "format: ip_prefixes_json}",
    "format: ip_prefixes_json, refresh: 36h, max_stale: 3d}",
  );
  let config = resolve(&format!("{BASE}{text}")).unwrap();
  let shown = serde_yaml_ng::to_string(&config.pow.policy).unwrap();
  assert!(shown.contains("ip_prefixes_json"), "{shown}");
  let again: rilgate_config::schema::policy::Definition = serde_yaml_ng::from_str(&shown).unwrap();
  assert_eq!(serde_yaml_ng::to_string(&again).unwrap(), shown);
  let feed = &again.ip_sets["feed"];
  assert_eq!(
    feed.refresh,
    Some(std::time::Duration::from_secs(36 * 3600))
  );
  assert_eq!(
    feed.max_stale,
    Some(std::time::Duration::from_secs(3 * 86_400))
  );
}

#[test]
fn auth_binding_is_disabled_by_default_and_enabled_by_any_component() {
  assert!(!resolve(BASE).unwrap().pow.auth_binding.enabled());
  let cfg = resolve(&format!(
    "{BASE}  auth:\n    bind:\n      client_ip: {{}}\n      user_agent: family\n      headers: [Accept-Language]\n      proxy_headers: [x-ja4]\n"
  ))
  .unwrap();
  let binding = &cfg.pow.auth_binding;
  assert!(binding.enabled());
  assert_eq!(binding.client_ip.unwrap().ipv4_prefix, 24);
  assert_eq!(binding.client_ip.unwrap().ipv6_prefix, 64);
  assert_eq!(binding.headers, ["Accept-Language"]);
  assert_eq!(binding.proxy_headers, ["x-ja4"]);
}

#[test]
fn invalid_auth_binding_reports_the_configuration_path() {
  for (bind, path) in [
    (
      "client_ip: {ipv4_prefix: 33}",
      "security.auth.bind.client_ip.ipv4_prefix: expected 0 to 32",
    ),
    (
      "client_ip: {ipv6_prefix: 129}",
      "security.auth.bind.client_ip.ipv6_prefix: expected 0 to 128",
    ),
    (
      "headers: ['not a header']",
      "security.auth.bind.headers: \"not a header\"",
    ),
    (
      "headers: [x-a]\n      proxy_headers: [X-A]",
      "security.auth.bind.proxy_headers",
    ),
    ("user_agent: browser", "security.auth.bind.user_agent"),
    ("client_ip: {prefix: 24}", "security.auth.bind.client_ip"),
  ] {
    let error = resolve(&format!("{BASE}  auth:\n    bind:\n      {bind}\n"))
      .unwrap_err()
      .to_string();
    assert!(error.contains(path), "{bind}: {error}");
  }
}

#[test]
fn auth_binding_round_trips_through_serialization() {
  let text = format!(
    "{BASE}  auth:\n    bind:\n      client_ip: {{ipv4_prefix: 16}}\n      user_agent: full\n      headers: [accept-language]\n"
  );
  let document = parse(&text).unwrap();
  let shown = serde_yaml_ng::to_string(&document).unwrap();
  assert!(shown.contains("ipv4_prefix: 16"), "{shown}");
  assert!(shown.contains("user_agent: full"), "{shown}");
  let again: rilgate_config::schema::Document = serde_yaml_ng::from_str(&shown).unwrap();
  assert_eq!(serde_yaml_ng::to_string(&again).unwrap(), shown);
  assert!(
    !serde_yaml_ng::to_string(&parse(BASE).unwrap())
      .unwrap()
      .contains("client_ip")
  );
}

fn with_listen(listen: &str) -> String {
  BASE.replace("  upstream:", &format!("  listen: {listen}\n  upstream:"))
}
fn resolve_with_port(listen: &str, port: u16) -> anyhow::Result<rilgate_config::Config> {
  let file = tempfile::NamedTempFile::new().unwrap();
  std::fs::write(file.path(), with_listen(listen)).unwrap();
  rilgate_config::Configuration::load(rilgate_config::Overrides {
    config: Some(file.path().to_owned()),
    port: Some(port),
    ..Default::default()
  })
}

#[test]
fn listen_accepts_a_string_or_a_list() {
  let single = resolve(&with_listen("':8080'")).unwrap();
  assert_eq!(single.listen_addresses, ["0.0.0.0:8080".parse().unwrap()]);
  let many = resolve(&with_listen("[':8080', '[::]:8443', '127.0.0.1:9000']")).unwrap();
  assert_eq!(
    many.listen_addresses,
    [
      "0.0.0.0:8080".parse().unwrap(),
      "[::]:8443".parse().unwrap(),
      "127.0.0.1:9000".parse().unwrap()
    ]
  );
}

#[test]
fn listen_rejects_duplicates_empty_and_oversized_lists() {
  let error = resolve(&with_listen("[':8080', '0.0.0.0:8080']")).unwrap_err();
  assert!(
    error.to_string().contains("server.listen[1]: duplicate"),
    "{error}"
  );
  assert!(resolve(&with_listen("[]")).is_err());
  let many = (1..=17)
    .map(|port| format!("'127.0.0.1:{port}'"))
    .collect::<Vec<_>>();
  assert!(resolve(&with_listen(&format!("[{}]", many[..16].join(",")))).is_ok());
  let error = resolve(&with_listen(&format!("[{}]", many.join(",")))).unwrap_err();
  assert!(error.to_string().contains("server.listen"), "{error}");
}

#[test]
fn listen_errors_name_the_offending_entry() {
  let error = resolve(&with_listen("[':8080', 'localhost:1']")).unwrap_err();
  assert!(
    format!("{error:#}").contains("server.listen[1]"),
    "{error:#}"
  );
  let error = resolve(&with_listen("'localhost:1'")).unwrap_err();
  assert!(format!("{error:#}").contains("server.listen:"), "{error:#}");
}

#[test]
fn port_override_applies_to_every_listen_address() {
  let cfg = resolve_with_port("['127.0.0.1:1', '[::1]:2']", 9).unwrap();
  assert_eq!(
    cfg.listen_addresses,
    ["127.0.0.1:9".parse().unwrap(), "[::1]:9".parse().unwrap()]
  );
  let error = resolve_with_port("['127.0.0.1:1', '127.0.0.1:2']", 9).unwrap_err();
  assert!(error.to_string().contains("duplicate"), "{error:#}");
}

#[test]
fn listen_round_trips_with_a_single_address_as_a_string() {
  let single = parse(&with_listen("':8080'")).unwrap();
  let yaml = serde_yaml_ng::to_string(&single.server).unwrap();
  assert!(
    yaml.contains("listen: :8080") || yaml.contains("listen: ':8080'"),
    "{yaml}"
  );
  let many = parse(&with_listen("[':8080', '[::]:8443']")).unwrap();
  let yaml = serde_yaml_ng::to_string(&many.server).unwrap();
  let back: rilgate_config::schema::server::Server = serde_yaml_ng::from_str(&yaml).unwrap();
  assert_eq!(back.listen, many.server.listen);
  assert!(yaml.contains("- "), "{yaml}");
}
