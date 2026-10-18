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
