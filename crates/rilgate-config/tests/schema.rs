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
fn randomx_cache_idle_timeout_resolves_without_rounding() {
  let mut document = parse(BASE).unwrap();
  document.pow.algorithms.randomx.cache.idle_timeout.0 = std::time::Duration::from_secs(8 * 86400);
  assert!(document.resolve().is_err());
  assert_eq!(
    resolve(BASE)
      .unwrap()
      .pow
      .randomx_cache_idle_timeout
      .as_secs(),
    1800
  );
  for (duration, expected) in [("0s", 0), ("300ms", 300), ("7d", 604_800_000)] {
    let config = format!(
      "{BASE}pow:\n  algorithms:\n    randomx:\n      cache:\n        idle_timeout: {duration}\n"
    );
    assert_eq!(
      resolve(&config)
        .unwrap()
        .pow
        .randomx_cache_idle_timeout
        .as_millis(),
      expected
    );
  }
  for duration in ["8d", "garbage"] {
    let config = format!(
      "{BASE}pow:\n  algorithms:\n    randomx:\n      cache:\n        idle_timeout: {duration}\n"
    );
    assert!(
      format!("{:#}", resolve(&config).unwrap_err())
        .contains("pow.algorithms.randomx.cache.idle_timeout")
    );
  }
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
      .contains("pow.stratum_pool.pools")
  );
  let config = format!(
    "{BASE}pow:\n  default_algorithm: randomx\n  verification: stratum_pool_accepted\n  stratum_pool:\n    pools: [{{name: primary, address: '[::1]:3333'}}]\n    timeout:\n      connect: 300ms\n    job_sessions:\n      mode: per_challenge\npolicy:\n  mode: custom\n  default_action: challenge\n"
  );
  let cfg = resolve(&config).unwrap();
  assert!(cfg.pow.randomx_enabled);
  assert_eq!(cfg.pow.stratum_pool.pools[0].host, "::1");
  assert_eq!(cfg.pow.stratum_pool.connect_timeout.as_millis(), 300);
  assert_eq!(
    cfg.pow.stratum_pool.job_session_mode,
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
    "{config}pow:\n  stratum_pool:\n    pools: [{{name: primary, address: localhost:3333, login: {{password: private-pool-password}}}}]\n"
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
      "{BASE}pow:\n  stratum_pool: {{}}\npolicy:\n  mode: custom\n  rules:\n    - name: invalid\n      when: {{path: {{prefix: /}}}}\n      action: challenge\n      challenge:\n        algorithm: {algorithm}\n        difficulty: {difficulty}\n"
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

fn with_operations(listen: &str) -> String {
  BASE.replace(
    "  upstream:",
    &format!("  operations:\n    listen: {listen}\n  upstream:"),
  )
}

#[test]
fn operations_listen_accepts_single_list_and_empty_default() {
  assert!(
    resolve(BASE)
      .unwrap()
      .operations_listen_addresses
      .is_empty()
  );
  for listen in ["'127.0.0.1:9090'", "['127.0.0.1:9090']"] {
    assert_eq!(
      resolve(&with_operations(listen))
        .unwrap()
        .operations_listen_addresses,
      ["127.0.0.1:9090".parse().unwrap()]
    );
  }
  assert_eq!(
    resolve(&with_operations("[':9090', '[::1]:9091']"))
      .unwrap()
      .operations_listen_addresses
      .len(),
    2
  );
  assert!(
    resolve(&with_operations("[]"))
      .unwrap()
      .operations_listen_addresses
      .is_empty()
  );
}

#[test]
fn operations_listen_validation_names_the_path() {
  for (listen, path) in [
    (
      "[':9090', '0.0.0.0:9090']",
      "server.operations.listen[1]: duplicate",
    ),
    ("[':9090', ':8080']", "server.operations.listen[1]:"),
    ("'localhost:9090'", "server.operations.listen:"),
  ] {
    let error = resolve(&with_operations(listen)).unwrap_err();
    assert!(format!("{error:#}").contains(path), "{error:#}");
  }
  let addresses = (1..=17)
    .map(|port| format!("':{port}'"))
    .collect::<Vec<_>>();
  assert!(
    resolve(&with_operations(&format!(
      "[{}]",
      addresses[..16].join(",")
    )))
    .is_ok()
  );
  let error = resolve(&with_operations(&format!("[{}]", addresses.join(",")))).unwrap_err();
  assert!(
    error.to_string().contains("server.operations.listen"),
    "{error:#}"
  );
  assert!(
    parse(&BASE.replace(
      "  upstream:",
      "  operations:\n    unknown: true\n  upstream:"
    ))
    .is_err()
  );
}

#[test]
fn port_override_preserves_operations_listen_addresses() {
  let file = tempfile::NamedTempFile::new().unwrap();
  std::fs::write(
    file.path(),
    with_operations("['127.0.0.1:9090', '[::1]:9091']"),
  )
  .unwrap();
  let cfg = rilgate_config::Configuration::load(rilgate_config::Overrides {
    config: Some(file.path().to_owned()),
    port: Some(9000),
    ..Default::default()
  })
  .unwrap();
  assert_eq!(cfg.listen_addresses[0].port(), 9000);
  assert_eq!(
    cfg.operations_listen_addresses,
    [
      "127.0.0.1:9090".parse().unwrap(),
      "[::1]:9091".parse().unwrap()
    ]
  );
}

#[test]
fn randomx_verification_rejects_invalid_order_endpoint_and_tls() {
  for verify in [
    "{order: []}",
    "{order: [local, local]}",
    "{order: [local, remote, local]}",
    "{remote: {timeout: 99ms}}",
    "{remote: {timeout: 31s}}",
    "{remote: {endpoint: 'https://host/path'}}",
    "{remote: {endpoint: 'https://user@host'}}",
    "{remote: {endpoint: 'https://host?query'}}",
    "{remote: {endpoint: 'ftp://host'}}",
    "{remote: {tls: {client_cert: cert.pem}}}",
    "{remote: {tls: {client_key: key.pem}}}",
  ] {
    let text = format!("{BASE}pow:\n  algorithms:\n    randomx:\n      verify: {verify}\n");
    assert!(resolve(&text).is_err(), "{verify}");
  }
  let text = format!(
    "environment: development\n{BASE}pow:\n  algorithms:\n    randomx:\n      verify: {{order: [remote, local], remote: {{endpoint: 'http://host:9443', timeout: 100ms}}}}\n"
  );
  let cfg = resolve(&text).unwrap();
  assert_eq!(
    cfg.pow.randomx_verify.remote.server_name.as_deref(),
    Some("host")
  );
}

#[test]
fn remote_readiness_requires_local_verification_randomx_challenges_and_remote_only() {
  use rilgate_config::{PowEnforcement, schema::pow::Hasher};
  let mut document = parse(BASE).unwrap();
  document.pow.stratum_pool = Some(Default::default());
  let cfg = document.resolve().unwrap();
  let mut pow = cfg.pow.clone();
  pow.policy = parse(&format!("{BASE}policy:\n  mode: custom\n  rules:\n    - name: all\n      when: {{path: {{prefix: /}}}}\n      action: challenge\n")).unwrap().policy;
  for enforce in [
    PowEnforcement::LocalVerify,
    PowEnforcement::StratumPoolAccepted,
  ] {
    for algorithm in [PowAlgorithm::Fast, PowAlgorithm::RandomX] {
      for order in [
        vec![Hasher::Local],
        vec![Hasher::Remote],
        vec![Hasher::Remote, Hasher::Local],
        vec![Hasher::Local, Hasher::Remote],
      ] {
        pow.enforce = enforce;
        pow.default_algorithm = algorithm;
        pow.randomx_verify.order = order.clone();
        assert_eq!(
          pow.needs_remote_verifier(),
          enforce == PowEnforcement::LocalVerify
            && algorithm == PowAlgorithm::RandomX
            && order == [Hasher::Remote]
        );
      }
    }
  }
  pow.enforce = PowEnforcement::LocalVerify;
  pow.default_algorithm = PowAlgorithm::RandomX;
  pow.randomx_verify.order = vec![Hasher::Remote];
  pow.policy = parse(&format!("{BASE}policy: {{mode: off}}\n"))
    .unwrap()
    .policy;
  assert!(!pow.needs_remote_verifier());
  pow.default_algorithm = PowAlgorithm::Fast;
  pow.policy = parse(&format!("{BASE}policy:\n  mode: custom\n  rules:\n    - name: randomx\n      when: {{path: {{prefix: /}}}}\n      action: challenge\n      challenge: {{algorithm: randomx}}\n")).unwrap().policy;
  assert!(pow.needs_remote_verifier());
}

#[test]
fn renamed_pool_keys_fail_with_the_new_name() {
  for (text, expected) in [
    (
      "pow:\n  p2pool: {}\n",
      "pow.p2pool: renamed to pow.stratum_pool",
    ),
    (
      "pow:\n  verification: p2pool_accepted\n",
      "pow.verification: p2pool_accepted is renamed to stratum_pool_accepted",
    ),
  ] {
    let error = format!("{:#}", parse(&format!("{BASE}{text}")).unwrap_err());
    assert!(error.contains(expected), "{error}");
  }
}

#[test]
fn pool_login_templates_reject_ambiguous_difficulty_and_hide_operator_identity() {
  for user in [
    "wallet",
    "{difficulty}{difficulty}",
    "{wallet}+{difficulty}",
  ] {
    let text = format!(
      "{BASE}pow:\n  stratum_pool:\n    pools:\n      - name: primary\n        address: localhost:3333\n        login: {{user: '{user}'}}\n"
    );
    let error = format!("{:#}", resolve(&text).unwrap_err());
    assert!(error.contains("login.user"), "{error}");
    assert!(!error.contains(user), "{error}");
  }
  let text = format!(
    "{BASE}pow:\n  stratum_pool:\n    pools:\n      - name: primary\n        address: localhost:3333\n        login: {{user: 'private-wallet+{{difficulty}}', password: private-password}}\n"
  );
  let document = parse(&text).unwrap();
  let cfg = document.resolve().unwrap();
  for output in [document.display(true).unwrap(), format!("{cfg:?}")] {
    assert!(!output.contains("private-wallet"), "{output}");
    assert!(!output.contains("private-password"), "{output}");
  }
}

#[test]
fn pool_lists_reject_duplicate_names_invalid_addresses_and_unbounded_failover() {
  for pools in [
    "[]",
    "[{name: same, address: 'localhost:1'}, {name: same, address: 'localhost:2'}]",
    "[{name: primary, address: 'user:password@localhost:1'}]",
    "[{name: BadName, address: 'localhost:1'}]",
  ] {
    assert!(
      resolve(&format!(
        "{BASE}pow:\n  stratum_pool:\n    pools: {pools}\n"
      ))
      .is_err()
    );
  }
  for cooldown in ["0s", "601s"] {
    assert!(
      resolve(&format!(
        "{BASE}pow:\n  stratum_pool:\n    failover: {{cooldown: {cooldown}}}\n"
      ))
      .is_err()
    );
  }
}

#[test]
fn proxy_overrides_replace_defaults_and_hide_urls_and_credentials() {
  let text = format!(
    "environment: development\n{BASE}pow:\n  stratum_pool:\n    proxies:\n      - {{name: shared, url: 'socks5h://proxy.private:1080', username: private-user, password: private-pass}}\n    pools:\n      - {{name: inherited, address: localhost:3333}}\n      - {{name: direct, address: localhost:3334, proxies: []}}\n      - name: replacement\n        address: localhost:3335\n        proxies: [{{name: local, url: 'http://local.private:3128'}}]\n"
  );
  let document = parse(&text).unwrap();
  let cfg = document.resolve().unwrap();
  assert!(cfg.pow.stratum_pool.pools[0].proxies.is_none());
  assert!(
    cfg.pow.stratum_pool.pools[1]
      .proxies
      .as_ref()
      .unwrap()
      .is_empty()
  );
  assert_eq!(
    cfg.pow.stratum_pool.pools[2].proxies.as_ref().unwrap()[0].name,
    "local"
  );
  for output in [document.display(true).unwrap(), format!("{cfg:?}")] {
    for secret in [
      "proxy.private",
      "local.private",
      "private-user",
      "private-pass",
    ] {
      assert!(!output.contains(secret), "{output}");
    }
  }
  for replacement in [
    "https://proxy.private:1080",
    "socks5://user:pass@proxy.private:1080",
    "http://proxy.private:1080/path",
  ] {
    assert!(resolve(&text.replace("socks5h://proxy.private:1080", replacement)).is_err());
  }
  assert!(resolve(&text.replace(", password: private-pass", "")).is_err());
  assert!(
    resolve(&text.replace("socks5h://proxy.private:1080", "http://proxy.private:80")).is_ok()
  );
}

#[test]
fn invalid_proxy_variables_name_the_source_without_echoing_the_url() {
  let source = "RILGATE_POW_STRATUM_POOL_PROXIES_0_URL";
  let document = rilgate_config::loader::parse_with(
    &format!(
      "{BASE}pow:\n  stratum_pool:\n    proxies: [{{name: route, url: 'http://localhost:3128'}}]\n"
    ),
    &[rilgate_config::Setting {
      path: "pow.stratum_pool.proxies.0.url".into(),
      source: source.into(),
      value: rilgate_config::SettingValue::Scalar(
        "http://private-user:private-pass@proxy.example:3128".into(),
      ),
    }],
  )
  .unwrap();
  let error = format!("{:#}", document.resolve().unwrap_err());
  assert!(error.contains(source), "{error}");
  assert!(!error.contains("private-user"));
  assert!(!error.contains("private-pass"));
  assert!(!error.contains("proxy.example"));
}

#[test]
fn maximum_length_logins_use_the_configured_difficulty_digits() {
  let user = format!("{}{{difficulty}}", "x".repeat(252));
  let text = format!(
    "{BASE}pow:\n  algorithms:\n    randomx:\n      difficulty: {{base: 1000, max: 1000}}\n  stratum_pool:\n    pools: [{{name: pool, address: localhost:3333, login: {{user: '{user}'}}}}]\n"
  );
  assert!(resolve(&text).is_ok());
  assert!(resolve(&text.replace("max: 1000", "max: 10000")).is_err());
}

#[test]
fn tls_server_names_fail_at_startup_instead_of_on_each_challenge() {
  let text = format!(
    "{BASE}pow:\n  stratum_pool:\n    pools: [{{name: pool, address: localhost:3333, tls: {{enabled: true, server_name: 'https://pool.example'}}}}]\n"
  );
  let error = format!("{:#}", resolve(&text).unwrap_err());
  assert!(error.contains("tls.server_name"), "{error}");
}

#[test]
fn resolution_errors_attribute_only_settings_inside_the_error_path() {
  use rilgate_config::{Setting, SettingValue, loader::parse_with};
  let setting = |path: &str, source: &str, value: &str| Setting {
    path: path.into(),
    source: source.into(),
    value: SettingValue::Scalar(value.into()),
  };
  for (text, settings, expected, unrelated) in [
    (
      format!(
        "{BASE}pow:\n  stratum_pool:\n    pools: [{{name: pool, address: localhost:3333, login: {{user: invalid}}}}]\n"
      ),
      vec![
        setting(
          "pow.stratum_pool.pools.0.address",
          "ADDRESS_SOURCE",
          "localhost:3333",
        ),
        setting(
          "pow.stratum_pool.pools.0.login.user",
          "LOGIN_SOURCE",
          "invalid",
        ),
        setting(
          "pow.stratum_pool.pools.0.login.password",
          "PASSWORD_SOURCE",
          "secret",
        ),
        setting("pow.default_algorithm", "ALGORITHM_SOURCE", "fast"),
      ],
      vec!["LOGIN_SOURCE"],
      vec!["ADDRESS_SOURCE", "PASSWORD_SOURCE", "ALGORITHM_SOURCE"],
    ),
    (
      format!(
        "{BASE}pow:\n  stratum_pool:\n    pools: [{{name: pool, address: localhost:3333}}]\n    proxies: [{{name: route, url: 'http://localhost:3128'}}]\n"
      ),
      vec![
        setting("pow.stratum_pool.proxies.0.url", "PROXY_SOURCE", "invalid"),
        setting("pow.stratum_pool.pools.0.name", "POOL_SOURCE", "pool"),
      ],
      vec!["PROXY_SOURCE"],
      vec!["POOL_SOURCE"],
    ),
  ] {
    let error = format!(
      "{:#}",
      parse_with(&text, &settings).unwrap().resolve().unwrap_err()
    );
    for source in expected {
      assert!(error.contains(source), "{error}");
    }
    for source in unrelated {
      assert!(!error.contains(source), "{error}");
    }
  }
}
