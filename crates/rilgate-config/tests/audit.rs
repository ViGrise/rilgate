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
