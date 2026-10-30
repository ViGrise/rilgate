use crate::ChallengePolicy;
use rilgate_crypto::Secret;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
  Off,
  Light,
  #[default]
  Balanced,
  Strict,
  Custom,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
  Allow,
  Deny,
  Challenge,
  Score,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Definition {
  pub mode: Mode,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub default_action: Option<Action>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub rules: Option<Vec<Rule>>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub score: Option<Scoring>,
  #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
  pub ip_sets: BTreeMap<String, IpSetDefinition>,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
  pub name: String,
  pub when: Condition,
  pub action: Action,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub score: Option<i32>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub challenge: Option<ChallengePolicy>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scoring {
  pub initial: i32,
  pub debug_header: DebugHeader,
  pub thresholds: Vec<Threshold>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct DebugHeader {
  pub enabled: bool,
  pub name: String,
}
impl Default for DebugHeader {
  fn default() -> Self {
    Self {
      enabled: false,
      name: "x-pow-weight".into(),
    }
  }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Threshold {
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub min: Option<i32>,
  pub action: Action,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub challenge: Option<ChallengePolicy>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Condition {
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub all: Option<Vec<Condition>>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub any: Option<Vec<Condition>>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub not: Option<Box<Condition>>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub path: Option<StringMatch>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub host: Option<StringMatch>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub method: Option<StringMatch>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub user_agent: Option<StringMatch>,
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  pub headers: BTreeMap<String, StringMatch>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub client_ip: Option<IpMatch>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub score: Option<NumberMatch>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub load: Option<LoadMatch>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub country: Option<Vec<String>>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub asn: Option<Vec<u32>>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub dnsbl: Option<String>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub honeypot: Option<bool>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub rate: Option<RateMatch>,
}
impl Condition {
  /// Reports whether this condition or any nested condition needs DNS lookups.
  pub fn uses_dns(&self) -> bool {
    self.dnsbl.is_some()
      || self
        .client_ip
        .as_ref()
        .is_some_and(|value| value.reverse_dns.is_some())
      || self
        .all
        .iter()
        .chain(self.any.iter())
        .flatten()
        .any(Self::uses_dns)
      || self.not.as_deref().is_some_and(Self::uses_dns)
  }

  /// Reports whether this condition or any nested condition reads a GeoIP database.
  pub fn uses_geoip(&self) -> bool {
    self.country.is_some()
      || self.asn.is_some()
      || self
        .all
        .iter()
        .chain(self.any.iter())
        .flatten()
        .any(Self::uses_geoip)
      || self.not.as_deref().is_some_and(Self::uses_geoip)
  }

  /// Reports whether this condition or any nested condition compares the host load.
  pub fn uses_load(&self) -> bool {
    self.load.is_some()
      || self
        .all
        .iter()
        .chain(self.any.iter())
        .flatten()
        .any(Self::uses_load)
      || self.not.as_deref().is_some_and(Self::uses_load)
  }

  pub(crate) fn reads_counter(&self, name: &str) -> bool {
    self.rate.as_ref().is_some_and(|rate| rate.counter == name)
      || self
        .all
        .iter()
        .chain(self.any.iter())
        .flatten()
        .any(|c| c.reads_counter(name))
      || self.not.as_deref().is_some_and(|c| c.reads_counter(name))
  }
}
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct StringMatch {
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub exact: Option<String>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub prefix: Option<String>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub contains: Option<String>,
  #[serde(
    rename = "in",
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub values: Option<Vec<String>>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub regex: Option<String>,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct NumberMatch {
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub eq: Option<i32>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub gt: Option<i32>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub gte: Option<i32>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub lt: Option<i32>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub lte: Option<i32>,
}
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum LoadWindow {
  #[serde(rename = "1m")]
  OneMinute,
  #[default]
  #[serde(rename = "5m")]
  FiveMinutes,
  #[serde(rename = "15m")]
  FifteenMinutes,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LoadMatch {
  pub window: LoadWindow,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub gt: Option<f64>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub gte: Option<f64>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub lt: Option<f64>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub lte: Option<f64>,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct IpMatch {
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub cidr: Option<String>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub in_sets: Option<Vec<String>>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub reverse_dns: Option<Vec<String>>,
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
      "policy.honeypot.ttl: expected 1s to 30d"
    );
    ensure!(
      (1..=Self::MAX_ENTRIES).contains(&self.max_entries),
      "policy.honeypot.max_entries: expected 1 to {}",
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

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RateMatch {
  pub counter: String,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub eq: Option<u32>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub gt: Option<u32>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub gte: Option<u32>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub lt: Option<u32>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub lte: Option<u32>,
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
      "policy.dns.timeout: expected 1ms to 5s"
    );
    ensure!(
      self.cache_ttl >= Duration::from_secs(1),
      "policy.dns.cache_ttl: expected a duration of at least 1s"
    );
    ensure!(
      self.negative_ttl >= Duration::from_secs(1),
      "policy.dns.negative_ttl: expected a duration of at least 1s"
    );
    ensure!(
      (1..=Self::MAX_ENTRIES).contains(&self.max_entries),
      "policy.dns.max_entries: expected 1 to {}",
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

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeedFormat {
  IpPrefixesJson,
}

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

pub(super) fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
  deserializer: D,
) -> Result<Option<T>, D::Error> {
  T::deserialize(deserializer).map(Some)
}

impl Condition {
  /// Reports whether the condition tree references any of the named IP sets.
  pub(crate) fn references(&self, names: &BTreeSet<&str>) -> bool {
    self
      .client_ip
      .as_ref()
      .and_then(|value| value.in_sets.as_ref())
      .is_some_and(|sets| sets.iter().any(|name| names.contains(name.as_str())))
      || self
        .all
        .as_ref()
        .is_some_and(|values| values.iter().any(|value| value.references(names)))
      || self
        .any
        .as_ref()
        .is_some_and(|values| values.iter().any(|value| value.references(names)))
      || self
        .not
        .as_ref()
        .is_some_and(|value| value.references(names))
  }
}
