use http::{HeaderMap, Method};
use rilgate_policy::{PolicyAction, PolicyEngine, conditions::Request, policy::schema::Definition};
use std::net::IpAddr;

const COUNTRY_DB: &str = concat!(
  env!("CARGO_MANIFEST_DIR"),
  "/tests/data/GeoLite2-Country-Test.mmdb"
);
const ASN_DB: &str = concat!(
  env!("CARGO_MANIFEST_DIR"),
  "/tests/data/GeoLite2-ASN-Test.mmdb"
);
// Fixture addresses from the MaxMind test database sources.
const GB_ADDRESS: &str = "2.125.160.217";
const BT_ADDRESS: &str = "67.43.156.1";
const GOOGLE_ASN_ADDRESS: &str = "1.0.0.1";
const TELSTRA_ASN_ADDRESS: &str = "1.128.0.1";

fn engine(rules: &str) -> anyhow::Result<PolicyEngine> {
  let text = format!(
    "mode: custom\ngeoip:\n  country: {{path: '{COUNTRY_DB}'}}\n  asn: {{path: '{ASN_DB}'}}\n{rules}"
  );
  PolicyEngine::from_definition(&serde_yaml_ng::from_str::<Definition>(&text)?)
}

fn load(engine: &PolicyEngine) {
  let geoip = engine.geoip();
  geoip
    .country()
    .unwrap()
    .publish(std::fs::read(COUNTRY_DB).unwrap())
    .unwrap();
  geoip
    .asn()
    .unwrap()
    .publish(std::fs::read(ASN_DB).unwrap())
    .unwrap();
}

fn weight(engine: &PolicyEngine, ip: &str) -> i32 {
  decide(engine, ip).1
}

fn decide(engine: &PolicyEngine, ip: &str) -> (PolicyAction, i32, bool) {
  let headers = HeaderMap::new();
  let decision = engine.evaluate_context(
    &Request {
      host: "example.test",
      headers: &headers,
      path: "/",
      method: &Method::GET,
      client_ip: Some(ip.parse::<IpAddr>().unwrap()),
      lookups: &rilgate_policy::Lookups::NONE,
    },
    0,
  );
  (decision.action, decision.weight, decision.unresolved)
}

const SCORE_RULES: &str = "rules:
  - {name: gb, when: {country: [GB, US]}, action: score, score: 3}
  - {name: google, when: {asn: [15169]}, action: score, score: 5}
";

#[test]
fn country_condition_matches_listed_codes() {
  let engine = engine(SCORE_RULES).unwrap();
  load(&engine);
  assert_eq!(weight(&engine, GB_ADDRESS), 3);
  assert_eq!(weight(&engine, BT_ADDRESS), 0);
  assert_eq!(weight(&engine, "203.0.113.9"), 0);
}

#[test]
fn asn_condition_matches_listed_numbers() {
  let engine = engine(SCORE_RULES).unwrap();
  load(&engine);
  assert_eq!(weight(&engine, GOOGLE_ASN_ADDRESS), 5);
  assert_eq!(weight(&engine, TELSTRA_ASN_ADDRESS), 0);
}

#[test]
fn geoip_condition_is_unknown_until_the_database_loads() {
  let engine = engine(
    "default_action: deny
rules:
  - {name: allow-gb, when: {country: [GB]}, action: allow}
",
  )
  .unwrap();
  let (action, _, unresolved) = decide(&engine, GB_ADDRESS);
  assert_eq!((action, unresolved), (PolicyAction::Deny, true));
  load(&engine);
  let (action, _, unresolved) = decide(&engine, GB_ADDRESS);
  assert_eq!((action, unresolved), (PolicyAction::Allow, false));
}

#[test]
fn geoip_treats_ipv4_mapped_ipv6_as_ipv4() {
  let engine = engine(SCORE_RULES).unwrap();
  load(&engine);
  assert_eq!(weight(&engine, &format!("::ffff:{GB_ADDRESS}")), 3);
  assert_eq!(weight(&engine, &format!("::ffff:{GOOGLE_ASN_ADDRESS}")), 5);
}

#[test]
fn geoip_condition_requires_its_database() {
  let error = PolicyEngine::from_definition(
    &serde_yaml_ng::from_str::<Definition>(
      "mode: custom\nrules:\n  - {name: r, when: {country: [GB]}, action: deny}\n",
    )
    .unwrap(),
  )
  .unwrap_err();
  assert!(
    format!("{error:#}").contains("policy.rules[0].when.country: requires policy.geoip.country")
  );
  let error =
    engine("rules:\n  - {name: r, when: {country: [GB, cn]}, action: deny}\n").unwrap_err();
  assert!(
    format!("{error:#}")
      .contains("policy.rules[0].when.country[1]: expected an ISO 3166-1 alpha-2 code")
  );
}

#[test]
fn invalid_database_keeps_the_previous_reader() {
  let engine = engine(SCORE_RULES).unwrap();
  load(&engine);
  let country = engine.geoip().country().unwrap();
  assert!(country.publish(b"not mmdb".to_vec()).is_err());
  assert!(country.publish(std::fs::read(ASN_DB).unwrap()).is_err());
  assert!(country.status().loaded);
  assert_eq!(weight(&engine, GB_ADDRESS), 3);
}

#[test]
fn asn_sets_match_through_the_asn_database() {
  let engine = engine("asn_sets: {dc: {asns: [15169]}}\nrules:\n  - {name: dc, when: {asn: {in_sets: [dc]}}, action: score, score: 5}\n").unwrap();
  load(&engine);
  assert_eq!(weight(&engine, GOOGLE_ASN_ADDRESS), 5);
  assert_eq!(weight(&engine, TELSTRA_ASN_ADDRESS), 0);
}
#[test]
fn remote_asn_set_is_unknown_until_loaded() {
  let engine = engine("asn_sets: {dc: {url: 'https://example.test/asns', format: list_text}}\nrules:\n  - {name: dc, when: {asn: {in_sets: [dc]}}, action: deny}\n").unwrap();
  load(&engine);
  let (action, _, unresolved) = decide(&engine, GOOGLE_ASN_ADDRESS);
  assert_eq!(action, PolicyAction::Challenge);
  assert!(unresolved);
  engine
    .sets()
    .asn
    .get("dc")
    .unwrap()
    .publish(rilgate_policy::AsnList::from_numbers([15169]));
  assert_eq!(decide(&engine, GOOGLE_ASN_ADDRESS).0, PolicyAction::Deny);
}
#[test]
fn asn_set_errors() {
  for (source, expected) in [
    (
      "asn_sets: {dc: {url: 'https://example.test/asns', format: ip_prefixes_json}}\nrules: []",
      "policy.asn_sets.dc.format: ip_prefixes_json holds IP networks, use list_text, regex or json_path",
    ),
    (
      "rules: [{name: dc, when: {asn: {in_sets: [missing]}}, action: deny}]",
      "policy.rules[0].when.asn.in_sets: unknown asn set missing",
    ),
    (
      "asn_sets: {dc: {asns: [0]}}\nrules: []",
      "policy.asn_sets.dc.asns[0]: expected an autonomous system number from 1 to 4294967295",
    ),
  ] {
    let error = engine(source).unwrap_err();
    assert!(format!("{error:#}").contains(expected), "{error:#}");
  }
  let definition: Definition =
    serde_yaml_ng::from_str("mode: balanced\nasn_sets: {dc: {asns: [15169]}}").unwrap();
  assert!(
    format!(
      "{:#}",
      PolicyEngine::from_definition(&definition).unwrap_err()
    )
    .contains("policy: asn_sets require mode: custom")
  );
}
