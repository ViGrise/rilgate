use http::{HeaderMap, Method};
use rilgate_policy::{
  Lookups, PolicyAction, PolicyDecision, PolicyEngine,
  conditions::{CompiledCondition, Request},
  policy::schema::PolicyDocument,
};

fn compile(text: &str) -> anyhow::Result<PolicyEngine> {
  PolicyEngine::from_definition(&serde_yaml_ng::from_str::<PolicyDocument>(text)?)
}

fn decide(engine: &PolicyEngine, rates: Vec<Option<u32>>, weight: i32) -> PolicyDecision {
  engine.evaluate_context(
    &Request {
      trusted_peer: false,
      host: "example.test",
      headers: &HeaderMap::new(),
      path: "/",
      method: &Method::GET,
      client_ip: Some("192.0.2.1".parse().unwrap()),
      lookups: &Lookups {
        rates,
        ..Lookups::NONE
      },
    },
    weight,
  )
}

#[test]
fn named_rates_and_fallback_select_the_required_decision() {
  let document: serde_yaml_ng::Value =
    serde_yaml_ng::from_str(include_str!("../../../config/examples/counters.yml")).unwrap();
  let mut definition: PolicyDocument = serde_yaml_ng::from_value(serde_yaml_ng::Value::Mapping(
    [
      ("signals".into(), document["signals"].clone()),
      ("policy".into(), document["policy"].clone()),
    ]
    .into_iter()
    .collect(),
  ))
  .unwrap();
  let engine = PolicyEngine::from_definition(&definition).unwrap();
  for (login, pages, weight, action, difficulty) in [
    (0, 0, 0, PolicyAction::Challenge, Some(2)),
    (0, 60, 20, PolicyAction::Challenge, Some(4)),
    (10, 0, 0, PolicyAction::Deny, None),
  ] {
    let decision = decide(&engine, vec![Some(login), Some(pages)], 0);
    assert_eq!(
      (decision.action, decision.weight, decision.difficulty),
      (action, weight, difficulty)
    );
  }
  for weight in [i32::MIN, -1, 0].into_iter().chain(1..20) {
    let decision = decide(&engine, vec![Some(0), Some(0)], weight);
    assert_eq!(decision.difficulty, Some(2));
    assert_eq!(&*decision.rule, "score fallback");
  }
  assert_eq!(
    decide(&engine, vec![Some(0), Some(0)], 30).difficulty,
    Some(5)
  );
  definition.policy.score = None;
  let engine = PolicyEngine::from_definition(&definition).unwrap();
  assert_eq!(
    decide(&engine, vec![Some(0), Some(0)], 0).action,
    PolicyAction::Allow
  );
}

#[test]
fn an_unknown_count_cannot_allow() {
  let engine = compile("signals:\n  counters: {pages: {key: [client_ip], window: 1m}}\npolicy:\n  default_action: deny\n  rules: [{name: rate, when: {rate: {counter: pages, gte: 0}}, action: allow}]").unwrap();
  assert_eq!(decide(&engine, vec![None], 0).action, PolicyAction::Deny);
  assert_eq!(
    decide(&engine, vec![Some(1)], 0).action,
    PolicyAction::Allow
  );
}

#[test]
fn unavailable_conditions_and_invalid_fallbacks_report_their_paths() {
  let error = compile("policy:\n  rules: [{name: a, when: {any: [{rate: {counter: missing, gte: 1}}]}, action: deny}]").unwrap_err().to_string();
  assert_eq!(
    error,
    "policy.rules[0].when.any[0].rate.counter: unknown counter missing"
  );
  for (condition, field) in [
    ("{score: {gte: 0}}", "score"),
    ("{rate: {counter: pages, gte: 0}}", "rate"),
    ("{dnsbl: dnsbl.example.com}", "dnsbl"),
    (
      "{client_ip: {reverse_dns: [example.com]}}",
      "client_ip.reverse_dns",
    ),
  ] {
    let error = compile(&format!("signals:\n  counters: {{pages: {{key: [client_ip], window: 1m, count_when: {{not: {condition}}}}}}}\n")).unwrap_err().to_string();
    assert_eq!(
      error,
      format!("signals.counters.pages.count_when.not.{field}: unavailable in count_when")
    );
  }
  assert_eq!(
    compile("signals:\n  counters: {pages: {key: [asn], window: 1m}}\n")
      .unwrap_err()
      .to_string(),
    "signals.counters.pages.key.asn: requires signals.geoip.asn"
  );
  for thresholds in [
    "[{action: allow}, {min: 1, action: deny}]",
    "[{action: allow}, {action: deny}]",
  ] {
    assert_eq!(
      compile(&format!("policy:\n  score: {{thresholds: {thresholds}}}"))
        .unwrap_err()
        .to_string(),
      "policy.score.thresholds: only the last threshold can omit min"
    );
  }
  assert_eq!(
    CompiledCondition::compile(
      &serde_yaml_ng::from_str("{rate: {counter: pages, gte: 1}}").unwrap(),
      "site.open_graph.when",
      &Default::default(),
      &Default::default(),
    )
    .unwrap_err()
    .to_string(),
    "site.open_graph.when.rate: rate is only available in policy rules"
  );
}

#[test]
fn key_parts_separate_hosts_join_networks_and_bound_header_values() {
  let engine = compile("signals:\n  counters:\n    hosts: {key: [client_ip, host], window: 1m}\n    networks: {key: [{client_ip: {ipv4_prefix: 24}}], window: 1m}\n    headers: {key: [{header: X-Counter}], window: 1m}\n").unwrap();
  let headers = HeaderMap::new();
  let request = Request {
    trusted_peer: false,
    headers: &headers,
    host: "a.example.com",
    path: "/",
    method: &Method::GET,
    client_ip: Some("192.0.2.1".parse().unwrap()),
    lookups: &Lookups::NONE,
  };
  let key =
    |index: usize, request: &Request<'_>| engine.counters()[index].key(request, engine.geoip());
  assert_ne!(
    key(1, &request),
    key(
      1,
      &Request {
        host: "b.example.com",
        ..request
      }
    )
  );
  assert_eq!(
    key(2, &request),
    key(
      2,
      &Request {
        client_ip: Some("192.0.2.99".parse().unwrap()),
        ..request
      }
    )
  );
  assert_eq!(
    key(2, &request),
    key(
      2,
      &Request {
        client_ip: Some("::ffff:192.0.2.99".parse().unwrap()),
        ..request
      }
    )
  );
  assert!(key(0, &request).is_none());
  let mut long_headers = HeaderMap::new();
  long_headers.insert("x-counter", "a".repeat(1024).parse().unwrap());
  let request = Request {
    headers: &long_headers,
    ..request
  };
  let first = key(0, &request).unwrap();
  assert!(matches!(
    first[0],
    rilgate_policy::counters::KeyPart::Digest(_)
  ));
  let mut other_headers = long_headers.clone();
  other_headers.insert(
    "x-counter",
    format!("{}b", "a".repeat(1023)).parse().unwrap(),
  );
  assert_ne!(
    Some(first),
    key(
      0,
      &Request {
        headers: &other_headers,
        ..request
      }
    )
  );
}

#[test]
fn invalid_limits_duplicate_parts_and_impossible_ranges_are_rejected() {
  for settings in [
    "key: [], window: 1m",
    "key: [host, host], window: 1m",
    "key: [client_ip, {client_ip: {ipv4_prefix: 24}}], window: 1m",
    "key: [{header: X-Test}, {header: x-test}], window: 1m",
    "key: [host], window: 0s",
    "key: [host], window: 3601s",
    "key: [host], window: 1m, max_entries: 0",
    "key: [host], window: 1m, max_entries: 1048577",
    "key: [{client_ip: {ipv6_prefix: 0}}], window: 1m",
  ] {
    assert!(
      compile(&format!(
        "signals:\n  counters: {{pages: {{{settings}}}}}\n"
      ))
      .is_err(),
      "{settings}"
    );
  }
  for bounds in [
    "",
    "gt: 3, lt: 4",
    "eq: 2, gte: 3",
    "lt: 0",
    "gt: 4294967295",
  ] {
    assert!(compile(&format!("signals:\n  counters: {{pages: {{key: [host], window: 1m}}}}\npolicy:\n  rules: [{{name: rate, when: {{rate: {{counter: pages, {bounds}}}}}, action: deny}}]")).is_err(), "{bounds}");
  }
  for bounds in [
    "eq: 4294967295",
    "gt: 2147483647",
    "gte: 4294967295, lte: 4294967295",
  ] {
    let engine = compile(&format!("signals:\n  counters: {{pages: {{key: [host], window: 1m}}}}\npolicy:\n  rules: [{{name: rate, when: {{rate: {{counter: pages, {bounds}}}}}, action: deny}}]")).unwrap();
    assert_eq!(
      decide(&engine, vec![Some(u32::MAX)], 0).action,
      PolicyAction::Deny
    );
  }
}

#[test]
fn request_selection_controls_counting_and_unused_counters_are_skipped() {
  let engine = compile("signals:\n  counters:\n    login: {key: [client_ip], window: 1m, count_when: {path: {prefix: /login}}}\n    unknown: {key: [client_ip], window: 1m, count_when: {client_ip: {in_sets: [remote]}}}\n    unused: {key: [host], window: 1m}\n  ip_sets:\n    remote: {url: 'https://example.com/prefixes.json', format: ip_prefixes_json}\npolicy:\n  rules:\n    - {name: login, when: {rate: {counter: login, gte: 10}}, action: deny}\n    - {name: unknown, when: {rate: {counter: unknown, gte: 10}}, action: deny}").unwrap();
  let headers = HeaderMap::new();
  let request = Request {
    trusted_peer: false,
    headers: &headers,
    host: "example.com",
    path: "/page",
    method: &Method::GET,
    client_ip: Some("192.0.2.1".parse().unwrap()),
    lookups: &Lookups::NONE,
  };
  let mut counts = [0; 3];
  let mut read = |index: usize, _, selected: bool| {
    counts[index] += u32::from(selected);
    Some(counts[index])
  };
  assert_eq!(
    engine.counter_values(&request, &mut read),
    vec![Some(0), Some(0), None]
  );
  assert_eq!(
    engine.counter_values(
      &Request {
        path: "/login",
        ..request
      },
      &mut read
    ),
    vec![Some(1), Some(0), None]
  );
  assert_eq!(
    engine.counter_values(&request, &mut read),
    vec![Some(1), Some(0), None]
  );
  assert_eq!(
    engine.counter_values(
      &Request {
        client_ip: None,
        ..request
      },
      &mut read
    ),
    vec![None, None, None]
  );
}
