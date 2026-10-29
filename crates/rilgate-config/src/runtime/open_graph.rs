use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, time::Duration};

#[derive(Debug, Clone)]
pub struct OpenGraphConfig {
  pub enabled: bool,
  pub ttl: Duration,
  pub negative_ttl: Duration,
  pub timeout: Duration,
  pub max_entries: u64,
  pub include_query: bool,
  pub tags: crate::schema::site::OpenGraphTags,
  pub when: Option<rilgate_policy::policy::schema::Condition>,
  pub request_headers: BTreeMap<String, String>,
  pub overrides: BTreeMap<String, String>,
}
impl Default for OpenGraphConfig {
  fn default() -> Self {
    crate::schema::site::OpenGraph::default()
      .resolve()
      .expect("Valid Open Graph defaults")
  }
}
impl OpenGraphConfig {
  pub fn compile_condition(
    &self,
    sets: &rilgate_policy::IpSets,
    geoip: &rilgate_policy::GeoDatabases,
  ) -> Result<Option<rilgate_policy::conditions::CompiledCondition>> {
    self
      .when
      .as_ref()
      .map(|condition| {
        rilgate_policy::conditions::CompiledCondition::compile(
          condition,
          "site.open_graph.when",
          sets,
          geoip,
        )
      })
      .transpose()
  }

  pub fn validate(&self) -> Result<()> {
    self.tags.validate()?;
    ensure!(
      (Duration::from_secs(60)..=Duration::from_secs(604800)).contains(&self.ttl),
      "site.open_graph.ttl: expected 1m to 7d"
    );
    ensure!(
      (Duration::from_secs(10)..=self.ttl).contains(&self.negative_ttl),
      "site.open_graph.negative_ttl: expected 10s to ttl"
    );
    ensure!(
      (Duration::from_millis(100)..=Duration::from_secs(10)).contains(&self.timeout),
      "site.open_graph.timeout: expected 100ms to 10s"
    );
    ensure!(
      (1..=1_000_000).contains(&self.max_entries),
      "site.open_graph.max_entries: expected 1 to 1000000"
    );
    ensure!(
      self.request_headers.len() <= 16,
      "site.open_graph.request_headers: expected at most 16 headers"
    );
    for (name, value) in &self.request_headers {
      let path = format!("site.open_graph.request_headers.{name}");
      let header = http::header::HeaderName::from_bytes(name.as_bytes())
        .with_context(|| format!("{path}: invalid header name"))?;
      ensure!(
        !matches!(
          header.as_str(),
          "host"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "upgrade"
            | "te"
            | "trailer"
            | "keep-alive"
            | "proxy-connection"
        ),
        "{path}: forbidden header name"
      );
      let literal = value.replace("{host}", "");
      ensure!(
        !literal.contains('{'),
        "{path}: unknown placeholder; expected {{host}}"
      );
      ensure!(
        literal.len() <= 1024,
        "{path}: expected at most 1024 bytes after expansion"
      );
      http::HeaderValue::from_str(value)
        .with_context(|| format!("{path}: invalid header value"))?;
    }
    ensure!(
      self.overrides.len() <= 32,
      "site.open_graph.override: expected at most 32 entries"
    );
    for (name, content) in &self.overrides {
      let path = format!("site.open_graph.override.{name}");
      ensure!(
        name.len() <= 64 && self.tags.matches(&name.to_ascii_lowercase()),
        "{path}: expected an allowlisted name of at most 64 bytes"
      );
      ensure!(content.len() <= 1024, "{path}: expected at most 1024 bytes");
    }
    Ok(())
  }
}
