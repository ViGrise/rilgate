use super::locale;
use minijinja::{Error, ErrorKind};
use std::collections::BTreeMap;

pub enum MessagePart {
  Text(String),
  Placeholder(String),
}
pub struct CompiledMessage {
  parts: Vec<MessagePart>,
}
impl CompiledMessage {
  pub fn new(message: String) -> Self {
    let mut parts = Vec::new();
    let mut rest = message.as_str();
    while let Some(start) = rest.find('{') {
      parts.push(MessagePart::Text(rest[..start].into()));
      rest = &rest[start..];
      let Some(end) = rest.find('}') else {
        break;
      };
      parts.push(MessagePart::Placeholder(rest[1..end].into()));
      rest = &rest[end + 1..];
    }
    parts.push(MessagePart::Text(rest.into()));
    Self { parts }
  }
  fn render(&self, args: &BTreeMap<String, String>) -> String {
    self
      .parts
      .iter()
      .map(|part| match part {
        MessagePart::Text(text) => text.clone(),
        MessagePart::Placeholder(key) => args
          .get(key)
          .cloned()
          .unwrap_or_else(|| format!("{{{key}}}")),
      })
      .collect()
  }
}
pub struct TranslationStore {
  pub locales: BTreeMap<String, BTreeMap<String, CompiledMessage>>,
  pub default_locale: String,
  pub strict: bool,
}
impl TranslationStore {
  pub fn translate(
    &self,
    locale: &str,
    key: &str,
    args: &BTreeMap<String, String>,
  ) -> Result<String, Error> {
    let chain = locale::parents(locale)
      .into_iter()
      .chain(locale::parents(&self.default_locale))
      .chain(std::iter::once("en".into()));
    let message = chain
      .filter_map(|tag| self.locales.get(&tag))
      .find_map(|map| map.get(key));
    let Some(message) = message else {
      // Rendering resolves every visible key on every request, so a missing
      // translation would otherwise emit one line per key per render. A missing
      // key is a content defect, and WebSnapshot::validate already resolves every
      // key for every supported locale at load time, where web.i18n.strict
      // decides whether it is fatal.
      tracing::debug!(locale, key, "Missing translation");
      if self.strict {
        return Err(Error::new(
          ErrorKind::UndefinedError,
          format!("Missing translation: {key}"),
        ));
      }
      return Ok(key.into());
    };
    Ok(message.render(args))
  }
}
