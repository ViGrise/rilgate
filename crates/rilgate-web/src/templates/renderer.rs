use crate::{
  config::TemplateConfig,
  i18n::{store::TranslationStore, translator},
};
use anyhow::{Context, Result};
use minijinja::{AutoEscape, Environment, UndefinedBehavior};
use std::{collections::BTreeMap, sync::Arc};

pub fn compile(
  config: &TemplateConfig,
  templates: BTreeMap<String, String>,
  translations: Arc<TranslationStore>,
) -> Result<Environment<'static>> {
  let mut env = Environment::new();
  env.set_auto_escape_callback(|name| {
    if name.ends_with(".html") {
      AutoEscape::Html
    } else {
      AutoEscape::None
    }
  });
  env.set_undefined_behavior(if config.strict {
    UndefinedBehavior::Strict
  } else {
    UndefinedBehavior::Lenient
  });
  env.set_recursion_limit(config.recursion_limit);
  env.set_fuel(Some(config.fuel));
  translator::register(&mut env, translations);
  for (name, source) in templates {
    crate::resources::loader::logical_path(&name)?;
    anyhow::ensure!(
      matches!(
        name.split('/').next(),
        Some("layouts" | "pages" | "partials")
      ),
      "Template must be under layouts, pages, or partials: {name}"
    );
    env
      .add_template_owned(name.clone(), source)
      .with_context(|| format!("Compiling template {name}"))?;
  }
  Ok(env)
}
