mod conditions;
mod signals;

pub use conditions::{
  AsnMatch, Condition, IpMatch, LoadMatch, LoadWindow, NumberMatch, RateMatch, SameAs, StringMatch,
};
pub use signals::{
  AsnSetDefinition, CounterDefinition, CounterField, CounterIp, CounterKeyPart, DnsDefinition,
  FeedFormat, GeoDatabaseDefinition, GeoIpDefinition, HoneypotDefinition, IpSetDefinition,
  MaxMindSource, Signals,
};

use crate::ChallengePolicy;
use serde::{Deserialize, Serialize};

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
}

/// The policy section with the signals its conditions read.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PolicyDocument {
  pub signals: Signals,
  pub policy: Definition,
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

pub(crate) fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
  deserializer: D,
) -> Result<Option<T>, D::Error> {
  T::deserialize(deserializer).map(Some)
}
