use http::{HeaderMap, Method};
use rilgate_policy::{
  Lookups, PolicyAction, PolicyEngine, conditions::Request, policy::schema::Definition,
};

fn compile(text: &str) -> anyhow::Result<PolicyEngine> {
  let definition: Definition = serde_yaml_ng::from_str(&format!("mode: custom\n{text}")).unwrap();
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

const RULES: &str = "honeypot: {}
rules:
  - {name: trapped, when: {honeypot: true}, action: deny}
";

#[test]
fn flagged_clients_match_the_honeypot_condition() {
  let engine = compile(RULES).unwrap();
  assert_eq!(decide(&engine, Some(true)), PolicyAction::Deny);
  assert_eq!(decide(&engine, Some(false)), PolicyAction::Allow);
  assert_eq!(decide(&engine, None), PolicyAction::Allow);
}

#[test]
fn honeypot_condition_requires_the_honeypot() {
  let error = compile("rules: [{name: a, when: {honeypot: true}, action: deny}]")
    .unwrap_err()
    .to_string();
  assert_eq!(
    error,
    "policy.rules[0].when.honeypot: requires policy.honeypot"
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
    compile("honeypot: {ttl: 31d}").unwrap_err().to_string(),
    "policy.honeypot.ttl: expected 1s to 30d"
  );
  assert_eq!(
    compile("honeypot: {max_entries: 0}")
      .unwrap_err()
      .to_string(),
    "policy.honeypot.max_entries: expected 1 to 1048576"
  );
  let definition: Definition = serde_yaml_ng::from_str("honeypot: {}").unwrap();
  assert_eq!(
    PolicyEngine::from_definition(&definition)
      .unwrap_err()
      .to_string(),
    "policy: honeypot requires mode: custom"
  );
}
