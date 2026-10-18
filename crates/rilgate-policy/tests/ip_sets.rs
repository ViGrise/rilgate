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
