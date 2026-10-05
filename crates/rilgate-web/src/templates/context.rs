use crate::config::WebConfig;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PublicRilgateInfo {
  pub name: String,
  pub version: &'static str,
  pub website_url: String,
  pub docs_url: String,
  pub base_path: String,
  pub static_url: String,
  pub stylesheet_url: String,
}
impl From<&WebConfig> for PublicRilgateInfo {
  fn from(config: &WebConfig) -> Self {
    Self {
      name: config.name.clone(),
      version: env!("CARGO_PKG_VERSION"),
      website_url: config.website_url.clone(),
      docs_url: config.website_url.clone(),
      base_path: config.base_path.clone(),
      stylesheet_url: format!("{}/static/{}", config.base_path, config.stylesheet_path),
      static_url: format!("{}/static", config.base_path),
    }
  }
}
#[derive(Debug, Clone, Serialize)]
pub struct PageInfo {
  pub title: String,
  pub description: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct PublicChallengeInfo {
  pub id: String,
  pub token: String,
  pub algorithm: String,
  pub difficulty: Option<u64>,
  pub expires_at: Option<i64>,
  pub expires_in: Option<u64>,
  pub solver_module_url: String,
}
impl Default for PublicChallengeInfo {
  fn default() -> Self {
    Self {
      id: String::new(),
      token: String::new(),
      algorithm: "fast".into(),
      difficulty: None,
      expires_at: None,
      expires_in: None,
      solver_module_url: String::new(),
    }
  }
}
#[derive(Debug, Clone, Serialize)]
pub struct TemplateContext {
  pub rilgate: PublicRilgateInfo,
  pub page: PageInfo,
  pub challenge: PublicChallengeInfo,
}
