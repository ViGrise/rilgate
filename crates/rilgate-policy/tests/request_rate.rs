use http::{HeaderMap, Method};
use rilgate_policy::{
  Lookups, PolicyAction, PolicyDecision, PolicyEngine,
  conditions::{CompiledCondition, Request},
  policy::schema::Definition,
};

fn compile(text: &str) -> anyhow::Result<PolicyEngine> {
  PolicyEngine::from_definition(&serde_yaml_ng::from_str::<Definition>(text).unwrap())
}

fn decide(engine: &PolicyEngine, count: Option<u32>) -> PolicyDecision {
  let headers = HeaderMap::new();
  engine.evaluate_context(
    &Request {
      host: "example.test",
      headers: &headers,
      path: "/",
      method: &Method::GET,
      client_ip: Some("192.0.2.1".parse().unwrap()),
      lookups: &Lookups {
        request_rate: count,
        ..Lookups::NONE
      },
    },
    0,
  )
}

#[test]
fn request_counts_accumulate_scores_and_select_difficulty_tiers() {
  let document: serde_yaml_ng::Value =
    serde_yaml_ng::from_str(include_str!("../../../config/examples/request-rate.yml")).unwrap();
  let definition = serde_yaml_ng::from_value(document["policy"].clone()).unwrap();
  let engine = PolicyEngine::from_definition(&definition).unwrap();
  for (count, action, difficulty, weight) in [
    (10, PolicyAction::Allow, None, 0),
    (60, PolicyAction::Challenge, Some(4), 10),
    (300, PolicyAction::Challenge, Some(6), 30),
    (2000, PolicyAction::Deny, None, 30),
  ] {
    let decision = decide(&engine, Some(count));
    assert_eq!(decision.action, action);
    assert_eq!(decision.difficulty, difficulty);
    assert_eq!(decision.weight, weight);
  }
}

#[test]
fn an_unknown_count_cannot_allow_and_keeps_positive_risk_scores() {
  let engine = compile("mode: custom\nrequest_rate: {}\ndefault_action: deny\nrules:\n  - {name: rate, when: {request_rate: {gte: 0}}, action: allow}").unwrap();
  assert_eq!(decide(&engine, None).action, PolicyAction::Deny);
  assert_eq!(decide(&engine, Some(1)).action, PolicyAction::Allow);
  let engine = compile("mode: custom\nrequest_rate: {}\nrules:\n  - {name: rate, when: {request_rate: {gte: 60}}, action: score, score: 10}").unwrap();
  assert_eq!(decide(&engine, None).weight, 10);
}

#[test]
fn request_rate_requires_settings_and_policy_rule_context() {
  assert_eq!(
    compile(
      "mode: custom\nrules: [{name: a, when: {any: [{request_rate: {gte: 60}}]}, action: deny}]"
    )
    .unwrap_err()
    .to_string(),
    "policy.rules[0].when.any[0].request_rate: requires policy.request_rate"
  );
  let error = CompiledCondition::compile(
    &serde_yaml_ng::from_str("{request_rate: {gte: 60}}").unwrap(),
    "site.open_graph.when",
    &Default::default(),
    &Default::default(),
  )
  .unwrap_err()
  .to_string();
  assert_eq!(
    error,
    "site.open_graph.when.request_rate: request_rate is only available in policy rules"
  );
}

#[test]
fn invalid_settings_and_impossible_count_ranges_are_rejected() {
  for (settings, error) in [
    ("window: 0s", "window: expected 1s to 1h"),
    ("window: 3601s", "window: expected 1s to 1h"),
    ("max_entries: 0", "max_entries: expected 1 to 1048576"),
    ("max_entries: 1048577", "max_entries: expected 1 to 1048576"),
  ] {
    assert_eq!(
      compile(&format!("mode: custom\nrequest_rate: {{{settings}}}"))
        .unwrap_err()
        .to_string(),
      format!("policy.request_rate.{error}")
    );
  }
  for mode in ["off", "light", "balanced", "strict"] {
    assert_eq!(
      compile(&format!("mode: {mode}\nrequest_rate: {{}}"))
        .unwrap_err()
        .to_string(),
      "policy: request_rate requires mode: custom"
    );
  }
  for (bounds, error) in [
    ("", "expected a numeric comparison"),
    ("gte: -1", "expected a non-negative integer"),
    ("gt: 3, lt: 4", "comparisons describe an empty range"),
    ("eq: 2, gte: 3", "comparisons describe an empty range"),
    ("lt: 0", "comparisons describe an empty range"),
  ] {
    assert_eq!(compile(&format!("mode: custom\nrequest_rate: {{}}\nrules: [{{name: rate, when: {{request_rate: {{{bounds}}}}}, action: deny}}]")).unwrap_err().to_string(), format!("policy.rules[0].when.request_rate: {error}"));
  }
}

#[test]
fn all_numeric_operators_match_counts_above_signed_integer_range() {
  let engine = compile("mode: custom\nrequest_rate: {}\nrules: [{name: rate, when: {request_rate: {gt: 2147483647}}, action: deny}]").unwrap();
  assert_eq!(decide(&engine, Some(u32::MAX)).action, PolicyAction::Deny);
  for bounds in ["eq: 5", "gt: 4, lt: 6", "gte: 5, lte: 5"] {
    let engine = compile(&format!("mode: custom\nrequest_rate: {{}}\nrules: [{{name: rate, when: {{request_rate: {{{bounds}}}}}, action: deny}}]")).unwrap();
    assert_eq!(decide(&engine, Some(5)).action, PolicyAction::Deny);
    assert_eq!(decide(&engine, Some(6)).action, PolicyAction::Allow);
  }
}
