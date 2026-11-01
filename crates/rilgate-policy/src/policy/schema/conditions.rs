use super::present;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  pub edge_headers: BTreeMap<String, StringMatch>,
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
  pub asn: Option<AsnMatch>,
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
  /// Reports whether this condition or any nested condition meets `test`.
  pub(crate) fn any_node(&self, test: &impl Fn(&Condition) -> bool) -> bool {
    test(self)
      || self
        .all
        .iter()
        .chain(self.any.iter())
        .flatten()
        .any(|child| child.any_node(test))
      || self
        .not
        .as_deref()
        .is_some_and(|child| child.any_node(test))
  }

  /// Reports whether this condition or any nested condition needs DNS lookups.
  pub fn uses_dns(&self) -> bool {
    self.any_node(&|node| {
      node.dnsbl.is_some()
        || node
          .client_ip
          .as_ref()
          .is_some_and(|value| value.reverse_dns.is_some())
    })
  }

  /// Reports whether this condition or any nested condition reads a GeoIP database.
  pub fn uses_geoip(&self) -> bool {
    self.any_node(&|node| node.country.is_some() || node.asn.is_some())
  }

  /// Reports whether this condition or any nested condition compares the host load.
  pub fn uses_load(&self) -> bool {
    self.any_node(&|node| node.load.is_some())
  }

  pub(crate) fn reads_counter(&self, name: &str) -> bool {
    self.any_node(&|node| node.rate.as_ref().is_some_and(|rate| rate.counter == name))
  }

  /// Reports whether the condition tree references any of the named IP sets.
  pub(crate) fn references(&self, names: &BTreeSet<&str>) -> bool {
    self.any_node(&|node| {
      node
        .client_ip
        .as_ref()
        .and_then(|value| value.in_sets.as_ref())
        .is_some_and(|sets| sets.iter().any(|name| names.contains(name.as_str())))
    })
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
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub same_as: Option<SameAs>,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SameAs {
  Host,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsnMatch {
  Numbers(Vec<u32>),
  Sets(Vec<String>),
}

impl<'de> Deserialize<'de> for AsnMatch {
  fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    struct AsnVisitor;
    impl<'de> serde::de::Visitor<'de> for AsnVisitor {
      type Value = AsnMatch;
      fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a list of autonomous system numbers or {in_sets: [...]}")
      }
      fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<AsnMatch, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = seq.next_element()? {
          values.push(value);
        }
        Ok(AsnMatch::Numbers(values))
      }
      fn visit_map<M: serde::de::MapAccess<'de>>(self, mut map: M) -> Result<AsnMatch, M::Error> {
        let Some(key) = map.next_key::<String>()? else {
          return Err(serde::de::Error::custom(
            "expected a list of autonomous system numbers or {in_sets: [...]}",
          ));
        };
        if key != "in_sets" {
          return Err(serde::de::Error::custom(format!(
            "unknown asn key {key}, expected in_sets"
          )));
        }
        let values = map.next_value()?;
        if let Some(key) = map.next_key::<String>()? {
          return Err(serde::de::Error::custom(format!(
            "unknown asn key {key}, expected in_sets"
          )));
        }
        Ok(AsnMatch::Sets(values))
      }
    }
    deserializer.deserialize_any(AsnVisitor)
  }
}

impl Serialize for AsnMatch {
  fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    match self {
      Self::Numbers(values) => values.serialize(serializer),
      Self::Sets(values) => {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("in_sets", values)?;
        map.end()
      }
    }
  }
}
