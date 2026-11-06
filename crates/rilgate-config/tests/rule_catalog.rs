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
      "signals:\n  ip_sets:\n    {set}:\n      cidrs: [192.0.2.0/24]\npolicy:\n  default_action: challenge\n  rules:\n    - !include builtin:rules/{rule}\n"
    );
    let path = directory.path().join("rilgate.yml");
    std::fs::write(&path, source).unwrap();
    let document = load(&Overrides {
      config: Some(path),
      ..Default::default()
    })
    .unwrap()
    .document;
    let engine = PolicyEngine::from_definition(&rilgate_policy::PolicyDocument {
      signals: document.signals.clone(),
      policy: document.policy.clone(),
    })
    .unwrap();
    for (address, user_agent, expected) in [
      ("192.0.2.10", agent, PolicyAction::Allow),
      ("198.51.100.10", agent, PolicyAction::Challenge),
      ("192.0.2.10", "Mozilla/5.0", PolicyAction::Challenge),
    ] {
      let mut headers = http::HeaderMap::new();
      headers.insert("user-agent", user_agent.parse().unwrap());
      let request = Request {
        trusted_peer: false,
        host: "example.test",
        headers: &headers,
        path: "/content",
        method: &http::Method::GET,
        client_ip: Some(address.parse().unwrap()),
        query: &rilgate_policy::conditions::Query::default(),
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
    rilgate_config::loader::parse("signals:\n  ip_sets: !include builtin:signals/crawler-ip-sets.yml\npolicy: !include builtin:policies/crawler-protection.yml\n")
      .unwrap();
  let engine = PolicyEngine::from_definition(&rilgate_policy::PolicyDocument {
    signals: document.signals.clone(),
    policy: document.policy,
  })
  .unwrap();
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
  assert_eq!(
    decision.steps.first().and_then(|step| step.difficulty),
    Some(2)
  );
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
  std::fs::write(&path, "policy:\n  rules:\n    - !include builtin:rules/deny-headless-browsers.yml\n    - !include local.yml\n    - !include builtin:rules/challenge-all.yml\n").unwrap();
  let document = load(&Overrides {
    config: Some(path),
    ..Default::default()
  })
  .unwrap()
  .document;
  let engine = PolicyEngine::from_definition(&rilgate_policy::PolicyDocument {
    signals: document.signals.clone(),
    policy: document.policy,
  })
  .unwrap();
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
    "policy:\n  rules: [!include builtin:rules/missing.yml]\n",
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
      "must select rules/, policies/, signals/ or hooks/",
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
  std::fs::write(&path, "server:\n  upstream: {url: http://127.0.0.1:3000}\nsecurity:\n  secret: rule-catalog-test-secret-with-at-least-32-bytes\npolicy:\n  rules: [!include builtin:rules/googlebot.yml]\n").unwrap();
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
    "policy:\n  rules:\n{}",
    "    - !include builtin:rules/challenge-all.yml\n".repeat(256)
  );
  let error = format!("{:#}", rilgate_config::loader::parse(&source).unwrap_err());
  assert!(error.contains("source reads"), "{error}");
  let source = format!(
    "#{}\npolicy:\n  rules:\n{}",
    "x".repeat(950_000),
    "    - !include builtin:rules/deny-ai-crawlers.yml\n".repeat(100)
  );
  let error = format!("{:#}", rilgate_config::loader::parse(&source).unwrap_err());
  assert!(error.contains("1 MiB"), "{error}");
}

#[test]
fn merged_builtin_policy_keeps_crawler_rules_and_overrides_the_fallback() {
  let document = rilgate_config::loader::parse(
    "signals:\n  ip_sets:\n    <<: !include builtin:signals/crawler-ip-sets.yml\n    internal: {cidrs: [10.0.0.0/8]}\npolicy:\n  <<: !include builtin:policies/crawler-protection.yml\n  default_action: challenge\n",
  )
  .unwrap();
  let engine = PolicyEngine::from_definition(&rilgate_policy::PolicyDocument {
    signals: document.signals.clone(),
    policy: document.policy,
  })
  .unwrap();
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
      trusted_peer: false,
      host: "example.test",
      headers: &headers,
      path: "/content",
      method: &http::Method::GET,
      client_ip: Some("192.0.2.10".parse().unwrap()),
      query: &rilgate_policy::conditions::Query::default(),
      lookups: &rilgate_policy::Lookups::NONE,
    },
    0,
  );
  assert_eq!(decision.action, PolicyAction::Allow);
  headers.insert("user-agent", "unscored-client/1.0".parse().unwrap());
  assert_eq!(
    engine.evaluate(&headers, "/content", 0).action,
    PolicyAction::Challenge
  );
  let base =
    rilgate_config::loader::parse("signals:\n  ip_sets: !include builtin:signals/crawler-ip-sets.yml\npolicy: !include builtin:policies/crawler-protection.yml\n")
      .unwrap();
  assert_eq!(
    PolicyEngine::from_definition(&rilgate_policy::PolicyDocument {
      signals: base.signals.clone(),
      policy: base.policy
    })
    .unwrap()
    .evaluate(&headers, "/content", 0)
    .action,
    PolicyAction::Allow
  );
}

#[test]
fn tor_exit_rule_scores_members_of_the_tor_set() {
  let document = rilgate_config::loader::parse("signals:\n  ip_sets:\n    tor-exits: {cidrs: [192.0.2.0/24]}\npolicy:\n  rules:\n    - !include builtin:rules/tor-exit.yml\n").unwrap();
  let engine = PolicyEngine::from_definition(&rilgate_policy::PolicyDocument {
    signals: document.signals.clone(),
    policy: document.policy,
  })
  .unwrap();
  for (ip, expected) in [("192.0.2.10", 20), ("198.51.100.10", 0)] {
    let headers = http::HeaderMap::new();
    let request = Request {
      trusted_peer: false,
      host: "example.test",
      headers: &headers,
      path: "/",
      method: &http::Method::GET,
      client_ip: Some(ip.parse().unwrap()),
      query: &rilgate_policy::conditions::Query::default(),
      lookups: &rilgate_policy::Lookups::NONE,
    };
    assert_eq!(engine.evaluate_context(&request, 0).weight, expected);
  }
}

#[test]
fn preset_files_match_the_previous_presets() {
  for (preset, names, difficulty) in [
    ("off", vec![], None),
    (
      "light",
      vec!["public-endpoints", "suspicious-client"],
      Some(1),
    ),
    (
      "balanced",
      vec!["public-endpoints", "protected-traffic"],
      Some(2),
    ),
    (
      "strict",
      vec!["public-endpoints", "protected-traffic"],
      Some(4),
    ),
  ] {
    let document =
      rilgate_config::loader::parse(&format!("policy: !include builtin:policies/{preset}.yml"))
        .unwrap();
    let (rules, default) = document.policy.resolved_rules().unwrap();
    assert_eq!(default, PolicyAction::Allow);
    assert_eq!(
      rules
        .iter()
        .map(|rule| rule.name.as_str())
        .collect::<Vec<_>>(),
      names
    );
    for (index, rule) in rules.iter().enumerate() {
      assert_eq!(
        rule.action,
        if index == 0 {
          PolicyAction::Allow
        } else {
          PolicyAction::Challenge
        }
      );
      if index == 0 {
        assert!(rule.challenge.is_none());
      } else {
        let challenge = rule.challenge.as_ref().unwrap();
        assert_eq!(
          challenge.algorithm,
          Some(rilgate_config::PowAlgorithm::Fast)
        );
        assert_eq!(challenge.difficulty, difficulty);
      }
    }
  }
}

#[test]
fn domain_fronting_and_empty_user_agent_rules() {
  let document = rilgate_config::loader::parse(
    "policy:\n  rules:\n    - !include builtin:rules/deny-domain-fronting.yml\n    - !include builtin:rules/deny-empty-user-agent.yml\n"
  ).unwrap();
  let engine = PolicyEngine::from_definition(&rilgate_policy::PolicyDocument {
    signals: document.signals,
    policy: document.policy,
  })
  .unwrap();
  for (sni, user_agent, denied) in [
    ("other.example", Some("Mozilla/5.0"), true),
    ("example.com", Some("Mozilla/5.0"), false),
    ("example.com", None, true),
    ("example.com", Some(""), true),
  ] {
    let mut headers = http::HeaderMap::new();
    headers.insert("x-tls-sni-name", sni.parse().unwrap());
    if let Some(value) = user_agent {
      headers.insert("user-agent", value.parse().unwrap());
    }
    let request = Request {
      headers: &headers,
      trusted_peer: true,
      host: "example.com",
      path: "/",
      method: &http::Method::GET,
      client_ip: None,
      query: &rilgate_policy::conditions::Query::default(),
      lookups: &rilgate_policy::Lookups::NONE,
    };
    assert_eq!(
      engine.evaluate_context(&request, 0).action == PolicyAction::Deny,
      denied
    );
  }
}

const BROWSER: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0";

fn catalog_engine(source: &str) -> PolicyEngine {
  let document = rilgate_config::loader::parse(source).unwrap();
  PolicyEngine::from_definition(&rilgate_policy::PolicyDocument {
    signals: document.signals.clone(),
    policy: document.policy,
  })
  .unwrap()
}

fn rule_engine(rule: &str) -> PolicyEngine {
  catalog_engine(&format!(
    "policy:\n  default_action: challenge\n  rules:\n    - !include builtin:rules/{rule}\n"
  ))
}

fn local_sets(names: &[&str]) -> String {
  let sets: String = names
    .iter()
    .map(|name| format!("    {name}: {{cidrs: [192.0.2.0/24]}}\n"))
    .collect();
  format!("signals:\n  ip_sets:\n{sets}")
}

const SEARCH_SETS: [&str; 4] = [
  "google-common-crawlers",
  "bing-crawlers",
  "apple-crawlers",
  "duckduckgo-crawlers",
];
const AI_SETS: [&str; 6] = [
  "openai-search-crawlers",
  "openai-training-crawlers",
  "chatgpt-user-fetchers",
  "perplexity-crawlers",
  "perplexity-user-fetchers",
  "mistral-user-fetchers",
];

struct Probe<'a> {
  agent: &'a str,
  address: &'a str,
  path: &'a str,
  headers: &'a [(&'a str, &'a str)],
  lookups: Option<&'a rilgate_policy::Lookups>,
}

impl Default for Probe<'_> {
  fn default() -> Self {
    Self {
      agent: BROWSER,
      address: "198.51.100.10",
      path: "/content",
      headers: &[],
      lookups: None,
    }
  }
}

fn decide(engine: &PolicyEngine, probe: Probe<'_>) -> rilgate_policy::PolicyDecision {
  let mut headers = http::HeaderMap::new();
  if !probe.agent.is_empty() {
    headers.insert("user-agent", probe.agent.parse().unwrap());
  }
  for (name, value) in probe.headers {
    headers.insert(
      http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
      value.parse().unwrap(),
    );
  }
  engine.evaluate_context(
    &Request {
      trusted_peer: false,
      host: "example.test",
      headers: &headers,
      path: probe.path,
      method: &http::Method::GET,
      client_ip: Some(probe.address.parse().unwrap()),
      query: &rilgate_policy::conditions::Query::default(),
      lookups: probe.lookups.unwrap_or(&rilgate_policy::Lookups::NONE),
    },
    0,
  )
}

fn confirmed(name: &str) -> rilgate_policy::Lookups {
  rilgate_policy::Lookups {
    reverse_dns: Some(Some(name.to_string())),
    ..rilgate_policy::Lookups::NONE
  }
}

#[test]
fn every_catalog_rule_compiles() {
  let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/rules");
  let names: Vec<&str> = SEARCH_SETS.iter().chain(&AI_SETS).copied().collect();
  for entry in std::fs::read_dir(directory).unwrap() {
    let file = entry.unwrap().file_name().into_string().unwrap();
    let source = format!(
      "{}    tor-exits: {{cidrs: [192.0.2.0/24]}}\n  geoip:\n    asn: {{path: /unused/asn.mmdb}}\npolicy:\n  rules:\n    - !include builtin:rules/{file}\n",
      local_sets(&names)
    );
    let document = rilgate_config::loader::parse(&source).unwrap();
    PolicyEngine::from_definition(&rilgate_policy::PolicyDocument {
      signals: document.signals.clone(),
      policy: document.policy,
    })
    .unwrap_or_else(|error| panic!("{file}: {error:#}"));
  }
}

#[test]
fn dns_verified_rules_require_the_agent_and_a_confirmed_name() {
  for (rule, agent, name) in [
    (
      "yandexbot.yml",
      "Mozilla/5.0 (compatible; YandexBot/3.0; +http://yandex.com/bots)",
      "5-255-231-1.spider.yandex.com",
    ),
    (
      "qwantbot.yml",
      "Mozilla/5.0 (compatible; Qwantbot/1.0; +https://help.qwant.com/bot/)",
      "qwantbot-1-171-187-194.qwant.com",
    ),
    (
      "mojeekbot.yml",
      "Mozilla/5.0 (compatible; MojeekBot/0.11; +https://www.mojeek.com/bot.html)",
      "crawl-5-102-173-71.mojeek.com",
    ),
    (
      "ccbot.yml",
      "CCBot/2.0 (https://commoncrawl.org/faq/)",
      "18-97-14-80.crawl.commoncrawl.org",
    ),
    (
      "telegram-preview.yml",
      "TelegramBot (like TwitterBot)",
      "ip-149-154-161-200.ptr.telegram.org",
    ),
    (
      "vk-preview.yml",
      "Mozilla/5.0 (compatible; vkShare; +http://vk.com/dev/Share)",
      "snipster1.go.mail.ru",
    ),
  ] {
    let engine = rule_engine(rule);
    let verified = confirmed(name);
    let forged = confirmed(&format!("{name}.attacker.example"));
    for (lookups, expected) in [
      (&verified, PolicyAction::Allow),
      (&forged, PolicyAction::Challenge),
    ] {
      let decision = decide(
        &engine,
        Probe {
          agent,
          lookups: Some(lookups),
          ..Default::default()
        },
      );
      assert_eq!(decision.action, expected, "{rule}");
    }
    let unresolved = decide(
      &engine,
      Probe {
        agent,
        ..Default::default()
      },
    );
    assert_ne!(unresolved.action, PolicyAction::Allow, "{rule}");
    assert!(unresolved.needs_lookups, "{rule}");
    let browser = decide(&engine, Probe::default());
    assert_eq!(browser.action, PolicyAction::Challenge, "{rule}");
    assert!(!browser.needs_lookups, "{rule}: browsers skip DNS lookups");
  }
}

#[test]
fn network_verified_rules_match_only_their_published_networks() {
  for (rule, agent, inside) in [
    (
      "kagibot.yml",
      "Mozilla/5.0 (compatible; Kagibot/1.0; +https://kagi.com/bot)",
      "216.18.205.234",
    ),
    (
      "marginalia-crawler.yml",
      "search.marginalia.nu",
      "193.183.0.170",
    ),
    (
      "wikimedia-citation-fetchers.yml",
      "Citoid/WMF (mailto:noc@wikimedia.org)",
      "2620:0:861::1",
    ),
    ("internet-archive.yml", BROWSER, "207.241.229.10"),
    ("arquivo-pt.yml", BROWSER, "194.210.235.10"),
  ] {
    let engine = rule_engine(rule);
    for (address, expected) in [
      (inside, PolicyAction::Allow),
      ("198.51.100.10", PolicyAction::Challenge),
    ] {
      let decision = decide(
        &engine,
        Probe {
          agent,
          address,
          ..Default::default()
        },
      );
      assert_eq!(decision.action, expected, "{rule}: {address}");
    }
  }
}

#[test]
fn tool_client_rules_stay_on_their_protocol_paths() {
  for (rule, agent, protocol_path) in [
    (
      "allow-git-clients.yml",
      "git/2.47.0",
      "/group/repo.git/info/refs",
    ),
    (
      "allow-git-clients.yml",
      "JGit/6.10.0",
      "/group/repo.git/git-upload-pack",
    ),
    (
      "allow-git-clients.yml",
      "git-lfs/3.5.1 (GitHub; linux amd64; go 1.22)",
      "/group/repo.git/info/lfs/objects/batch",
    ),
    (
      "allow-container-clients.yml",
      "docker/27.1.0 go/go1.22 git-commit/abc kernel/6.8 os/linux arch/amd64",
      "/v2/app/manifests/latest",
    ),
    (
      "allow-container-clients.yml",
      "containerd/1.7.20",
      "/v2/app/blobs/sha256:abc",
    ),
  ] {
    let engine = rule_engine(rule);
    for (path, expected) in [
      (protocol_path, PolicyAction::Allow),
      ("/admin/users", PolicyAction::Challenge),
    ] {
      let decision = decide(
        &engine,
        Probe {
          agent,
          path,
          ..Default::default()
        },
      );
      assert_eq!(decision.action, expected, "{rule}: {agent} {path}");
    }
    let browser = decide(
      &engine,
      Probe {
        path: protocol_path,
        ..Default::default()
      },
    );
    assert_eq!(browser.action, PolicyAction::Challenge, "{rule}");
  }
}

#[test]
fn go_get_rule_requires_the_client_and_the_parameter() {
  let engine = rule_engine("allow-go-get.yml");
  for (agent, raw, method, expected) in [
    (
      "Go-http-client/1.1",
      "go-get=1",
      http::Method::GET,
      PolicyAction::Allow,
    ),
    (
      "Go-http-client/1.1",
      "",
      http::Method::GET,
      PolicyAction::Challenge,
    ),
    (
      "Go-http-client/1.1",
      "go-get=1",
      http::Method::POST,
      PolicyAction::Challenge,
    ),
    (
      BROWSER,
      "go-get=1",
      http::Method::GET,
      PolicyAction::Challenge,
    ),
  ] {
    let mut headers = http::HeaderMap::new();
    headers.insert("user-agent", agent.parse().unwrap());
    let decision = engine.evaluate_context(
      &Request {
        trusted_peer: false,
        host: "example.test",
        headers: &headers,
        path: "/group/module",
        method: &method,
        client_ip: None,
        query: &rilgate_policy::conditions::Query::new(raw),
        lookups: &rilgate_policy::Lookups::NONE,
      },
      0,
    );
    assert_eq!(decision.action, expected, "{agent}: {raw} {method}");
  }
}

#[test]
fn crawler_protection_denies_unwanted_clients_and_tiers_suspicious_ones() {
  let engine = catalog_engine(&format!(
    "{}policy: !include builtin:policies/crawler-protection.yml\n",
    local_sets(&SEARCH_SETS)
  ));
  for agent in [
    "",
    "AIWebIndex/2.0 (+https://lyrenth.com/bot)",
    "code-review-sourcing/1.0 (+xai-research)",
    "scraper (+https://github.com/US-Artificial-Intelligence/scraper)",
    "Mozilla/5.0 (compatible; TavilyBot/1.0)",
  ] {
    let decision = decide(
      &engine,
      Probe {
        agent,
        ..Default::default()
      },
    );
    assert_eq!(decision.action, PolicyAction::Deny, "{agent:?}");
  }
  let discontinued = decide(
    &engine,
    Probe {
      agent: "Mozilla/4.0 (compatible; MSIE 6.0; Windows 98)",
      ..Default::default()
    },
  );
  assert_eq!(discontinued.action, PolicyAction::Challenge);
  assert_eq!(discontinued.steps[0].difficulty, Some(6));

  let browser = decide(&engine, Probe::default());
  assert_eq!(browser.steps[0].difficulty, Some(2));
  assert!(!browser.needs_lookups);

  let low_score = decide(
    &engine,
    Probe {
      agent: "preview-client/1.0",
      headers: &[("x-firefox-ai", "1")],
      ..Default::default()
    },
  );
  assert_eq!(low_score.action, PolicyAction::Challenge);
  assert_eq!(
    format!("{:?}", low_score.steps[0].algorithm),
    "Some(Refresh)"
  );

  let yandex = confirmed("5-255-231-1.spider.yandex.com");
  let crawler = decide(
    &engine,
    Probe {
      agent: "Mozilla/5.0 (compatible; YandexBot/3.0; +http://yandex.com/bots)",
      lookups: Some(&yandex),
      ..Default::default()
    },
  );
  assert_eq!(crawler.action, PolicyAction::Allow);
}

#[test]
fn ai_search_policy_allows_only_verified_search_agents() {
  let names: Vec<&str> = SEARCH_SETS.iter().chain(&AI_SETS).copied().collect();
  let engine = catalog_engine(&format!(
    "{}policy: !include builtin:policies/crawler-protection-ai-search.yml\n",
    local_sets(&names)
  ));
  for (agent, address, expected) in [
    (
      "Mozilla/5.0 (compatible; OAI-SearchBot/1.0; +https://openai.com/searchbot)",
      "192.0.2.10",
      PolicyAction::Allow,
    ),
    (
      "Mozilla/5.0 (compatible; OAI-SearchBot/1.0; +https://openai.com/searchbot)",
      "198.51.100.10",
      PolicyAction::Deny,
    ),
    (
      "Mozilla/5.0 (compatible; PerplexityBot/1.0; +https://perplexity.ai/perplexitybot)",
      "192.0.2.10",
      PolicyAction::Allow,
    ),
    (
      "Mozilla/5.0 (compatible; GPTBot/1.1; +https://openai.com/gptbot)",
      "192.0.2.10",
      PolicyAction::Deny,
    ),
  ] {
    let decision = decide(
      &engine,
      Probe {
        agent,
        address,
        ..Default::default()
      },
    );
    assert_eq!(decision.action, expected, "{agent} from {address}");
  }
  let thresholds = decide(&engine, Probe::default());
  assert_eq!(thresholds.steps[0].difficulty, Some(2));
}

#[test]
fn browser_signals_lower_the_score_only_for_complete_headers() {
  let engine = catalog_engine(
    "policy:\n  default_action: allow\n  rules:\n    - !include builtin:rules/browser-signals-score.yml\n    - !include builtin:rules/client-score.yml\n  score:\n    thresholds:\n      - {min: 1, action: deny}\n",
  );
  let complete = [
    ("accept", "text/html"),
    ("accept-language", "en"),
    ("accept-encoding", "gzip, br"),
    ("sec-fetch-dest", "document"),
    ("sec-fetch-mode", "navigate"),
    ("sec-fetch-site", "none"),
  ];
  let browser = decide(
    &engine,
    Probe {
      headers: &complete,
      ..Default::default()
    },
  );
  assert_eq!(browser.action, PolicyAction::Allow);
  let partial = decide(
    &engine,
    Probe {
      headers: &complete[..5],
      ..Default::default()
    },
  );
  assert_eq!(partial.action, PolicyAction::Deny);
}
