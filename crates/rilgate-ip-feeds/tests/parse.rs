use rilgate_ip_feeds::parse_ip;
use rilgate_policy::policy::schema::FeedFormat::IpPrefixesJson;

#[test]
fn published_crawler_documents_parse_without_format_version() {
  let body = br#"{"creationTime":"2026-10-16T14:46:09.000000","prefixes":[{"ipv6Prefix":"2001:4860:4801:10::/64"},{"ipv4Prefix":"66.249.64.0/27","service":"Googlebot"}],"extra":1}"#;
  let ranges = parse_ip(&IpPrefixesJson, body).unwrap();
  assert_eq!(ranges.prefix_count(), 2);
  assert!(ranges.contains("66.249.64.31".parse().unwrap()));
}

#[test]
fn ambiguous_prefix_objects_are_ignored() {
  let body = br#"{"prefixes":[{"ipv4Prefix":"192.0.2.0/24","ipv6Prefix":"2001:db8::/32"},{},{"ipv4Prefix":"198.51.100.0/24"}]}"#;
  assert_eq!(parse_ip(&IpPrefixesJson, body).unwrap().prefix_count(), 1);
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
    let result = parse_ip(&IpPrefixesJson, body);
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
    format!(
      "{:#}",
      parse_ip(&IpPrefixesJson, body.as_bytes()).unwrap_err()
    )
    .contains("more than 65536 prefixes")
  );
}

use rilgate_ip_feeds::parse_asn;
use rilgate_policy::{FeedFormat, FeedIndex};

#[test]
fn list_text_reads_single_addresses_as_host_networks() {
  let ranges = parse_ip(&FeedFormat::ListText, b"185.220.101.1\n2a0b:f4c2::1\n").unwrap();
  assert!(ranges.contains("185.220.101.1".parse().unwrap()));
  assert!(ranges.contains("2a0b:f4c2::1".parse().unwrap()));
  assert!(!ranges.contains("185.220.101.2".parse().unwrap()));
}
#[test]
fn list_text_ignores_comments_blank_lines_and_crlf() {
  let ranges = parse_ip(
    &FeedFormat::ListText,
    b"# header\r\n1.10.16.0/20 ; SBL256894\r\n\r\n",
  )
  .unwrap();
  assert_eq!(ranges.prefix_count(), 1);
}
#[test]
fn list_text_rejects_a_line_with_two_values() {
  assert!(
    format!(
      "{:#}",
      parse_ip(&FeedFormat::ListText, b"1.2.3.4 5.6.7.8\n").unwrap_err()
    )
    .contains("line 1: expected one value")
  );
}
#[test]
fn regex_reads_the_value_group() {
  let ranges = parse_ip(
    &FeedFormat::Regex(r"(?m)^(?P<value>[0-9.]+):\d+$".into()),
    b"192.0.2.1:8080\n192.0.2.2:3128\n",
  )
  .unwrap();
  for ip in ["192.0.2.1", "192.0.2.2"] {
    assert!(ranges.contains(ip.parse().unwrap()));
  }
  let error = parse_ip(
    &FeedFormat::Regex("(?m)^(?P<value>[a-z]+):".into()),
    b"bad:80",
  )
  .unwrap_err();
  assert!(format!("{error:#}").contains("match 1: expected an IPv4 or IPv6 address"));
}
#[test]
fn json_path_filters_by_sibling_field() {
  let body = br#"{"prefixes":[{"service":"EC2","ip_prefix":"192.0.2.0/24"},{"service":"S3","ip_prefix":"198.51.100.0/24"}]}"#;
  let ranges = parse_ip(
    &FeedFormat::JsonPath(vec!["$.prefixes[?@.service == 'EC2'].ip_prefix".into()]),
    body,
  )
  .unwrap();
  assert!(ranges.contains("192.0.2.1".parse().unwrap()));
  assert!(!ranges.contains("198.51.100.1".parse().unwrap()));
}
#[test]
fn asn_lists_accept_prefixed_and_plain_numbers() {
  let numbers = parse_asn(&FeedFormat::ListText, b"AS45090 # cloud\nas13335\n16509\n").unwrap();
  assert_eq!(numbers.entries(), 3);
  for number in [45090, 13335, 16509] {
    assert!(numbers.contains(number));
  }
  for body in [b"AS0\n", b"ASX\n"] {
    assert!(
      format!("{:#}", parse_asn(&FeedFormat::ListText, body).unwrap_err())
        .contains("line 1: expected an autonomous system number")
    );
  }
}
#[test]
fn asn_json_path_accepts_integers() {
  let numbers = parse_asn(
    &FeedFormat::JsonPath(vec!["$.asns[*]".into()]),
    br#"{"asns": [15169, "AS13335"]}"#,
  )
  .unwrap();
  assert!(numbers.contains(15169));
  assert!(numbers.contains(13335));
}
#[test]
fn extracted_values_over_the_limit_are_rejected() {
  let body = "192.0.2.1\n".repeat(65_537);
  assert!(
    format!(
      "{:#}",
      parse_ip(&FeedFormat::ListText, body.as_bytes()).unwrap_err()
    )
    .contains("more than 65536 values")
  );
}
