use super::MAX_ENTRIES_PER_SET;
use anyhow::{Context, Result, bail, ensure};
use serde::{
  Deserialize, Deserializer, Serialize, Serializer,
  de::{self, MapAccess, Visitor},
  ser::SerializeMap,
};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedFormat {
  IpPrefixesJson,
  ListText,
  Regex(String),
  JsonPath(Vec<String>),
}

impl<'de> Deserialize<'de> for FeedFormat {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    struct FormatVisitor;
    impl<'de> Visitor<'de> for FormatVisitor {
      type Value = FeedFormat;
      fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a format name or a map with regex or json_path")
      }
      fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        match value {
          "ip_prefixes_json" => Ok(FeedFormat::IpPrefixesJson),
          "list_text" => Ok(FeedFormat::ListText),
          _ => Err(E::custom(format!(
            "unknown format {value}, expected ip_prefixes_json, list_text, {{regex: ...}} or {{json_path: [...]}}"
          ))),
        }
      }
      fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        let Some(key) = map.next_key::<String>()? else {
          return Err(de::Error::custom("expected one of regex, json_path"));
        };
        let format = match key.as_str() {
          "regex" => FeedFormat::Regex(map.next_value()?),
          "json_path" => FeedFormat::JsonPath(map.next_value()?),
          _ => {
            return Err(de::Error::custom(format!(
              "unknown format key {key}, expected regex or json_path"
            )));
          }
        };
        if map.next_key::<de::IgnoredAny>()?.is_some() {
          return Err(de::Error::custom("expected one of regex, json_path"));
        }
        Ok(format)
      }
    }
    deserializer.deserialize_any(FormatVisitor)
  }
}
impl Serialize for FeedFormat {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    match self {
      Self::IpPrefixesJson => serializer.serialize_str("ip_prefixes_json"),
      Self::ListText => serializer.serialize_str("list_text"),
      Self::Regex(pattern) => {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("regex", pattern)?;
        map.end()
      }
      Self::JsonPath(paths) => {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("json_path", paths)?;
        map.end()
      }
    }
  }
}

pub struct Extracted {
  pub origin: String,
  pub text: String,
}

/// Bounds quoted feed values on a UTF-8 character boundary.
pub fn quoted_value(value: &str) -> &str {
  let mut end = value.len().min(64);
  while !value.is_char_boundary(end) {
    end -= 1;
  }
  &value[..end]
}
impl FeedFormat {
  pub fn validate(&self, path: &str) -> Result<()> {
    match self {
      Self::Regex(pattern) => {
        ensure!(
          pattern.len() <= 1024,
          "{path}.regex: pattern exceeds 1024 bytes"
        );
        let regex = regex::RegexBuilder::new(pattern)
          .size_limit(1 << 20)
          .build()
          .map_err(|error| anyhow::anyhow!("{path}.regex: {error}"))?;
        ensure!(
          regex.capture_names().flatten().any(|name| name == "value"),
          "{path}.regex: expected a capture group named value"
        );
      }
      Self::JsonPath(paths) => {
        ensure!(
          (1..=16).contains(&paths.len()),
          "{path}.json_path: expected 1 to 16 paths"
        );
        for (index, value) in paths.iter().enumerate() {
          serde_json_path::JsonPath::parse(value)
            .map_err(|error| anyhow::anyhow!("{path}.json_path[{index}]: {error}"))?;
        }
      }
      _ => {}
    }
    Ok(())
  }
  pub fn extract(&self, body: &[u8]) -> Result<Vec<Extracted>> {
    let mut values = Vec::new();
    let mut push = |origin: String, text: String| -> Result<()> {
      ensure!(values.len() < MAX_ENTRIES_PER_SET, "more than 65536 values");
      values.push(Extracted { origin, text });
      Ok(())
    };
    match self {
      Self::ListText => {
        let text = std::str::from_utf8(body).context("expected UTF-8 text")?;
        for (index, line) in text.split('\n').enumerate() {
          let line = line.strip_suffix('\r').unwrap_or(line);
          let remainder = line
            .split(['#', ';'])
            .next()
            .unwrap_or("")
            .trim_matches(|c: char| c.is_ascii_whitespace());
          if remainder.is_empty() {
            continue;
          }
          let n = index + 1;
          ensure!(
            !remainder.bytes().any(|b| b.is_ascii_whitespace()),
            "line {n}: expected one value, found \"{}\"",
            quoted_value(remainder)
          );
          push(format!("line {n}"), remainder.to_string())?;
        }
      }
      Self::Regex(pattern) => {
        let text = std::str::from_utf8(body).context("expected UTF-8 text")?;
        let regex = regex::RegexBuilder::new(pattern)
          .size_limit(1 << 20)
          .build()?;
        for (index, capture) in regex.captures_iter(text).enumerate() {
          if let Some(value) = capture.name("value") {
            push(format!("match {}", index + 1), value.as_str().to_string())?;
          }
        }
      }
      Self::JsonPath(paths) => {
        let document: serde_json::Value =
          serde_json::from_slice(body).context("expected a JSON document")?;
        for (p, path) in paths.iter().enumerate() {
          let query = serde_json_path::JsonPath::parse(path)?;
          for (index, node) in query.query(&document).all().into_iter().enumerate() {
            let origin = format!("json_path[{p}] result {}", index + 1);
            let text = match node {
              serde_json::Value::String(value) => value.clone(),
              serde_json::Value::Number(value) if value.as_u64().is_some() => value.to_string(),
              _ => bail!("{origin}: expected a string or an integer"),
            };
            push(origin, text)?;
          }
        }
      }
      Self::IpPrefixesJson => bail!("ip_prefixes_json is read by the IP prefix parser"),
    }
    ensure!(!values.is_empty(), "no usable values");
    Ok(values)
  }
}
