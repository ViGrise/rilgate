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
        headers: &headers,
        path: "/",
        method: &Method::GET,
        client_ip: Some(ip.parse().unwrap()),
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
  let office = engine.ip_sets().get("office").unwrap();
  assert_eq!(
    office.membership("203.0.113.1".parse().unwrap()),
    rilgate_policy::Membership::Member
  );
  assert_eq!(
    office.membership("192.0.2.1".parse().unwrap()),
    rilgate_policy::Membership::NotMember
  );
}
