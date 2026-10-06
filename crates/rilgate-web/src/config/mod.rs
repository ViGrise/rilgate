mod validation;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};
pub use validation::valid_cookie_name;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebConfig {
  pub name: String,
  pub website_url: String,
  pub base_path: String,
  pub stylesheet_path: String,
  pub root: Option<PathBuf>,
  pub seed: SeedConfig,
  pub watch: WatchConfig,
  pub templates: TemplateConfig,
  pub i18n: I18nConfig,
}

impl Default for WebConfig {
  fn default() -> Self {
    Self {
      name: "Rilgate".into(),
      website_url: "https://rilgate.com".into(),
      base_path: "/.well-known/rilgate".into(),
      stylesheet_path: "css/rilgate.css".into(),
      root: None,
      seed: SeedConfig::default(),
      watch: WatchConfig::default(),
      templates: TemplateConfig::default(),
      i18n: I18nConfig::default(),
    }
  }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SeedConfig {
  pub empty_directory: bool,
}
impl Default for SeedConfig {
  fn default() -> Self {
    Self {
      empty_directory: true,
    }
  }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WatchConfig {
  pub enabled: bool,
  #[serde(with = "humantime_serde")]
  pub debounce: Duration,
}
impl Default for WatchConfig {
  fn default() -> Self {
    Self {
      enabled: false,
      debounce: Duration::from_millis(300),
    }
  }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TemplateConfig {
  pub cache: bool,
  pub strict: bool,
  pub recursion_limit: usize,
  pub fuel: u64,
}
impl Default for TemplateConfig {
  fn default() -> Self {
    Self {
      cache: true,
      strict: true,
      recursion_limit: 64,
      fuel: 50_000,
    }
  }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct I18nConfig {
  pub default_locale: String,
  /// Locales to offer, restricting what the translation files provide.
  ///
  /// Empty means every locale that has a translation file, so adding
  /// `web/i18n/vi.json` is enough to offer Vietnamese.
  pub supported_locales: Vec<String>,
  pub strict: bool,
  pub language_cookie_name: String,
}
impl Default for I18nConfig {
  fn default() -> Self {
    Self {
      default_locale: "en".into(),
      supported_locales: vec![],
      strict: false,
      language_cookie_name: "rilgate_lang".into(),
    }
  }
}
