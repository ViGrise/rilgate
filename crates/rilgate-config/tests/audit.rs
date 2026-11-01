use rilgate_config::loader::parse;

const BASE: &str = "server:\n  upstream:\n    url: http://127.0.0.1:3000\nsecurity:\n  secret: '0123456789abcdef0123456789abcdef'\n";
const DEVELOPMENT: &str = "environment: development\n";

fn resolve(text: &str) -> anyhow::Result<rilgate_config::Config> {
  parse(text)?.resolve()
}

fn warnings(text: &str) -> Vec<&'static str> {
  resolve(text)
    .unwrap()
    .warnings()
    .into_iter()
    .map(|finding| finding.id)
    .collect()
}

fn server(extra: &str) -> String {
  BASE.replace("server:\n", &format!("server:\n{extra}"))
}

#[test]
fn defaults_produce_no_findings_in_production() {
  assert!(warnings(BASE).is_empty());
}

#[test]
fn broad_trusted_proxy_ranges_fail_in_production() {
  for range in ["0.0.0.0/0", "0.0.0.0/7", "::/0", "2000::/15"] {
    let text = server(&format!("  trusted_proxies: ['{range}']\n"));
    let error = format!("{:#}", resolve(&text).unwrap_err());
    assert!(error.contains("server.trusted_proxies"), "{range}: {error}");
    assert!(error.contains("trusted-proxy-broad"), "{range}: {error}");
    let development = warnings(&format!("{DEVELOPMENT}{text}"));
    assert!(
      development
        .iter()
        .all(|id| !id.starts_with("trusted-proxy")),
      "{range}: {development:?}"
    );
  }
}

#[test]
fn public_trusted_proxy_ranges_warn_and_private_ranges_pass() {
  for range in ["173.245.48.0/20", "2400:cb00::/32"] {
    let text = server(&format!("  trusted_proxies: ['{range}']\n"));
    assert_eq!(warnings(&text), ["trusted-proxy-public"], "{range}");
  }
  for range in [
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.1.0/24",
    "127.0.0.1/32",
    "100.64.0.0/10",
    "::1/128",
    "fd00::/8",
  ] {
    let text = server(&format!("  trusted_proxies: ['{range}']\n"));
    assert!(warnings(&text).is_empty(), "{range}");
  }
}

#[test]
fn development_warns_when_listening_beyond_loopback() {
  assert_eq!(
    warnings(&format!("{DEVELOPMENT}{BASE}")),
    ["development-public-listener"]
  );
  for listen in ["127.0.0.1:8080", "'[::1]:8080'"] {
    let text = format!("{DEVELOPMENT}{}", server(&format!("  listen: {listen}\n")));
    assert!(warnings(&text).is_empty(), "{listen}");
  }
}

#[test]
fn public_operations_listeners_warn_in_production() {
  let text = server("  operations:\n    listen: 0.0.0.0:9090\n");
  assert_eq!(warnings(&text), ["operations-listen-public"]);
  for listen in ["127.0.0.1:9090", "10.1.2.3:9090"] {
    let text = server(&format!("  operations:\n    listen: {listen}\n"));
    assert!(warnings(&text).is_empty(), "{listen}");
  }
}

#[test]
fn enabled_metrics_without_operations_listener_warn() {
  let text = server("  metrics_enabled: true\n");
  assert_eq!(warnings(&text), ["metrics-unreachable"]);
  let reachable = server("  metrics_enabled: true\n  operations:\n    listen: 127.0.0.1:9090\n");
  assert!(warnings(&reachable).is_empty());
}

#[test]
fn production_warns_about_disabled_quotas_watching_and_long_access() {
  let auth_ttl = |ttl: &str| {
    BASE.replace(
      "security:\n",
      &format!("security:\n  auth:\n    ttl: {ttl}\n"),
    )
  };
  for (text, id) in [
    (
      format!("{BASE}pow:\n  rate_limit:\n    enabled: false\n"),
      "proof-rate-limit-disabled",
    ),
    (
      format!(
        "{BASE}site:\n  resources:\n    directory: /var/lib/rilgate/web\n    watch:\n      enabled: true\n"
      ),
      "resource-watch-enabled",
    ),
    (auth_ttl("8d"), "auth-ttl-long"),
  ] {
    assert_eq!(warnings(&text), [id], "{id}");
    assert!(
      !warnings(&format!("{DEVELOPMENT}{text}")).contains(&id),
      "{id}"
    );
  }
  assert!(warnings(&auth_ttl("7d")).is_empty());
}

#[test]
fn findings_display_their_identifier() {
  let finding = resolve(&server("  metrics_enabled: true\n"))
    .unwrap()
    .warnings()
    .remove(0);
  let text = finding.to_string();
  assert!(text.starts_with("server.metrics_enabled:"), "{text}");
  assert!(text.ends_with("[metrics-unreachable]"), "{text}");
}

#[test]
fn remote_verifier_environment_errors_have_stable_identifiers() {
  let configured = "pow:\n  algorithms:\n    randomx:\n      verify:\n        order: [remote]\n        remote:\n          endpoint: https://verifier.test\n          tls: {ca: ca.pem, client_cert: cert.pem, client_key: key.pem}\n";
  assert!(resolve(&format!("{BASE}{configured}")).is_ok());
  for (text, id) in [
    (
      configured.replace("https://", "http://"),
      "verifier-remote-plain-http",
    ),
    (
      configured.replace("ca: ca.pem, ", ""),
      "verifier-remote-credentials",
    ),
    (
      configured.replace(", client_cert: cert.pem, client_key: key.pem", ""),
      "verifier-remote-credentials",
    ),
    (
      configured.replace("          endpoint: https://verifier.test\n", ""),
      "verifier-remote-missing",
    ),
  ] {
    let error = format!("{:#}", resolve(&format!("{BASE}{text}")).unwrap_err());
    assert!(error.contains(id), "{error}");
    if id == "verifier-remote-missing" {
      assert!(
        format!(
          "{:#}",
          resolve(&format!("{DEVELOPMENT}{BASE}{text}")).unwrap_err()
        )
        .contains(id)
      );
    } else {
      assert!(resolve(&format!("{DEVELOPMENT}{BASE}{text}")).is_ok());
    }
  }
}

#[test]
fn remote_verifier_warnings_follow_order_rules_and_enforcement() {
  let configured = "pow:\n  stratum_pool: {}\n  algorithms:\n    randomx:\n      verify:\n        order: [remote, local]\n        remote:\n          endpoint: https://verifier.test\n          tls: {ca: ca.pem, client_cert: cert.pem, client_key: key.pem}\n";
  for environment in ["", DEVELOPMENT] {
    let text = format!("{environment}{BASE}{configured}");
    let findings = warnings(&text);
    assert!(findings.contains(&"verifier-local-fallback"));
    assert!(findings.contains(&"verifier-remote-unused"));
    for order in ["local, remote", "remote", "local"] {
      let findings = warnings(&text.replace("remote, local", order));
      assert!(!findings.contains(&"verifier-local-fallback"));
      assert_eq!(
        findings.contains(&"verifier-remote-unused"),
        order.contains("remote")
      );
    }
    let randomx = format!(
      "{}\npolicy:\n  rules:\n    - name: all\n      when: {{path: {{prefix: /}}}}\n      action: challenge\n",
      text.replace(
        "  stratum_pool:",
        "  default_algorithm: randomx\n  stratum_pool:"
      )
    );
    assert!(!warnings(&randomx).contains(&"verifier-remote-unused"));
    let pool_accepted = randomx.replace(
      "  stratum_pool:",
      "  verification: stratum_pool_accepted\n  stratum_pool:",
    );
    assert!(warnings(&pool_accepted).contains(&"verifier-remote-unused"));
  }
}

#[test]
fn public_plain_pool_transport_fails_production_and_warns_development() {
  for host in ["203.0.113.1", "pool.example", "[2001:db8::1]"] {
    let text = format!(
      "{BASE}pow:\n  stratum_pool:\n    pools: [{{name: public, address: '{host}:3333'}}]\n"
    );
    let error = format!("{:#}", resolve(&text).unwrap_err());
    assert!(error.contains("stratum-pool-plain-tcp"), "{error}");
    assert!(warnings(&format!("{DEVELOPMENT}{text}")).contains(&"stratum-pool-plain-tcp"));
    assert!(
      !warnings(&text.replace("address:", "tls: {enabled: true}, address:"))
        .contains(&"stratum-pool-plain-tcp")
    );
  }
  for host in [
    "localhost",
    "127.0.0.1",
    "10.1.2.3",
    "192.168.1.2",
    "[::1]",
    "[::ffff:127.0.0.1]",
    "[fd00::1]",
  ] {
    let text = format!(
      "{BASE}pow:\n  stratum_pool:\n    pools: [{{name: private, address: '{host}:3333'}}]\n"
    );
    assert!(
      !warnings(&text).contains(&"stratum-pool-plain-tcp"),
      "{host}"
    );
  }
}

#[test]
fn public_proxy_credentials_fail_even_with_pool_tls_and_private_credentials_pass() {
  for scope in [
    "    proxies:",
    "    pools:\n      - name: primary\n        address: localhost:3333\n        proxies:",
  ] {
    let indentation = if scope.ends_with("        proxies:") {
      "          "
    } else {
      "      "
    };
    let text = format!(
      "{BASE}pow:\n  stratum_pool:\n{scope}\n{indentation}- {{name: route, url: 'socks5h://proxy.example:1080', username: private-user, password: private-pass}}\n"
    );
    let error = format!("{:#}", resolve(&text).unwrap_err());
    assert!(error.contains("stratum-proxy-plain-credentials"), "{error}");
    for secret in ["private-user", "private-pass", "proxy.example"] {
      assert!(!error.contains(secret));
    }
    assert!(warnings(&format!("{DEVELOPMENT}{text}")).contains(&"stratum-proxy-plain-credentials"));
    assert!(
      !warnings(&text.replace("proxy.example", "127.0.0.1"))
        .contains(&"stratum-proxy-plain-credentials")
    );
    assert!(
      !warnings(&text.replace(", username: private-user, password: private-pass", ""))
        .contains(&"stratum-proxy-plain-credentials")
    );
  }
}

#[test]
fn public_per_challenge_connections_warn_in_both_environments_even_with_tls() {
  let text = format!(
    "{BASE}pow:\n  stratum_pool:\n    pools: [{{name: public, address: 'pool.example:443', tls: {{enabled: true}}}}]\n    job_sessions: {{mode: per_challenge}}\n"
  );
  for prefix in ["", DEVELOPMENT] {
    assert!(warnings(&format!("{prefix}{text}")).contains(&"stratum-pool-per-challenge-public"));
    assert!(
      !warnings(&format!(
        "{prefix}{}",
        text.replace("per_challenge", "pooled")
      ))
      .contains(&"stratum-pool-per-challenge-public")
    );
    assert!(
      !warnings(&format!(
        "{prefix}{}",
        text.replace("pool.example", "localhost")
      ))
      .contains(&"stratum-pool-per-challenge-public")
    );
  }
}

#[test]
fn private_dns_names_allow_pool_and_proxy_transport_in_production() {
  for host in [
    "p2pool",
    "pool.local",
    "pool.internal",
    "pool.lan",
    "pool.home.arpa",
    "pool.svc",
    "pool.cluster.local",
    "POOL.INTERNAL.",
  ] {
    let text = format!(
      "{BASE}pow:\n  stratum_pool:\n    pools: [{{name: private, address: '{host}:3333'}}]\n    proxies: [{{name: route, url: 'http://{host}:3128', username: user, password: pass}}]\n"
    );
    assert!(warnings(&text).is_empty(), "{host}");
  }
  for host in [
    "pool.internal.example",
    "pool.notinternal",
    "pool.svc.example",
  ] {
    let text = format!(
      "{BASE}pow:\n  stratum_pool:\n    pools: [{{name: public, address: '{host}:3333'}}]\n"
    );
    assert!(resolve(&text).is_err(), "{host}");
  }
}
