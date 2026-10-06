use super::WebConfig;
use anyhow::{Result, ensure};
use std::time::Duration;
pub fn valid_cookie_name(name: &str) -> bool {
  !name.is_empty()
    && name
      .bytes()
      .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

impl WebConfig {
  pub fn validate(&self) -> Result<()> {
    ensure!(
      !self.name.trim().is_empty() && !self.name.chars().any(char::is_control),
      "web.name must contain visible text without control characters"
    );
    let url = url::Url::parse(&self.website_url)?;
    ensure!(
      matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none(),
      "web.website_url must be HTTP or HTTPS without credentials"
    );
    ensure!(
      self.base_path.starts_with('/')
        && self.base_path != "/"
        && !self.base_path.ends_with('/')
        && !self.base_path.split('/').any(|p| p == "." || p == "..")
        && !self
          .base_path
          .chars()
          .any(|c| c.is_control()
            || matches!(c, '"' | '\'' | '<' | '>' | '\\' | '?' | '#' | '{' | '}')),
      "web.base_path must be a safe non-root URL path without a trailing slash"
    );
    crate::resources::loader::logical_path(&self.stylesheet_path)?;
    ensure!(
      self.root.as_ref().is_none_or(|p| !p.as_os_str().is_empty()),
      "web.root cannot be empty"
    );
    ensure!(
      self.templates.cache,
      "web.templates.cache must be enabled; templates are memory-backed"
    );
    ensure!(
      (1..=256).contains(&self.templates.recursion_limit) && self.templates.fuel > 0,
      "Template recursion and fuel limits must be positive and bounded"
    );
    ensure!(
      !self.watch.debounce.is_zero() && self.watch.debounce <= Duration::from_secs(60),
      "web.watch.debounce must be between 1ns and 60s"
    );
    ensure!(
      valid_cookie_name(&self.i18n.language_cookie_name),
      "web.i18n.language_cookie_name must be an HTTP token"
    );
    crate::i18n::locale::normalize(&self.i18n.default_locale)?;
    // Whether the default locale is actually served depends on the translation
    // files, so WebSnapshot::build checks that once the loaded set is known.
    for tag in &self.i18n.supported_locales {
      crate::i18n::locale::normalize(tag)?;
    }
    Ok(())
  }
}
