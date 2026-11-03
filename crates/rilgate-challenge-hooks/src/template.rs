use anyhow::{Result, bail, ensure};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const URL_VALUE: &AsciiSet = &NON_ALPHANUMERIC
  .remove(b'-')
  .remove(b'.')
  .remove(b'_')
  .remove(b'~');
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
  BrowserUrl,
  Browser,
  Verify,
}
#[derive(Debug, Clone)]
enum Part {
  Literal(String),
  Placeholder(String),
}
#[derive(Debug, Clone)]
pub struct Template {
  parts: Vec<Part>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TemplateValue {
  String(String),
  Number(serde_json::Number),
  Bool(bool),
}
impl std::fmt::Display for TemplateValue {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::String(s) => f.write_str(s),
      Self::Number(n) => n.fmt(f),
      Self::Bool(b) => b.fmt(f),
    }
  }
}
#[derive(Debug, Default)]
pub struct Values {
  pub values: BTreeMap<String, TemplateValue>,
}
impl Template {
  pub fn parse(input: &str, context: Context) -> Result<Self> {
    let mut chars = input.chars().peekable();
    let mut parts = Vec::new();
    let mut literal = String::new();
    while let Some(c) = chars.next() {
      match c {
        '{' if chars.peek() == Some(&'{') => {
          chars.next();
          literal.push('{');
        }
        '}' if chars.peek() == Some(&'}') => {
          chars.next();
          literal.push('}');
        }
        '{' => {
          if !literal.is_empty() {
            parts.push(Part::Literal(std::mem::take(&mut literal)));
          }
          let mut name = String::new();
          loop {
            match chars.next() {
              Some('}') => break,
              Some('{') | None => bail!("unmatched brace"),
              Some(c) => name.push(c),
            }
          }
          let public = matches!(name.as_str(), "challenge_id" | "host" | "locale")
            || name.starts_with("params.");
          let redirect = matches!(name.as_str(), "state" | "callback");
          let private = matches!(
            name.as_str(),
            "client_ip" | "user_agent" | "result" | "verification_id"
          ) || name.starts_with("secrets.");
          ensure!(
            public || redirect || private,
            "unknown placeholder {{{name}}}"
          );
          ensure!(
            public
              || (redirect && context == Context::BrowserUrl)
              || (private && context == Context::Verify),
            "placeholder {{{name}}} is not allowed here"
          );
          parts.push(Part::Placeholder(name));
        }
        '}' => bail!("unmatched brace"),
        c => literal.push(c),
      }
    }
    if !literal.is_empty() {
      parts.push(Part::Literal(literal));
    }
    Ok(Self { parts })
  }
  pub fn validate_keys(
    &self,
    params: &BTreeMap<String, TemplateValue>,
    secrets: &BTreeMap<String, rilgate_crypto::Secret>,
  ) -> Result<()> {
    for part in &self.parts {
      if let Part::Placeholder(name) = part {
        if let Some(key) = name.strip_prefix("params.") {
          ensure!(params.contains_key(key), "unknown params key {{{key}}}");
        }
        if let Some(key) = name.strip_prefix("secrets.") {
          ensure!(secrets.contains_key(key), "unknown secrets key {{{key}}}");
        }
      }
    }
    Ok(())
  }
  pub fn is_placeholder(&self) -> bool {
    matches!(self.parts.as_slice(), [Part::Placeholder(_)])
  }
  pub fn render(&self, values: &Values) -> TemplateValue {
    if let [Part::Placeholder(name)] = self.parts.as_slice() {
      return values
        .values
        .get(name)
        .cloned()
        .unwrap_or_else(|| TemplateValue::String(String::new()));
    }
    TemplateValue::String(
      self
        .parts
        .iter()
        .map(|p| match p {
          Part::Literal(s) => s.clone(),
          Part::Placeholder(name) => values
            .values
            .get(name)
            .map(ToString::to_string)
            .unwrap_or_default(),
        })
        .collect(),
    )
  }
  pub fn render_url(&self, values: &Values) -> Result<url::Url> {
    let rendered: String = self
      .parts
      .iter()
      .map(|p| match p {
        Part::Literal(s) => s.clone(),
        Part::Placeholder(name) => utf8_percent_encode(
          &values
            .values
            .get(name)
            .map(ToString::to_string)
            .unwrap_or_default(),
          URL_VALUE,
        )
        .to_string(),
      })
      .collect();
    Ok(url::Url::parse(&rendered)?)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn browser_templates_reject_secrets() {
    assert!(
      Template::parse("{secrets.x}", Context::Browser)
        .unwrap_err()
        .to_string()
        .contains("is not allowed here")
    );
  }
  #[test]
  fn parameter_placeholder_preserves_numeric_type() {
    let mut values = Values::default();
    values.values.insert(
      "params.min_score".into(),
      TemplateValue::Number(serde_json::Number::from_f64(0.5).unwrap()),
    );
    assert!(matches!(
      Template::parse("{params.min_score}", Context::Verify)
        .unwrap()
        .render(&values),
      TemplateValue::Number(_)
    ));
  }
  #[test]
  fn url_values_cannot_inject_query_parameters() {
    let mut values = Values::default();
    values
      .values
      .insert("host".into(), TemplateValue::String("a&b c".into()));
    let url = Template::parse("https://example.com/?host={host}", Context::Browser)
      .unwrap()
      .render_url(&values)
      .unwrap();
    assert_eq!(url.as_str(), "https://example.com/?host=a%26b%20c");
  }
}
