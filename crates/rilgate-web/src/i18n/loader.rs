use super::locale;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;

pub type TranslationMaps = BTreeMap<String, BTreeMap<String, String>>;
pub fn load(entries: &BTreeMap<String, Vec<u8>>) -> Result<TranslationMaps> {
  let mut result = BTreeMap::new();
  for (name, bytes) in entries {
    let Some(tag) = name
      .strip_prefix("i18n/")
      .and_then(|name| name.strip_suffix(".json"))
    else {
      continue;
    };
    let tag = locale::normalize(tag).with_context(|| format!("Translation file {name}"))?;
    let map: BTreeMap<String, String> =
      serde_json::from_slice(bytes).with_context(|| format!("Translation file {name}"))?;
    ensure!(
      map
        .keys()
        .all(|key| !key.is_empty() && !key.chars().any(char::is_control)),
      "Invalid translation key in {name}"
    );
    ensure!(
      !result.contains_key(&tag),
      "Duplicate normalized translation locale {tag}"
    );
    result.insert(tag, map);
  }
  Ok(result)
}
