use crate::config::I18nConfig;
use anyhow::{Result, ensure};

pub fn normalize(tag: &str) -> Result<String> {
  ensure!(
    !tag.is_empty() && tag.len() <= 128,
    "Invalid locale tag: {tag}"
  );
  Ok(language_tags::LanguageTag::parse(tag)?.into_string())
}

pub fn parents(tag: &str) -> Vec<String> {
  let mut result = vec![tag.to_string()];
  let mut parent = tag;
  while let Some((prefix, _)) = parent.rsplit_once('-') {
    parent = prefix;
    // A language-range fallback must not end with an extension singleton.
    if parent.rsplit('-').next().is_some_and(|p| p.len() == 1) {
      parent = parent.rsplit_once('-').map_or(parent, |(prefix, _)| prefix);
    }
    result.push(parent.to_string());
  }
  result
}

pub fn negotiate(
  config: &I18nConfig,
  explicit: Option<&str>,
  cookie: Option<&str>,
  accept: Option<&str>,
) -> String {
  let supported = config
    .supported_locales
    .iter()
    .filter_map(|tag| normalize(tag).ok())
    .collect::<Vec<_>>();
  let matched = |tag: &str| -> Option<String> {
    parents(&normalize(tag).ok()?)
      .into_iter()
      .find(|p| supported.contains(p))
  };
  for candidate in [explicit, cookie].into_iter().flatten() {
    if let Some(locale) = matched(candidate) {
      return locale;
    }
  }
  let mut requested = Vec::new();
  for (index, entry) in accept.unwrap_or("").split(',').take(64).enumerate() {
    let mut parts = entry.trim().split(';');
    let tag = parts.next().unwrap_or("").trim();
    let mut quality = 1.0_f32;
    for parameter in parts {
      let parameter = parameter.trim();
      if let Some(q) = parameter.strip_prefix("q=") {
        quality = q.parse::<f32>().unwrap_or(0.0);
      }
    }
    if quality.is_finite() && quality > 0.0 && quality <= 1.0 {
      requested.push((tag, quality, index));
    }
  }
  requested.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.2.cmp(&b.2)));
  for (tag, _, _) in requested {
    if tag == "*" {
      return normalize(&config.default_locale).unwrap_or_else(|_| "en".into());
    }
    if let Some(locale) = matched(tag) {
      return locale;
    }
  }
  normalize(&config.default_locale).unwrap_or_else(|_| "en".into())
}
