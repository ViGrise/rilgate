use rilgate_config::{Overrides, PolicyAction, loader::load};
use rilgate_policy::{PolicyEngine, conditions::Request};

#[test]
fn verified_crawler_rules_require_both_crawler_network_and_matching_user_agent() {
  for (rule, set, agent) in [
    (
      "googlebot.yml",
      "google-common-crawlers",
      "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)",
    ),
    (
      "bingbot.yml",
      "bing-crawlers",
      "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; bingbot/2.0; +http://www.bing.com/bingbot.htm) Chrome/116.0.1938.76 Safari/537.36",
    ),
    (
      "applebot.yml",
      "apple-crawlers",
      "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_5) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/13.1.1 Safari/605.1.15 (Applebot/0.1; +http://www.apple.com/go/applebot)",
    ),
    (
      "duckduckbot.yml",
      "duckduckgo-crawlers",
      "DuckDuckBot/1.1; (+http://duckduckgo.com/duckduckbot.html)",
    ),
  ] {
    let directory = tempfile::tempdir().unwrap();
    let source = format!(
      "policy:\n  mode: custom\n  default_action: challenge\n  ip_sets:\n    {set}:\n      cidrs: [192.0.2.0/24]\n  rules:\n    - !include builtin:rules/{rule}\n"
    );
    let path = directory.path().join("rilgate.yml");
    std::fs::write(&path, source).unwrap();
    let document = load(&Overrides {
      config: Some(path),
      ..Default::default()
    })
    .unwrap()
    .document;
    let engine = PolicyEngine::from_definition(&document.policy).unwrap();
    for (address, user_agent, expected) in [
      ("192.0.2.10", agent, PolicyAction::Allow),
      ("198.51.100.10", agent, PolicyAction::Challenge),
      ("192.0.2.10", "Mozilla/5.0", PolicyAction::Challenge),
    ] {
      let mut headers = http::HeaderMap::new();
      headers.insert("user-agent", user_agent.parse().unwrap());
      let request = Request {
        host: "example.test",
        headers: &headers,
        path: "/content",
        method: &http::Method::GET,
        client_ip: Some(address.parse().unwrap()),
        lookups: &rilgate_policy::Lookups::NONE,
      };
      assert_eq!(
        engine.evaluate_context(&request, 0).action,
        expected,
        "{rule}: {address}, {user_agent}"
      );
    }
  }
}

#[test]
fn embedded_policy_resolves_relative_rules_from_string_configuration() {
  let document =
    rilgate_config::loader::parse("policy: !include builtin:policies/crawler-protection.yml\n")
      .unwrap();
  let engine = PolicyEngine::from_definition(&document.policy).unwrap();
  let mut headers = http::HeaderMap::new();
  headers.insert("user-agent", "GPTBot".parse().unwrap());
  assert_eq!(
    engine.evaluate(&headers, "/robots.txt", 0).action,
    PolicyAction::Deny
  );
  headers.insert("user-agent", "Mozilla/5.0".parse().unwrap());
  assert_eq!(
    engine.evaluate(&headers, "/sitemap.xml", 0).action,
    PolicyAction::Allow
  );
  let decision = engine.evaluate(&headers, "/content", 0);
  assert_eq!(decision.action, PolicyAction::Challenge);
  assert_eq!(decision.difficulty, Some(2));
}

#[test]
fn embedded_and_local_rules_keep_order_without_catalog_shadowing() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("rilgate.yml");
  std::fs::create_dir(directory.path().join("rules")).unwrap();
  std::fs::write(
    directory.path().join("rules/deny-headless-browsers.yml"),
    "invalid: [",
  )
  .unwrap();
  std::fs::write(
    directory.path().join("local.yml"),
    "name: health\nwhen: {path: {exact: /health}}\naction: allow\n",
  )
  .unwrap();
  std::fs::write(&path, "policy:\n  mode: custom\n  rules:\n    - !include builtin:rules/deny-headless-browsers.yml\n    - !include local.yml\n    - !include builtin:rules/challenge-all.yml\n").unwrap();
  let document = load(&Overrides {
    config: Some(path),
    ..Default::default()
  })
  .unwrap()
  .document;
  let engine = PolicyEngine::from_definition(&document.policy).unwrap();
  let mut headers = http::HeaderMap::new();
  headers.insert("user-agent", "HeadlessChrome".parse().unwrap());
  assert_eq!(
    engine.evaluate(&headers, "/health", 0).action,
    PolicyAction::Deny
  );
  headers.insert("user-agent", "Mozilla/5.0".parse().unwrap());
  assert_eq!(
    engine.evaluate(&headers, "/health", 0).action,
    PolicyAction::Allow
  );
  assert_eq!(
    engine.evaluate(&headers, "/content", 0).action,
    PolicyAction::Challenge
  );
  std::fs::write(
    directory.path().join("rules/missing.yml"),
    "name: local-shadow\nwhen: {path: {prefix: /}}\naction: allow\n",
  )
  .unwrap();
  let path = directory.path().join("rilgate.yml");
  std::fs::write(
    &path,
    "policy:\n  mode: custom\n  rules: [!include builtin:rules/missing.yml]\n",
  )
  .unwrap();
  let error = format!(
    "{:#}",
    load(&Overrides {
      config: Some(path),
      ..Default::default()
    })
    .err()
    .unwrap()
  );
  assert!(
    error.contains("builtin:rules/missing.yml: resource does not exist"),
    "{error}"
  );
}

#[test]
fn embedded_lookup_errors_preserve_resource_identity_and_include_context() {
  for (include, expected) in [
    ("builtin:rules/missing.yml", "resource does not exist"),
    (
      "builtin:examples/custom.yml",
      "must select rules/ or policies/",
    ),
    ("builtin:../rules/googlebot.yml", "escapes the catalog root"),
  ] {
    let error = format!(
      "{:#}",
      rilgate_config::loader::parse(&format!("policy: !include {include}\n")).unwrap_err()
    );
    assert!(error.contains(expected), "{error}");
    assert!(error.contains("include at"), "{error}");
  }
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("rilgate.yml");
  std::fs::write(&path, "server:\n  upstream: {url: http://127.0.0.1:3000}\nsecurity:\n  secret: rule-catalog-test-secret-with-at-least-32-bytes\npolicy:\n  mode: custom\n  rules: [!include builtin:rules/googlebot.yml]\n").unwrap();
  let document = load(&Overrides {
    config: Some(path),
    ..Default::default()
  })
  .unwrap()
  .document;
  let error = format!("{:#}", document.resolve().unwrap_err());
  assert!(
    error.contains("unknown ip set google-common-crawlers"),
    "{error}"
  );
  assert!(error.contains("builtin:rules/googlebot.yml"), "{error}");
  assert!(error.contains("include chain:"), "{error}");
}

#[test]
fn embedded_sources_share_read_and_byte_budgets_with_configuration_input() {
  let source = format!(
    "policy:\n  mode: custom\n  rules:\n{}",
    "    - !include builtin:rules/challenge-all.yml\n".repeat(256)
  );
  let error = format!("{:#}", rilgate_config::loader::parse(&source).unwrap_err());
  assert!(error.contains("source reads"), "{error}");
  let source = format!(
    "#{}\npolicy:\n  mode: custom\n  rules:\n{}",
    "x".repeat(950_000),
    "    - !include builtin:rules/deny-ai-crawlers.yml\n".repeat(100)
  );
  let error = format!("{:#}", rilgate_config::loader::parse(&source).unwrap_err());
  assert!(error.contains("1 MiB"), "{error}");
}

#[test]
fn merged_builtin_policy_keeps_crawler_rules_and_overrides_the_fallback() {
  let document = rilgate_config::loader::parse(
    "policy:\n  <<: !include builtin:policies/crawler-protection.yml\n  default_action: challenge\n  ip_sets:\n    internal: {cidrs: [10.0.0.0/8]}\n",
  )
  .unwrap();
  let engine = PolicyEngine::from_definition(&document.policy).unwrap();
  engine
    .sets()
    .ip
    .get("google-common-crawlers")
    .unwrap()
    .publish(rilgate_policy::IpRanges::from_networks(vec![
      "192.0.2.0/24".parse().unwrap(),
    ]));
  assert_eq!(
    engine
      .sets()
      .ip
      .get("internal")
      .unwrap()
      .membership("10.1.2.3".parse().unwrap()),
    rilgate_policy::Membership::Member
  );
  let mut headers = http::HeaderMap::new();
  headers.insert(
    "user-agent",
    "Googlebot/2.1; +http://www.google.com/bot.html"
      .parse()
      .unwrap(),
  );
  let decision = engine.evaluate_context(
    &Request {
      host: "example.test",
      headers: &headers,
      path: "/content",
      method: &http::Method::GET,
      client_ip: Some("192.0.2.10".parse().unwrap()),
      lookups: &rilgate_policy::Lookups::NONE,
    },
    0,
  );
  assert_eq!(decision.action, PolicyAction::Allow);
  headers.clear();
  assert_eq!(
    engine.evaluate(&headers, "/content", 0).action,
    PolicyAction::Challenge
  );
  let base =
    rilgate_config::loader::parse("policy: !include builtin:policies/crawler-protection.yml\n")
      .unwrap();
  assert_eq!(
    PolicyEngine::from_definition(&base.policy)
      .unwrap()
      .evaluate(&headers, "/content", 0)
      .action,
    PolicyAction::Allow
  );
}

#[test]
fn tor_exit_rule_scores_members_of_the_tor_set() {
  let document = rilgate_config::loader::parse("policy:\n  mode: custom\n  ip_sets:\n    tor-exits: {cidrs: [192.0.2.0/24]}\n  rules:\n    - !include builtin:rules/tor-exit.yml\n").unwrap();
  let engine = PolicyEngine::from_definition(&document.policy).unwrap();
  for (ip, expected) in [("192.0.2.10", 20), ("198.51.100.10", 0)] {
    let headers = http::HeaderMap::new();
    let request = Request {
      host: "example.test",
      headers: &headers,
      path: "/",
      method: &http::Method::GET,
      client_ip: Some(ip.parse().unwrap()),
      lookups: &rilgate_policy::Lookups::NONE,
    };
    assert_eq!(engine.evaluate_context(&request, 0).weight, expected);
  }
}
