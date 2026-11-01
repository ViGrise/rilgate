use http::{HeaderMap, Method};
use rilgate_policy::{
  Lookups, PolicyAction, PolicyEngine, conditions::Request, policy::schema::PolicyDocument,
};

fn engine(rules: &str) -> PolicyEngine {
  PolicyEngine::from_definition(&serde_yaml_ng::from_str::<PolicyDocument>(rules).unwrap()).unwrap()
}

fn decide(engine: &PolicyEngine, lookups: &Lookups) -> rilgate_policy::PolicyDecision {
  let headers = HeaderMap::new();
  engine.evaluate_context(
    &Request {
      host: "example.test",
      headers: &headers,
      path: "/",
      method: &Method::GET,
      client_ip: Some("66.249.66.1".parse().unwrap()),
      lookups,
    },
    0,
  )
}

const CRAWLER_RULES: &str = "policy:\n  default_action: deny\n  rules:\n    - name: crawler\n      when: {client_ip: {reverse_dns: [googlebot.com]}}\n      action: allow\n";

#[test]
fn unresolved_reverse_dns_never_allows() {
  let engine = engine(CRAWLER_RULES);
  let decision = decide(&engine, &Lookups::NONE);
  assert_ne!(decision.action, PolicyAction::Allow);
  assert!(decision.needs_lookups);
  assert!(decision.unresolved);
}

#[test]
fn verified_reverse_dns_matches_on_a_label_boundary() {
  let engine = engine(CRAWLER_RULES);
  let verified = |name: &str| Lookups {
    reverse_dns: Some(Some(name.to_string())),
    ..Lookups::NONE
  };
  for name in [
    "crawl-66-249-66-1.googlebot.com",
    "Crawl.GoogleBot.COM.",
    "googlebot.com",
  ] {
    let decision = decide(&engine, &verified(name));
    assert_eq!(decision.action, PolicyAction::Allow, "{name}");
    assert!(!decision.needs_lookups);
  }
  for name in ["crawl.evilgooglebot.com", "googlebot.com.evil.test"] {
    assert_eq!(
      decide(&engine, &verified(name)).action,
      PolicyAction::Deny,
      "{name}"
    );
  }
  let unverified = Lookups {
    reverse_dns: Some(None),
    ..Lookups::NONE
  };
  assert_eq!(decide(&engine, &unverified).action, PolicyAction::Deny);
}

#[test]
fn dnsbl_listing_scores() {
  let engine = engine(
    "policy:\n  rules:\n    - {name: listed, when: {dnsbl: dnsbl.example}, action: score, score: 10}\n",
  );
  let answer = |listed| Lookups {
    dnsbl: [("dnsbl.example".to_string(), listed)].into(),
    ..Lookups::NONE
  };
  assert_eq!(decide(&engine, &answer(true)).weight, 10);
  assert_eq!(decide(&engine, &answer(false)).weight, 0);
  assert!(!decide(&engine, &answer(false)).needs_lookups);
  assert!(decide(&engine, &Lookups::NONE).needs_lookups);
}

#[test]
fn lookup_plan_lists_the_zones_and_reverse_dns_use() {
  let engine = engine(
    "policy:\n  rules:\n    - {name: a, when: {dnsbl: DNSBL.example.}, action: score, score: 1}\n    - {name: b, when: {any: [{dnsbl: other.example}, {client_ip: {reverse_dns: [example.org]}}]}, action: score, score: 1}\n",
  );
  let plan = engine.lookup_plan();
  assert_eq!(plan.dnsbl_zones, ["dnsbl.example", "other.example"]);
  assert!(plan.reverse_dns);
}

#[test]
fn lookup_conditions_are_rejected_outside_policy_rules() {
  let condition = |text: &str| {
    rilgate_policy::conditions::CompiledCondition::compile(
      &serde_yaml_ng::from_str(text).unwrap(),
      "site.open_graph.when",
      &rilgate_policy::PolicySets::default(),
      &rilgate_policy::GeoDatabases::default(),
    )
  };
  assert_eq!(
    condition("{dnsbl: dnsbl.example}").unwrap_err().to_string(),
    "site.open_graph.when.dnsbl: dnsbl is only available in policy rules"
  );
  assert_eq!(
    condition("{client_ip: {reverse_dns: [example.org]}}")
      .unwrap_err()
      .to_string(),
    "site.open_graph.when.client_ip.reverse_dns: reverse_dns is only available in policy rules"
  );
}

#[test]
fn invalid_lookup_settings_are_rejected() {
  let error = |text: &str| {
    let definition: PolicyDocument = serde_yaml_ng::from_str(text).unwrap();
    PolicyEngine::from_definition(&definition)
      .unwrap_err()
      .to_string()
  };
  assert_eq!(
    error("policy:\n  rules: [{name: a, when: {dnsbl: 'bad zone'}, action: deny}]"),
    "policy.rules[0].when.dnsbl: expected a DNS zone name"
  );
  assert_eq!(
    error("policy:\n  rules: [{name: a, when: {client_ip: {reverse_dns: []}}, action: deny}]"),
    "policy.rules[0].when.client_ip.reverse_dns: expected 1 to 16 domain names"
  );
  assert_eq!(
    error("signals:\n  dns: {timeout: 30s}\npolicy: {}\n"),
    "signals.dns.timeout: expected 1ms to 5s"
  );
}
