use http::{HeaderMap, Method};
use rilgate_policy::{
  PolicyAction, PolicyEngine, conditions::Request, policy::schema::PolicyDocument,
};

fn engine(text: &str) -> anyhow::Result<PolicyEngine> {
  let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(text)?;
  let definition: PolicyDocument = serde_yaml_ng::from_value(value)?;
  PolicyEngine::from_definition(&definition)
}
fn matched(when: &str, request: &Request<'_>, score: i32) -> bool {
  let engine = engine(&format!(
    "policy:\n  rules:\n    - name: test\n      when: {when}\n      action: deny\n"
  ))
  .unwrap();
  engine.evaluate_context(request, score).action == PolicyAction::Deny
}

#[test]
fn rules_accumulate_scores_then_stop_on_terminal_actions_before_thresholds() {
  let engine = engine(
    r#"
policy:
  default_action: deny
  rules:
    - name: browser
      when: {user_agent: {contains: Mozilla}}
      action: score
      score: 10
    - name: trusted-api
      when:
        all:
          - path: {prefix: /api/}
          - any:
              - method: {exact: POST}
              - headers: {X-Allowed: {exact: 'yes'}}
          - not: {path: {contains: /private/}}
      action: allow
    - name: adjustment
      when: {path: {prefix: /}}
      action: score
      score: -3
  score:
    thresholds:
      - min: 10
        action: deny
      - min: 5
        action: challenge
        challenge: {algorithm: fast, difficulty: 2}
"#,
  )
  .unwrap();
  let mut headers = HeaderMap::new();
  headers.insert("user-agent", "Mozilla/5".parse().unwrap());
  let decision = engine.evaluate_context(
    &Request {
      trusted_peer: false,
      host: "example.test",
      headers: &headers,
      path: "/api/public",
      method: &Method::POST,
      client_ip: None,
      lookups: &rilgate_policy::Lookups::NONE,
    },
    0,
  );
  assert_eq!(decision.action, PolicyAction::Allow);
  assert_eq!(decision.weight, 10);
  assert_eq!(&*decision.rule, "trusted-api");
  let decision = engine.evaluate_context(
    &Request {
      trusted_peer: false,
      host: "example.test",
      headers: &headers,
      path: "/api/private/info",
      method: &Method::POST,
      client_ip: None,
      lookups: &rilgate_policy::Lookups::NONE,
    },
    0,
  );
  assert_eq!(decision.action, PolicyAction::Challenge);
  assert_eq!(decision.weight, 7);
  assert_eq!(decision.difficulty, Some(2));
  assert_eq!(
    engine.evaluate(&HeaderMap::new(), "/", 0).action,
    PolicyAction::Deny
  );
}

#[test]
fn string_numeric_and_ip_matchers_have_consistent_semantics() {
  let mut headers = HeaderMap::new();
  headers.insert("user-agent", "Browser Agent".parse().unwrap());
  headers.append("x-test", "first".parse().unwrap());
  headers.append("x-test", "second".parse().unwrap());
  let request = Request {
    trusted_peer: false,
    host: "example.test",
    headers: &headers,
    path: "/api/users",
    method: &Method::POST,
    client_ip: Some("192.0.2.10".parse().unwrap()),
    lookups: &rilgate_policy::Lookups::NONE,
  };
  for (when, expected) in [
    ("{path: {exact: /api/users}}", true),
    ("{path: {exact: /api}}", false),
    ("{path: {prefix: /api/}}", true),
    ("{path: {contains: users}}", true),
    ("{path: {in: [/a, /api/users]}}", true),
    ("{path: {regex: '^/api/[a-z]+$'}}", true),
    ("{method: {exact: POST}, path: {prefix: /api}}", true),
    ("{method: {in: [GET, HEAD]}}", false),
    ("{user_agent: {contains: Agent}}", true),
    ("{headers: {X-Test: {exact: second}}}", true),
    ("{headers: {X-Missing: {regex: '^$'}}}", false),
    ("{not: {headers: {X-Missing: {regex: '.*'}}}}", true),
    ("{client_ip: {cidr: 192.0.2.0/24}}", true),
    ("{client_ip: {cidr: 198.51.100.0/24}}", false),
    ("{score: {eq: 10}}", true),
    ("{score: {gt: 10}}", false),
    ("{score: {gte: 10, lt: 20}}", true),
    ("{score: {lte: 10}}", true),
  ] {
    assert_eq!(matched(when, &request, 10), expected, "{when}");
  }
  let ipv6 = Request {
    client_ip: Some("2001:db8::1".parse().unwrap()),
    lookups: &rilgate_policy::Lookups::NONE,
    ..request
  };
  assert!(matched("{client_ip: {cidr: '2001:db8::/32'}}", &ipv6, 0));
  let unknown = Request {
    trusted_peer: false,
    client_ip: None,
    lookups: &rilgate_policy::Lookups::NONE,
    ..ipv6
  };
  assert!(!matched("{client_ip: {cidr: '::/0'}}", &unknown, 0));
}

#[test]
fn scores_saturate_and_negative_adjustments_are_supported() {
  for (initial, adjustment, expected) in [
    (i32::MAX, 10, i32::MAX),
    (i32::MIN, -10, i32::MIN),
    (10, -20, -10),
  ] {
    let engine = engine(&format!("policy:\n  rules:\n    - name: adjustment\n      when: {{path: {{prefix: /}}}}\n      action: score\n      score: {adjustment}\n")).unwrap();
    let decision = engine.evaluate(&HeaderMap::new(), "/", initial);
    assert_eq!(decision.weight, expected);
    assert_eq!(decision.action, PolicyAction::Allow);
  }
}

#[test]
fn preset_profiles_are_pool_independent_and_custom_is_not_merged() {
  for (mode, arbitrary, suspicious, difficulty) in [
    ("off", PolicyAction::Allow, PolicyAction::Allow, None),
    (
      "light",
      PolicyAction::Allow,
      PolicyAction::Challenge,
      Some(1),
    ),
    (
      "balanced",
      PolicyAction::Challenge,
      PolicyAction::Challenge,
      Some(2),
    ),
    (
      "strict",
      PolicyAction::Challenge,
      PolicyAction::Challenge,
      Some(4),
    ),
  ] {
    let definition: PolicyDocument = serde_yaml_ng::from_str(&format!(
      "policy:\n{}",
      std::fs::read_to_string(format!("../../config/policies/{mode}.yml"))
        .unwrap()
        .lines()
        .map(|line| format!("  {line}\n"))
        .collect::<String>()
    ))
    .unwrap();
    assert!(
      definition
        .challenges()
        .unwrap()
        .iter()
        .all(|(_, c)| c.algorithm == Some(rilgate_protocol::PowAlgorithm::Fast))
    );
    let engine = PolicyEngine::from_definition(&definition).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("user-agent", "Mozilla".parse().unwrap());
    assert_eq!(engine.evaluate(&headers, "/protected", 0).action, arbitrary);
    headers.insert("user-agent", "curl".parse().unwrap());
    let decision = engine.evaluate(&headers, "/protected", 0);
    assert_eq!(decision.action, suspicious);
    assert_eq!(decision.difficulty, difficulty);
    assert_eq!(
      engine.evaluate(&headers, "/robots.txt", 0).action,
      PolicyAction::Allow
    );
  }
  let custom = engine("policy:\n  default_action: deny").unwrap();
  assert_eq!(
    custom.evaluate(&HeaderMap::new(), "/robots.txt", 0).action,
    PolicyAction::Deny
  );
}

#[test]
fn validation_rejects_ignored_options_ambiguous_thresholds_and_invalid_actions() {
  for text in [
    "policy: !include builtin:policies/balanced.yml\n  rules: []",
    "policy: !include builtin:policies/balanced.yml\n  rules: null",
    "policy: !include builtin:policies/strict.yml\n  score: null",
    "policy: !include builtin:policies/light.yml\n  score: {}",
    "policy: !include builtin:policies/off.yml\n  default_action: deny",
    "policy:\n  default_action: score",
    "policy:\n  score:\n    thresholds: [{min: 1, action: deny}, {min: 2, action: allow}]",
    "policy:\n  score:\n    thresholds: [{min: 1, action: deny}, {min: 1, action: allow}]",
    "policy:\n  score:\n    thresholds: [{min: 1, action: score}]",
    "policy:\n  score:\n    thresholds: [{min: 1, action: allow, challenge: {difficulty: 1}}]",
  ] {
    assert!(engine(text).is_err(), "{text}");
  }
  for options in [
    "action: score",
    "action: allow\n    score: 1",
    "action: deny\n    challenge: {difficulty: 1}",
  ] {
    assert!(engine(&format!("policy:\n  rules:\n    - name: invalid\n      when: {{path: {{prefix: /}}}}\n      {options}\n")).is_err(), "{options}");
  }
}

#[test]
fn invalid_conditions_and_excessive_evaluation_cost_are_rejected() {
  for condition in [
    "{}",
    "{all: []}",
    "{any: []}",
    "{path: {}}",
    "{path: null, method: {exact: POST}}",
    "{path: {exact: null}}",
    "{path: {prefix: /, exact: /}}",
    "{path: {in: []}}",
    "{path: {regex: '['}}",
    "{score: {}}",
    "{score: {gt: 10, lte: 10}}",
    "{score: {gt: 2147483647}}",
    "{client_ip: {cidr: invalid}}",
    "{method: {eq: 1}}",
  ] {
    assert!(
      engine(&format!(
        "policy:\n  rules:\n    - name: invalid\n      when: {condition}\n      action: allow\n"
      ))
      .is_err(),
      "{condition}"
    );
  }
  let mut condition = "{path: {prefix: /}}".to_string();
  for _ in 0..18 {
    condition = format!("{{not: {condition}}}");
  }
  assert!(
    engine(&format!(
      "policy:\n  rules:\n    - name: deep\n      when: {condition}\n      action: allow\n"
    ))
    .is_err()
  );
  let children = (0..300)
    .map(|_| "{path: {prefix: /}}")
    .collect::<Vec<_>>()
    .join(",");
  assert!(
    engine(&format!(
      "policy:\n  rules:\n    - name: costly\n      when: {{any: [{children}]}}\n      action: allow\n"
    ))
    .is_err()
  );
}

#[test]
fn cidr_condition_matches_mapped_clients_and_mapped_networks() {
  let headers = HeaderMap::new();
  let lookups = rilgate_policy::Lookups::NONE;
  let request = |ip: &str| Request {
    trusted_peer: false,
    host: "example.test",
    headers: &headers,
    path: "/",
    method: &Method::GET,
    client_ip: Some(ip.parse().unwrap()),
    lookups: &lookups,
  };
  assert!(matched(
    "{client_ip: {cidr: '::ffff:192.0.2.0/120'}}",
    &request("192.0.2.1"),
    0
  ));
  assert!(matched(
    "{client_ip: {cidr: 192.0.2.0/24}}",
    &request("::ffff:192.0.2.1"),
    0
  ));
}

#[test]
fn condition_budget_errors_report_the_rule_path() {
  let mut when = String::from("{path: {prefix: /}}");
  for _ in 0..20 {
    when = format!("{{not: {when}}}");
  }
  let error = format!(
    "{:#}",
    engine(&format!(
      "policy:\n  rules:\n    - name: deep\n      when: {when}\n      action: deny\n"
    ))
    .unwrap_err()
  );
  assert!(
    error.contains("policy.rules[0].when.not.not")
      && error.contains("condition depth must not exceed 16"),
    "{error}"
  );
}

#[test]
fn edge_headers_require_a_trusted_peer() {
  let mut headers = HeaderMap::new();
  headers.insert("x-tls-sni-name", "example.com".parse().unwrap());
  for trusted_peer in [false, true] {
    let request = Request {
      headers: &headers,
      trusted_peer,
      host: "example.com",
      path: "/",
      method: &Method::GET,
      client_ip: None,
      lookups: &rilgate_policy::Lookups::NONE,
    };
    assert_eq!(
      matched(
        "{edge_headers: {X-Tls-Sni-Name: {exact: example.com}}}",
        &request,
        0
      ),
      trusted_peer
    );
  }
}

#[test]
fn same_as_host_ignores_port_and_case() {
  for (value, expected) in [("Example.COM", true), ("other.example", false)] {
    let mut headers = HeaderMap::new();
    headers.insert("x-tls-sni-name", value.parse().unwrap());
    let request = Request {
      headers: &headers,
      trusted_peer: true,
      host: "example.com:8443",
      path: "/",
      method: &Method::GET,
      client_ip: None,
      lookups: &rilgate_policy::Lookups::NONE,
    };
    assert_eq!(
      matched(
        "{edge_headers: {X-Tls-Sni-Name: {same_as: host}}}",
        &request,
        0
      ),
      expected
    );
  }
}
