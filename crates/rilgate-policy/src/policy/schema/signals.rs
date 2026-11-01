use super::{Condition, present};
use rilgate_crypto::Secret;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What Rilgate knows about a request beyond the request itself.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Signals {
  #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
  pub ip_sets: BTreeMap<String, IpSetDefinition>,
  #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
  pub asn_sets: BTreeMap<String, AsnSetDefinition>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub geoip: Option<GeoIpDefinition>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub dns: Option<DnsDefinition>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub honeypot: Option<HoneypotDefinition>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub counters: Option<BTreeMap<String, CounterDefinition>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct IpSetDefinition {
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub cidrs: Option<Vec<String>>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub url: Option<String>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub format: Option<FeedFormat>,
  #[serde(
    default,
    with = "humantime_serde",
    skip_serializing_if = "Option::is_none"
  )]
  pub refresh: Option<std::time::Duration>,
  #[serde(
    default,
    with = "humantime_serde",
    skip_serializing_if = "Option::is_none"
  )]
  pub max_stale: Option<std::time::Duration>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AsnSetDefinition {
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub asns: Option<Vec<u32>>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub url: Option<String>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub format: Option<FeedFormat>,
  #[serde(
    default,
    with = "humantime_serde",
    skip_serializing_if = "Option::is_none"
  )]
  pub refresh: Option<std::time::Duration>,
  #[serde(
    default,
    with = "humantime_serde",
    skip_serializing_if = "Option::is_none"
  )]
  pub max_stale: Option<std::time::Duration>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeoIpDefinition {
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub country: Option<GeoDatabaseDefinition>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub asn: Option<GeoDatabaseDefinition>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeoDatabaseDefinition {
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub path: Option<std::path::PathBuf>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub maxmind: Option<MaxMindSource>,
  #[serde(
    default,
    with = "humantime_serde",
    skip_serializing_if = "Option::is_none"
  )]
  pub refresh: Option<std::time::Duration>,
  #[serde(
    default,
    with = "humantime_serde",
    skip_serializing_if = "Option::is_none"
  )]
  pub max_stale: Option<std::time::Duration>,
}

/// Settings of the hidden link that flags clients which follow it.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HoneypotDefinition {
  /// How long a client stays flagged.
  #[serde(with = "humantime_serde")]
  pub ttl: std::time::Duration,
  /// The most clients that are flagged at one time.
  pub max_entries: usize,
}

impl Default for HoneypotDefinition {
  fn default() -> Self {
    Self {
      ttl: std::time::Duration::from_secs(24 * 3600),
      max_entries: 65_536,
    }
  }
}

impl HoneypotDefinition {
  pub const MAX_TTL: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);
  pub const MAX_ENTRIES: usize = 1_048_576;

  pub(crate) fn validate(&self) -> anyhow::Result<()> {
    use anyhow::ensure;
    ensure!(
      (std::time::Duration::from_secs(1)..=Self::MAX_TTL).contains(&self.ttl),
      "signals.honeypot.ttl: expected 1s to 30d"
    );
    ensure!(
      (1..=Self::MAX_ENTRIES).contains(&self.max_entries),
      "signals.honeypot.max_entries: expected 1 to {}",
      Self::MAX_ENTRIES
    );
    Ok(())
  }
}

/// Settings of a named sliding request counter.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CounterDefinition {
  pub key: Vec<CounterKeyPart>,
  #[serde(with = "humantime_serde")]
  pub window: std::time::Duration,
  #[serde(default = "counter_capacity")]
  pub max_entries: usize,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub count_when: Option<Condition>,
}

fn counter_capacity() -> usize {
  65_536
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum CounterKeyPart {
  Field(CounterField),
  ClientIp { client_ip: CounterIp },
  Header { header: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CounterField {
  ClientIp,
  Host,
  Method,
  Path,
  UserAgent,
  Country,
  Asn,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CounterIp {
  pub ipv4_prefix: u8,
  pub ipv6_prefix: u8,
}

impl Default for CounterIp {
  fn default() -> Self {
    Self {
      ipv4_prefix: 32,
      ipv6_prefix: 64,
    }
  }
}

/// Settings of the DNS lookups behind `dnsbl` and `client_ip.reverse_dns`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct DnsDefinition {
  /// The time one request waits for the lookups of its client.
  #[serde(with = "humantime_serde")]
  pub timeout: std::time::Duration,
  /// How long a listed address or a verified name stays cached.
  #[serde(with = "humantime_serde")]
  pub cache_ttl: std::time::Duration,
  /// How long an unlisted address or a missing name stays cached.
  #[serde(with = "humantime_serde")]
  pub negative_ttl: std::time::Duration,
  pub max_entries: usize,
}

impl Default for DnsDefinition {
  fn default() -> Self {
    Self {
      timeout: std::time::Duration::from_secs(1),
      cache_ttl: std::time::Duration::from_secs(3600),
      negative_ttl: std::time::Duration::from_secs(600),
      max_entries: 65_536,
    }
  }
}

impl DnsDefinition {
  pub const MAX_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
  pub const MAX_ENTRIES: usize = 1_048_576;

  pub(crate) fn validate(&self) -> anyhow::Result<()> {
    use anyhow::ensure;
    use std::time::Duration;
    ensure!(
      (Duration::from_millis(1)..=Self::MAX_TIMEOUT).contains(&self.timeout),
      "signals.dns.timeout: expected 1ms to 5s"
    );
    ensure!(
      self.cache_ttl >= Duration::from_secs(1),
      "signals.dns.cache_ttl: expected a duration of at least 1s"
    );
    ensure!(
      self.negative_ttl >= Duration::from_secs(1),
      "signals.dns.negative_ttl: expected a duration of at least 1s"
    );
    ensure!(
      (1..=Self::MAX_ENTRIES).contains(&self.max_entries),
      "signals.dns.max_entries: expected 1 to {}",
      Self::MAX_ENTRIES
    );
    Ok(())
  }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaxMindSource {
  pub edition: String,
  #[serde(deserialize_with = "text_or_number")]
  pub account_id: String,
  pub license_key: Secret,
}

pub use crate::feed_sets::FeedFormat;

/// Reads a YAML number or string, because account identifiers are digits.
fn text_or_number<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
  #[derive(Deserialize)]
  #[serde(untagged)]
  enum Value {
    Text(String),
    Number(u64),
  }
  Ok(match Value::deserialize(deserializer)? {
    Value::Text(text) => text,
    Value::Number(number) => number.to_string(),
  })
}
