use http::{HeaderMap, Method};
use rilgate_policy::{
  Lookups, PolicyAction, PolicyEngine, conditions::Request, policy::schema::PolicyDocument,
};

fn compile(text: &str) -> anyhow::Result<PolicyEngine> {
  let definition: PolicyDocument = serde_yaml_ng::from_str(text).unwrap();
  PolicyEngine::from_definition(&definition)
}

fn decide(engine: &PolicyEngine, honeypot: Option<bool>) -> PolicyAction {
  let headers = HeaderMap::new();
  let lookups = Lookups {
    honeypot,
    ..Lookups::NONE
  };
  engine
    .evaluate_context(
      &Request {
        host: "example.test",
        headers: &headers,
        path: "/",
        method: &Method::GET,
        client_ip: Some("192.0.2.1".parse().unwrap()),
        lookups: &lookups,
      },
      0,
    )
    .action
}

const RULES: &str = "signals:\n  honeypot: {}\npolicy:\n  rules:\n    - {name: trapped, when: {honeypot: true}, action: deny}\n";

#[test]
fn flagged_clients_match_the_honeypot_condition() {
  let engine = compile(RULES).unwrap();
  assert_eq!(decide(&engine, Some(true)), PolicyAction::Deny);
  assert_eq!(decide(&engine, Some(false)), PolicyAction::Allow);
  assert_eq!(decide(&engine, None), PolicyAction::Allow);
}

#[test]
fn honeypot_condition_requires_the_honeypot() {
  let error = compile("policy:\n  rules: [{name: a, when: {honeypot: true}, action: deny}]")
    .unwrap_err()
    .to_string();
  assert_eq!(
    error,
    "policy.rules[0].when.honeypot: requires signals.honeypot"
  );
}

#[test]
fn honeypot_condition_is_rejected_outside_policy_rules() {
  let error = rilgate_policy::conditions::CompiledCondition::compile(
    &serde_yaml_ng::from_str("{honeypot: true}").unwrap(),
    "site.open_graph.when",
    &rilgate_policy::PolicySets::default(),
    &rilgate_policy::GeoDatabases::default(),
  )
  .unwrap_err()
  .to_string();
  assert_eq!(
    error,
    "site.open_graph.when.honeypot: honeypot is only available in policy rules"
  );
}

#[test]
fn honeypot_settings_are_validated() {
  assert_eq!(
    compile("signals:\n  honeypot: {ttl: 31d}\npolicy: {}\n")
      .unwrap_err()
      .to_string(),
    "signals.honeypot.ttl: expected 1s to 30d"
  );
  assert_eq!(
    compile("signals:\n  honeypot: {max_entries: 0}\npolicy: {}\n")
      .unwrap_err()
      .to_string(),
    "signals.honeypot.max_entries: expected 1 to 1048576"
  );
  let definition: PolicyDocument = serde_yaml_ng::from_str("signals: {honeypot: {}}").unwrap();
  assert!(PolicyEngine::from_definition(&definition).is_ok());
}
