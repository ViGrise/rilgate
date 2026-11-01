use crate::{
  WebConfig,
  schema::{Document, site::Seed},
};

impl Document {
  pub(super) fn resolve_web(&self) -> WebConfig {
    let site = &self.site;
    WebConfig {
      development: matches!(self.environment, crate::Environment::Development),
      name: site.name.clone(),
      website_url: site.url.clone(),
      base_path: site.base_path.clone(),
      stylesheet_path: site.stylesheet.clone(),
      content_security_policy: site.content_security_policy.clone(),
      root: site.resources.directory.clone(),
      seed: rilgate_web::config::SeedConfig {
        empty_directory: matches!(site.resources.seed, Seed::IfEmpty),
      },
      watch: site.resources.watch.clone(),
      templates: site.templates.clone(),
      responses: site.responses.clone(),
      i18n: rilgate_web::config::I18nConfig {
        default_locale: site.i18n.default.clone(),
        supported_locales: site.i18n.supported.clone(),
        strict: site.i18n.strict,
        language_cookie_name: site.i18n.cookie.clone(),
      },
    }
  }
}
