use crate::{
  loader::file::FileConfig,
  resolution::{compatibility::normalize_internal_prefix, lookup, resolve},
};
use rilgate_web::WebConfig;
pub(super) fn load(file: &FileConfig) -> WebConfig {
  let legacy_prefix = resolve(
    file,
    "pow.internal_prefix",
    &[],
    None,
    "/.well-known/rilgate".to_string(),
  );
  let mut web: WebConfig = file.section("web").unwrap_or_default();
  web.name = resolve(file, "web.name", &[], None, web.name);
  web.website_url = resolve(file, "web.website_url", &[], None, web.website_url);
  web.stylesheet_path = resolve(file, "web.stylesheet_path", &[], None, web.stylesheet_path);
  if let Some(root) = lookup::<String>(file, "web.root", &[], None) {
    web.root = Some(root.into());
  }
  web.base_path =
    normalize_internal_prefix(resolve(file, "web.base_path", &[], None, legacy_prefix));
  web.seed.empty_directory = resolve(
    file,
    "web.seed.empty_directory",
    &[],
    None,
    web.seed.empty_directory,
  );
  web.watch.enabled = resolve(file, "web.watch.enabled", &[], None, web.watch.enabled);
  if let Some(value) = lookup::<String>(file, "web.watch.debounce", &[], None) {
    web.watch.debounce =
      humantime::parse_duration(&value).expect("Valid web.watch.debounce duration");
  }
  web.templates.cache = resolve(file, "web.templates.cache", &[], None, web.templates.cache);
  web.templates.strict = resolve(
    file,
    "web.templates.strict",
    &[],
    None,
    web.templates.strict,
  );
  web.templates.recursion_limit = resolve(
    file,
    "web.templates.recursion_limit",
    &[],
    None,
    web.templates.recursion_limit,
  );
  web.templates.fuel = resolve(file, "web.templates.fuel", &[], None, web.templates.fuel);
  web.i18n.default_locale = resolve(
    file,
    "web.i18n.default_locale",
    &[],
    None,
    web.i18n.default_locale,
  );
  web.i18n.strict = resolve(file, "web.i18n.strict", &[], None, web.i18n.strict);
  web.i18n.language_cookie_name = resolve(
    file,
    "web.i18n.language_cookie_name",
    &[],
    None,
    web.i18n.language_cookie_name,
  );
  if let Ok(locales) = std::env::var("WEB_I18N_SUPPORTED_LOCALES") {
    web.i18n.supported_locales = locales
      .split(',')
      .map(|tag| tag.trim().to_string())
      .collect();
  }

  web
}
