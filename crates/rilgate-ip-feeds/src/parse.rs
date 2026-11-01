use anyhow::{Context, Result, ensure};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use rilgate_policy::IpRanges;
use rilgate_policy::MAX_PREFIXES_PER_SET;
use rilgate_policy::policy::schema::FeedFormat;
use serde::Deserialize;

#[derive(Deserialize)]
struct Document {
  #[serde(rename = "formatVersion")]
  format_version: Option<String>,
  prefixes: Vec<PrefixObject>,
}

#[derive(Deserialize)]
struct PrefixObject {
  #[serde(rename = "ipv4Prefix")]
  ipv4: Option<String>,
  #[serde(rename = "ipv6Prefix")]
  ipv6: Option<String>,
}

/// Parses a published feed body into an address index.
pub fn parse_ip(format: &FeedFormat, body: &[u8]) -> Result<IpRanges> {
  match format {
    FeedFormat::IpPrefixesJson => parse_ip_prefixes_json(body),
    _ => {
      let networks = format
        .extract(body)?
        .into_iter()
        .map(|value| {
          let parsed = if value.text.contains('/') {
            value.text.parse::<IpNet>().ok()
          } else {
            value.text.parse::<std::net::IpAddr>().ok().map(IpNet::from)
          };
          parsed.with_context(|| {
            format!(
              "{}: expected an IPv4 or IPv6 address or network, found \"{}\"",
              value.origin,
              rilgate_policy::feed_sets::quoted_value(&value.text)
            )
          })
        })
        .collect::<Result<Vec<_>>>()?;
      Ok(IpRanges::from_networks(networks))
    }
  }
}

fn parse_ip_prefixes_json(body: &[u8]) -> Result<IpRanges> {
  let document: Document =
    serde_json::from_slice(body).context("expected a JSON IP prefix document")?;
  if let Some(version) = &document.format_version {
    let major = version.split('.').next().unwrap_or(version);
    ensure!(major == "1", "unsupported formatVersion {version}");
  }
  let mut networks = Vec::new();
  for (index, object) in document.prefixes.iter().enumerate() {
    // A prefix object naming both families or neither is ignored; a named
    // prefix that is not a valid network rejects the whole document.
    if let (Some(ipv4), None) = (&object.ipv4, &object.ipv6) {
      let net: Ipv4Net = ipv4
        .parse()
        .with_context(|| format!("prefixes[{index}].ipv4Prefix: expected an IPv4 network"))?;
      networks.push(IpNet::V4(net));
    } else if let (None, Some(ipv6)) = (&object.ipv4, &object.ipv6) {
      let net: Ipv6Net = ipv6
        .parse()
        .with_context(|| format!("prefixes[{index}].ipv6Prefix: expected an IPv6 network"))?;
      networks.push(IpNet::V6(net));
    }
    ensure!(
      networks.len() <= MAX_PREFIXES_PER_SET,
      "more than {MAX_PREFIXES_PER_SET} prefixes"
    );
  }
  ensure!(!networks.is_empty(), "no usable prefixes");
  Ok(IpRanges::from_networks(networks))
}

/// Parses a published feed body into an autonomous system index.
pub fn parse_asn(format: &FeedFormat, body: &[u8]) -> Result<rilgate_policy::AsnList> {
  ensure!(
    !matches!(format, FeedFormat::IpPrefixesJson),
    "ip_prefixes_json holds IP networks"
  );
  let numbers = format
    .extract(body)?
    .into_iter()
    .map(|value| {
      let text = &value.text;
      let number = if text
        .get(..2)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("as"))
      {
        &text[2..]
      } else {
        text
      };
      number
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| number.parse::<u32>().ok())
        .flatten()
        .filter(|n| *n != 0)
        .with_context(|| {
          format!(
            "{}: expected an autonomous system number, found \"{}\"",
            value.origin,
            rilgate_policy::feed_sets::quoted_value(text)
          )
        })
    })
    .collect::<Result<Vec<_>>>()?;
  Ok(rilgate_policy::AsnList::from_numbers(numbers))
}
pub trait FeedParse: rilgate_policy::FeedIndex + Sized {
  const LABEL: &'static str;
  fn parse(format: &FeedFormat, body: &[u8]) -> Result<Self>;
}
impl FeedParse for IpRanges {
  const LABEL: &'static str = "IP set";
  fn parse(format: &FeedFormat, body: &[u8]) -> Result<Self> {
    parse_ip(format, body)
  }
}
impl FeedParse for rilgate_policy::AsnList {
  const LABEL: &'static str = "ASN set";
  fn parse(format: &FeedFormat, body: &[u8]) -> Result<Self> {
    parse_asn(format, body)
  }
}
