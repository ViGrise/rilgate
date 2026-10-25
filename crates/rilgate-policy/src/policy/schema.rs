use crate::ChallengePolicy;
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
  pub min: i32,
  pub action: Action,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub challenge: Option<ChallengePolicy>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
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
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeedFormat {
  IpPrefixesJson,
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
