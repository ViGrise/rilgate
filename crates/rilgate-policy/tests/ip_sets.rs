use rilgate_policy::IpRanges;

fn ranges(nets: &[&str]) -> IpRanges {
  IpRanges::from_networks(nets.iter().map(|n| n.parse().unwrap()))
}

#[test]
fn ranges_match_inclusive_boundaries_and_merge_overlaps() {
  let set = ranges(&[
    "192.0.2.0/25",
    "192.0.2.64/26",
    "192.0.2.128/25",
    "198.51.100.7/32",
  ]);
  for ip in ["192.0.2.0", "192.0.2.255", "198.51.100.7"] {
    assert!(set.contains(ip.parse().unwrap()), "{ip}");
  }
  for ip in ["192.0.1.255", "192.0.3.0", "198.51.100.8"] {
    assert!(!set.contains(ip.parse().unwrap()), "{ip}");
  }
  assert_eq!(set.prefix_count(), 4);
}

#[test]
fn ranges_truncate_host_bits_and_cover_ipv6() {
  let set = ranges(&["203.0.113.77/24", "2001:db8::1/32"]);
  assert!(set.contains("203.0.113.1".parse().unwrap()));
  assert!(set.contains("2001:db8:ffff::1".parse().unwrap()));
  assert!(!set.contains("2001:db9::".parse().unwrap()));
}

#[test]
fn mapped_addresses_and_networks_match_as_ipv4() {
  let set = ranges(&["::ffff:192.0.2.0/120", "::/0"]);
  assert!(set.contains("192.0.2.9".parse().unwrap()));
  assert!(set.contains("::ffff:192.0.2.9".parse().unwrap()));
  assert!(!set.contains("198.51.100.1".parse().unwrap()));
  assert!(set.contains("2001:db8::1".parse().unwrap()));
}

#[test]
fn empty_ranges_match_nothing() {
  assert!(!ranges(&[]).contains("0.0.0.0".parse().unwrap()));
}

use http::{HeaderMap, Method};
use rilgate_policy::{PolicyAction, PolicyEngine, conditions::Request, policy::schema::Definition};

fn engine2(text: &str) -> anyhow::Result<PolicyEngine> {
  PolicyEngine::from_definition(&serde_yaml_ng::from_str::<Definition>(text)?)
}
fn action(engine: &PolicyEngine, ip: &str) -> PolicyAction {
  let headers = HeaderMap::new();
  engine
    .evaluate_context(
      &Request {
        host: "example.test",
        headers: &headers,
        path: "/",
        method: &Method::GET,
        client_ip: Some(ip.parse().unwrap()),
        lookups: &rilgate_policy::Lookups::NONE,
      },
      0,
    )
    .action
}
const STATIC: &str = "mode: custom\nip_sets:\n  office: {cidrs: [203.0.113.0/24]}\n  lab: {cidrs: ['2001:db8::/32']}\nrules:\n  - name: inside\n    when: {client_ip: {in_sets: [office, lab]}}\n    action: deny\n";

#[test]
fn in_sets_matches_any_listed_set() {
  let engine = engine2(STATIC).unwrap();
  assert_eq!(action(&engine, "203.0.113.5"), PolicyAction::Deny);
  assert_eq!(action(&engine, "2001:db8::5"), PolicyAction::Deny);
  assert_eq!(action(&engine, "198.51.100.5"), PolicyAction::Allow);
}

#[test]
fn ip_set_definitions_are_validated_with_their_path() {
  for (text, message) in [
    (
      STATIC.replace("[office, lab]", "[office, missing]"),
      "policy.rules[0].when.client_ip.in_sets: unknown ip set missing",
    ),
    (
      STATIC.replace(
        "{in_sets: [office, lab]}",
        "{in_sets: [office], cidr: 192.0.2.0/24}",
      ),
      "client_ip: expected exactly one of cidr, in_sets",
    ),
    (
      STATIC.replace("{in_sets: [office, lab]}", "{in_sets: []}"),
      "in_sets requires 1 to 16 set names",
    ),
    (
      STATIC.replace("  office:", "  Office:"),
      "policy.ip_sets.Office: name must contain 1 to 64 bytes of a-z, 0-9, - and _",
    ),
    (
      STATIC.replace("[203.0.113.0/24]", "[203.0.113.0/24, nope]"),
      "policy.ip_sets.office.cidrs[1]: expected an IPv4 or IPv6 network",
    ),
    (
      STATIC.replace("[203.0.113.0/24]", "[]"),
      "policy.ip_sets.office.cidrs: expected 1 to 65536 networks",
    ),
    (
      STATIC.replace("{cidrs: [203.0.113.0/24]}", "{}"),
      "policy.ip_sets.office: expected exactly one of cidrs, url",
    ),
    (
      "mode: balanced\nip_sets:\n  office: {cidrs: [203.0.113.0/24]}\n".into(),
      "policy: ip_sets require mode: custom",
    ),
  ] {
    let error = format!("{:#}", engine2(&text).unwrap_err());
    assert!(error.contains(message), "{error}");
  }
}

#[test]
fn compiled_registry_exposes_static_sets_as_loaded() {
  let engine = engine2(STATIC).unwrap();
  let office = engine.sets().ip.get("office").unwrap();
  assert_eq!(
    office.membership("203.0.113.1".parse().unwrap()),
    rilgate_policy::Membership::Member
  );
  assert_eq!(
    office.membership("192.0.2.1".parse().unwrap()),
    rilgate_policy::Membership::NotMember
  );
}

const REMOTE: &str = "mode: custom\nip_sets:\n  feed: {url: 'https://feeds.example/ips.json', format: ip_prefixes_json, refresh: 1h, max_stale: 2h}\nrules:\n";

fn engine3(text: &str) -> anyhow::Result<PolicyEngine> {
  PolicyEngine::from_definition(&serde_yaml_ng::from_str::<Definition>(text)?)
}
fn remote(rules: &str) -> PolicyEngine {
  engine3(&format!("{REMOTE}{rules}")).unwrap()
}
fn publish(engine: &PolicyEngine, nets: &[&str]) {
  engine
    .sets()
    .ip
    .get("feed")
    .unwrap()
    .publish(rilgate_policy::IpRanges::from_networks(
      nets.iter().map(|n| n.parse().unwrap()),
    ));
}
fn decide(engine: &PolicyEngine, ip: &str, score: i32) -> rilgate_policy::PolicyDecision {
  let headers = HeaderMap::new();
  engine.evaluate_context(
    &Request {
      host: "example.test",
      headers: &headers,
      path: "/",
      method: &Method::GET,
      client_ip: Some(ip.parse().unwrap()),
      lookups: &rilgate_policy::Lookups::NONE,
    },
    score,
  )
}

#[test]
fn unknown_sets_never_produce_a_more_permissive_outcome() {
  let allow = remote(
    "  - {name: a, when: {client_ip: {in_sets: [feed]}}, action: allow}\ndefault_action: deny\n",
  );
  assert_eq!(decide(&allow, "192.0.2.1", 0).action, PolicyAction::Deny);
  let negated = remote(
    "  - {name: a, when: {not: {client_ip: {in_sets: [feed]}}}, action: allow}\ndefault_action: deny\n",
  );
  assert_eq!(decide(&negated, "192.0.2.1", 0).action, PolicyAction::Deny);
  let deny = remote("  - {name: d, when: {client_ip: {in_sets: [feed]}}, action: deny}\n");
  let decision = decide(&deny, "192.0.2.1", 0);
  assert_eq!(
    (decision.action, &*decision.rule, decision.unresolved),
    (PolicyAction::Challenge, "d", true)
  );
  assert_eq!((decision.algorithm, decision.difficulty), (None, None));
  let scores = remote(
    "  - {name: up, when: {client_ip: {in_sets: [feed]}}, action: score, score: 5}\n  - {name: down, when: {client_ip: {in_sets: [feed]}}, action: score, score: -7}\n",
  );
  assert_eq!(decide(&scores, "192.0.2.1", 0).weight, 5);
}

#[test]
fn kleene_logic_resolves_known_branches() {
  let any = remote(
    "  - {name: a, when: {any: [{client_ip: {in_sets: [feed]}}, {path: {prefix: /}}]}, action: deny}\n",
  );
  let decision = decide(&any, "192.0.2.1", 0);
  assert_eq!(
    (decision.action, decision.unresolved),
    (PolicyAction::Deny, false)
  );
  let all = remote(
    "  - {name: a, when: {all: [{client_ip: {in_sets: [feed]}}, {path: {prefix: /never}}]}, action: allow}\ndefault_action: deny\n",
  );
  let decision = decide(&all, "192.0.2.1", 0);
  assert_eq!(decision.action, PolicyAction::Deny);
  assert!(!decision.unresolved);
}

#[test]
fn published_snapshots_resolve_membership_until_stale() {
  let engine = remote("  - {name: d, when: {client_ip: {in_sets: [feed]}}, action: deny}\n");
  publish(&engine, &["192.0.2.0/24"]);
  assert_eq!(decide(&engine, "192.0.2.1", 0).action, PolicyAction::Deny);
  assert_eq!(
    decide(&engine, "198.51.100.1", 0).action,
    PolicyAction::Allow
  );
  let feed = engine.sets().ip.get("feed").unwrap();
  let later = std::time::Instant::now() + std::time::Duration::from_secs(2 * 3600 + 1);
  assert_eq!(
    feed.membership_at("192.0.2.1".parse().unwrap(), later),
    rilgate_policy::Membership::Unknown
  );
  assert!(feed.status_at(later).stale);
  feed.record_failure();
  assert_eq!(feed.status().refresh_failures, 1);
  assert_eq!(feed.status().entries, 1);
}

#[test]
fn lookups_racing_publishes_see_complete_snapshots() {
  let engine = std::sync::Arc::new(remote(
    "  - {name: d, when: {client_ip: {in_sets: [feed]}}, action: deny}\n",
  ));
  publish(&engine, &["192.0.2.0/24"]);
  std::thread::scope(|scope| {
    let writer = engine.clone();
    scope.spawn(move || {
      for i in 0..2000 {
        publish(
          &writer,
          if i % 2 == 0 {
            &["198.51.100.0/24"]
          } else {
            &["192.0.2.0/24"]
          },
        );
      }
    });
    for _ in 0..4 {
      let reader = engine.clone();
      scope.spawn(move || {
        for _ in 0..20_000 {
          let feed = reader.sets().ip.get("feed").unwrap();
          let a = feed.membership("192.0.2.1".parse().unwrap());
          assert_ne!(a, rilgate_policy::Membership::Unknown);
        }
      });
    }
  });
}

#[test]
fn remote_definitions_are_validated_and_deny_rules_register_fallback_challenges() {
  for (text, message) in [
    (
      REMOTE.replace("refresh: 1h", "refresh: 59m"),
      "policy.ip_sets.feed.refresh: expected 1h to 7d",
    ),
    (
      REMOTE.replace("max_stale: 2h", "max_stale: 30m"),
      "policy.ip_sets.feed.max_stale: expected refresh to 30d",
    ),
    (
      REMOTE.replace("format: ip_prefixes_json, ", ""),
      "policy.ip_sets.feed.format: required with url",
    ),
    (
      REMOTE.replace(
        "url: 'https://feeds.example/ips.json'",
        "url: 'ftp://feeds.example/x'",
      ),
      "policy.ip_sets.feed.url: expected an absolute http or https URL",
    ),
    (
      "mode: custom\nip_sets:\n  s: {cidrs: [192.0.2.0/24], refresh: 1h}\nrules:\n".into(),
      "policy.ip_sets.s.refresh: only valid with url",
    ),
  ] {
    let error = format!(
      "{:#}",
      engine3(&format!(
        "{text}  - {{name: x, when: {{path: {{prefix: /}}}}, action: allow}}\n"
      ))
      .unwrap_err()
    );
    assert!(error.contains(message), "{error}");
  }
  let definition: Definition = serde_yaml_ng::from_str(&format!(
    "{REMOTE}  - {{name: d, when: {{not: {{client_ip: {{in_sets: [feed]}}}}}}, action: deny}}\n"
  ))
  .unwrap();
  assert!(
    definition
      .challenges()
      .unwrap()
      .iter()
      .any(|(name, c)| name == "d (unknown ip set)" && c.algorithm.is_none())
  );
}

#[test]
fn decisions_record_any_rule_that_depended_on_unknown_data() {
  let challenge =
    remote("  - {name: c, when: {client_ip: {in_sets: [feed]}}, action: challenge}\n");
  let decision = decide(&challenge, "192.0.2.1", 0);
  assert_eq!(decision.action, PolicyAction::Challenge);
  assert!(decision.unresolved);
  let skipped = remote(
    "  - {name: a, when: {client_ip: {in_sets: [feed]}}, action: allow}\n  - {name: d, when: {path: {prefix: /}}, action: deny}\n",
  );
  let decision = decide(&skipped, "192.0.2.1", 0);
  assert_eq!(
    (decision.action, &*decision.rule),
    (PolicyAction::Deny, "d")
  );
  assert!(decision.unresolved);
  let scored =
    remote("  - {name: up, when: {client_ip: {in_sets: [feed]}}, action: score, score: 5}\n");
  let decision = decide(&scored, "192.0.2.1", 0);
  assert_eq!((decision.action, decision.weight), (PolicyAction::Allow, 5));
  assert!(decision.unresolved);
  publish(&scored, &["198.51.100.0/24"]);
  assert!(!decide(&scored, "192.0.2.1", 0).unresolved);
}

#[test]
fn feed_format_forms_parse_and_round_trip() {
  use rilgate_policy::FeedFormat;
  for yaml in [
    "ip_prefixes_json",
    "list_text",
    "{regex: '(?P<value>.+)'}",
    "{json_path: ['$.asns[*]']}",
  ] {
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml).unwrap();
    let format: FeedFormat = serde_yaml_ng::from_value(value).unwrap();
    let serialized = serde_yaml_ng::to_value(&format).unwrap();
    assert_eq!(
      format,
      serde_yaml_ng::from_value::<FeedFormat>(serialized).unwrap()
    );
  }
}
#[test]
fn invalid_feed_formats_are_rejected_at_load() {
  let paths = format!("{{json_path: [{}]}}", ["'$.asns'"; 17].join(","));
  let long_regex = format!("{{regex: '{}'}}", "a".repeat(1025));
  for (format, expected) in [
    (
      "unknown",
      "unknown format unknown, expected ip_prefixes_json, list_text, {regex: ...} or {json_path: [...]}",
    ),
    (
      "{other: x}",
      "unknown format key other, expected regex or json_path",
    ),
    ("{}", "expected one of regex, json_path"),
    (
      "{regex: x, json_path: ['$.x']}",
      "expected one of regex, json_path",
    ),
    (
      "42",
      "expected a format name or a map with regex or json_path",
    ),
    (
      "{regex: x}",
      "policy.ip_sets.feed.format.regex: expected a capture group named value",
    ),
    (
      "{regex: '['}",
      "policy.ip_sets.feed.format.regex: regex parse error",
    ),
    (
      &long_regex,
      "policy.ip_sets.feed.format.regex: pattern exceeds 1024 bytes",
    ),
    (
      "{json_path: []}",
      "policy.ip_sets.feed.format.json_path: expected 1 to 16 paths",
    ),
    (
      &paths,
      "policy.ip_sets.feed.format.json_path: expected 1 to 16 paths",
    ),
    (
      "{json_path: ['[']}",
      "policy.ip_sets.feed.format.json_path[0]:",
    ),
  ] {
    let error = engine2(&format!("mode: custom\nip_sets:\n  feed: {{url: 'https://example.test/feed', format: {format}}}\nrules: []\n")).unwrap_err();
    assert!(format!("{error:#}").contains(expected), "{error:#}");
  }
}
