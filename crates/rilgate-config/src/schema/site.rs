use rilgate_web::config::{ResponseConfig, TemplateConfig, WatchConfig};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Site {
  pub name: String,
  pub url: String,
  pub base_path: String,
  pub stylesheet: String,
  pub resources: Resources,
  pub templates: TemplateConfig,
  pub responses: ResponseConfig,
  pub i18n: I18n,
  pub open_graph: OpenGraph,
}
impl Default for Site {
  fn default() -> Self {
    Self {
      name: "Rilgate".into(),
      url: "https://rilgate.com".into(),
      base_path: "/.well-known/rilgate".into(),
      stylesheet: "css/rilgate.css".into(),
      resources: Resources::default(),
      templates: TemplateConfig::default(),
      responses: ResponseConfig::default(),
      i18n: I18n::default(),
      open_graph: OpenGraph::default(),
    }
  }
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Resources {
  pub directory: Option<PathBuf>,
  pub seed: Seed,
  pub watch: WatchConfig,
}
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Seed {
  Never,
  #[default]
  IfEmpty,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct I18n {
  pub default: String,
  pub supported: Vec<String>,
  pub strict: bool,
  pub cookie: String,
}
impl Default for I18n {
  fn default() -> Self {
    Self {
      default: "en".into(),
      supported: vec![],
      strict: false,
      cookie: "rilgate_lang".into(),
    }
  }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct OpenGraph {
  pub enabled: bool,
  pub ttl: super::value::Time,
  pub negative_ttl: super::value::Time,
  pub timeout: super::value::Time,
  pub max_entries: u64,
  pub include_query: bool,
  pub tags: OpenGraphTags,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub when: Option<rilgate_policy::policy::schema::Condition>,
  #[serde(deserialize_with = "lowercase_headers")]
  pub request_headers: std::collections::BTreeMap<String, String>,
  #[serde(rename = "override")]
  pub overrides: std::collections::BTreeMap<String, String>,
}
impl Default for OpenGraph {
  fn default() -> Self {
    Self {
      enabled: false,
      ttl: super::value::Time::seconds(86400),
      negative_ttl: super::value::Time::seconds(300),
      timeout: super::value::Time::seconds(2),
      max_entries: 10000,
      include_query: false,
      tags: OpenGraphTags::default(),
      when: None,
      request_headers: [
        ("user-agent".into(), "Rilgate-OpenGraph/1.0".into()),
        ("x-forwarded-host".into(), "{host}".into()),
        ("x-forwarded-proto".into(), "https".into()),
      ]
      .into(),
      overrides: Default::default(),
    }
  }
}
fn lowercase_headers<'de, D: serde::Deserializer<'de>>(
  deserializer: D,
) -> Result<std::collections::BTreeMap<String, String>, D::Error> {
  let headers = std::collections::BTreeMap::<String, String>::deserialize(deserializer)?;
  let mut normalized = std::collections::BTreeMap::new();
  for (name, value) in headers {
    let name = name.to_ascii_lowercase();
    if normalized.insert(name.clone(), value).is_some() {
      return Err(serde::de::Error::custom(format!(
        "site.open_graph.request_headers.{name}: duplicate header"
      )));
    }
  }
  Ok(normalized)
}
impl OpenGraph {
  pub fn resolve(&self) -> anyhow::Result<crate::OpenGraphConfig> {
    let config = crate::OpenGraphConfig {
      enabled: self.enabled,
      ttl: self.ttl.0,
      negative_ttl: self.negative_ttl.0,
      timeout: self.timeout.0,
      max_entries: self.max_entries,
      include_query: self.include_query,
      tags: self.tags.clone(),
      when: self.when.clone(),
      request_headers: self.request_headers.clone(),
      overrides: self.overrides.clone(),
    };
    config.validate()?;
    Ok(config)
  }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct OpenGraphTags {
  #[serde(deserialize_with = "lowercase_tags")]
  pub prefixes: Vec<String>,
  #[serde(deserialize_with = "lowercase_tags")]
  pub names: Vec<String>,
}
impl Default for OpenGraphTags {
  fn default() -> Self {
    Self {
      prefixes: ["og:", "twitter:", "fediverse:"].map(str::to_owned).into(),
      names: ["description", "keywords", "author"]
        .map(str::to_owned)
        .into(),
    }
  }
}
fn lowercase_tags<'de, D: serde::Deserializer<'de>>(
  deserializer: D,
) -> Result<Vec<String>, D::Error> {
  Vec::<String>::deserialize(deserializer).map(|values| {
    values
      .into_iter()
      .map(|value| value.to_ascii_lowercase())
      .collect()
  })
}
impl OpenGraphTags {
  pub fn matches(&self, name: &str) -> bool {
    self.prefixes.iter().any(|prefix| name.starts_with(prefix))
      || self.names.iter().any(|value| value == name)
  }
  pub fn validate(&self) -> anyhow::Result<()> {
    use anyhow::ensure;
    ensure!(
      !self.prefixes.is_empty() || !self.names.is_empty(),
      "site.open_graph.tags: prefixes and names cannot both be empty"
    );
    for (label, entries, limit) in [("prefixes", &self.prefixes, 16), ("names", &self.names, 32)] {
      let path = format!("site.open_graph.tags.{label}");
      ensure!(
        entries.len() <= limit,
        "{path}: expected at most {limit} entries"
      );
      let mut unique = std::collections::BTreeSet::new();
      for (index, entry) in entries.iter().enumerate() {
        ensure!(
          (1..=64).contains(&entry.len())
            && entry.bytes().all(|byte| byte.is_ascii_lowercase()
              || byte.is_ascii_digit()
              || matches!(byte, b':' | b'_' | b'.' | b'-')),
          "{path}[{index}]: expected 1 to 64 bytes of a-z, 0-9, :, _, . and -"
        );
        ensure!(unique.insert(entry), "{path}[{index}]: duplicate entry");
      }
    }
    Ok(())
  }
}
