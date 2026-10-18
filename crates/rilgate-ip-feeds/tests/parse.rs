use rilgate_ip_feeds::parse;
use rilgate_policy::policy::schema::FeedFormat::IpPrefixesJson;

#[test]
fn published_crawler_documents_parse_without_format_version() {
  let body = br#"{"creationTime":"2026-10-16T14:46:09.000000","prefixes":[{"ipv6Prefix":"2001:4860:4801:10::/64"},{"ipv4Prefix":"66.249.64.0/27","service":"Googlebot"}],"extra":1}"#;
  let ranges = parse(IpPrefixesJson, body).unwrap();
  assert_eq!(ranges.prefix_count(), 2);
  assert!(ranges.contains("66.249.64.31".parse().unwrap()));
}

#[test]
fn ambiguous_prefix_objects_are_ignored() {
  let body = br#"{"prefixes":[{"ipv4Prefix":"192.0.2.0/24","ipv6Prefix":"2001:db8::/32"},{},{"ipv4Prefix":"198.51.100.0/24"}]}"#;
  assert_eq!(parse(IpPrefixesJson, body).unwrap().prefix_count(), 1);
}

#[test]
fn invalid_documents_are_rejected() {
  for (body, message) in [
    (
      &br#"{"formatVersion":"2.0","prefixes":[{"ipv4Prefix":"192.0.2.0/24"}]}"#[..],
      "unsupported formatVersion 2.0",
    ),
    (
      br#"{"formatVersion":"1.3","prefixes":[{"ipv4Prefix":"192.0.2.0/24"}]}"#,
      "",
    ),
    (
      br#"{"prefixes":[{"ipv4Prefix":"192.0.2.0/33"}]}"#,
      "prefixes[0].ipv4Prefix: expected an IPv4 network",
    ),
    (
      br#"{"prefixes":[{"ipv4Prefix":"2001:db8::/32"}]}"#,
      "prefixes[0].ipv4Prefix: expected an IPv4 network",
    ),
    (br#"{"prefixes":[]}"#, "no usable prefixes"),
    (br#"{"items":[]}"#, "missing field `prefixes`"),
    (b"<html>captive portal</html>", "expected value"),
    (br#"{"prefixes":[{"ipv4Prefix":"192.0.2.0/24"}"#, "EOF"),
  ] {
    let result = parse(IpPrefixesJson, body);
    if message.is_empty() {
      assert!(result.is_ok());
      continue;
    }
    let error = format!("{:#}", result.unwrap_err());
    assert!(error.contains(message), "{error}");
  }
}

#[test]
fn documents_over_the_prefix_limit_are_rejected() {
  let prefixes: Vec<_> = (0..=rilgate_policy::MAX_PREFIXES_PER_SET as u32)
    .map(|i| format!(r#"{{"ipv4Prefix":"{}/32"}}"#, std::net::Ipv4Addr::from(i)))
    .collect();
  let body = format!(r#"{{"prefixes":[{}]}}"#, prefixes.join(","));
  assert!(
    format!("{:#}", parse(IpPrefixesJson, body.as_bytes()).unwrap_err())
      .contains("more than 65536 prefixes")
  );
}
