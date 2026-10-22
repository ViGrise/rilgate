use super::shell::{self, BOOTSTRAP_SLOT, ChallengeShell};
use crate::{
  assets::{AssetIndex, AssetMetadata},
  config::WebConfig,
  i18n::{
    loader as i18n_loader,
    locale::{self, SupportedLocales},
    store::TranslationStore,
  },
  resources::embedded::EmbeddedWeb,
  resources::loader,
  templates::context::{PageInfo, PublicChallengeInfo, PublicRilgateInfo, TemplateContext},
  templates::renderer,
};
use anyhow::{Context, Result, ensure};
use minijinja::{Environment, context};
use std::{collections::BTreeMap, sync::Arc};

/// Translations the browser is handed for one locale.
///
/// Resolved per locale when the snapshot is built, so rendering reuses the same visible key set.
type ClientTranslations = Arc<BTreeMap<String, String>>;

pub struct WebSnapshot {
  pub generation: u64,
  pub config: WebConfig,
  pub templates: Environment<'static>,
  pub translations: Arc<TranslationStore>,
  pub assets: AssetIndex,
  /// Locales this deployment serves, resolved once.
  pub supported: SupportedLocales,
  /// Public site details, derived from config once.
  pub public: Arc<PublicRilgateInfo>,
  client_i18n: BTreeMap<String, ClientTranslations>,
  client_i18n_json: BTreeMap<String, minijinja::Value>,
  challenge_shells: BTreeMap<String, ChallengeShell>,
}
impl WebSnapshot {
  pub fn build(config: &WebConfig, generation: u64, overrides: bool) -> Result<Self> {
    config.validate()?;
    let embedded = EmbeddedWeb::entries();
    let mut sources: BTreeMap<String, Vec<u8>> = embedded
      .iter()
      .filter(|(name, _)| !name.starts_with("static/"))
      .map(|(name, bytes)| (name.clone(), bytes.to_vec()))
      .collect();
    let mut assets = embedded
      .iter()
      .filter_map(|(name, bytes)| {
        name
          .strip_prefix("static/")
          .map(|name| (name.to_string(), AssetMetadata::embedded(name, bytes)))
      })
      .collect::<AssetIndex>();
    let mut custom_i18n = BTreeMap::new();
    let mut custom_templates = false;
    if !assets.contains_key(&config.stylesheet_path) {
      assets.insert(
        config.stylesheet_path.clone(),
        AssetMetadata::embedded(
          &config.stylesheet_path,
          EmbeddedWeb::get("static/css/rilgate.css").expect("Embedded stylesheet"),
        ),
      );
    }
    if overrides && let Some(root) = config.root.as_ref().filter(|root| root.exists()) {
      let root = loader::canonical_root(root)?;
      let files = loader::files(&root)?;
      for (name, path) in &files {
        if let Some(asset) = name.strip_prefix("static/") {
          let canonical = crate::assets::canonical_asset(asset);
          if canonical != asset && files.contains_key(&format!("static/{canonical}")) {
            continue;
          }
          assets.insert(
            canonical.to_string(),
            AssetMetadata::file(&root, canonical, path)?,
          );
        } else if name.starts_with("templates/") {
          let bytes = loader::read_checked(&root, path)?;
          custom_templates |= sources.get(name) != Some(&bytes);
          sources.insert(name.clone(), bytes);
        } else if name.starts_with("i18n/") {
          custom_i18n.insert(name.clone(), loader::read_checked(&root, path)?);
        }
      }
    }
    crate::resources::bundle::assemble(&mut assets)?;
    let mut maps = i18n_loader::load(&sources)?;
    for (locale, keys) in i18n_loader::load(&custom_i18n)? {
      maps.entry(locale).or_default().extend(keys);
    }
    let translations = Arc::new(TranslationStore {
      locales: maps
        .into_iter()
        .map(|(tag, map)| {
          (
            tag,
            map
              .into_iter()
              .map(|(key, value)| (key, crate::i18n::store::CompiledMessage::new(value)))
              .collect(),
          )
        })
        .collect(),
      default_locale: locale::normalize(&config.i18n.default_locale)?,
      strict: config.i18n.strict,
    });
    let templates = sources
      .into_iter()
      .filter_map(|(name, bytes)| {
        name
          .strip_prefix("templates/")
          .map(|logical| (logical.to_string(), bytes))
      })
      .map(|(name, bytes)| {
        Ok((
          name.clone(),
          String::from_utf8(bytes).with_context(|| format!("Template {name} is not UTF-8"))?,
        ))
      })
      .collect::<Result<BTreeMap<_, _>>>()?;
    let templates = renderer::compile(&config.templates, templates, translations.clone())?;
    if let Some(wasm) = assets.get("js/solvers/randomx/randomx_wasm.wasm") {
      let revision = wasm.etag.trim_matches('"').to_string();
      for name in [
        "js/solvers/randomx/solver.js",
        crate::resources::bundle::BUNDLE_PATH,
        "js/solvers/fast/worker.js",
      ] {
        if let Some(asset) = assets.get(name) {
          let bytes = asset.bytes()?;
          let source = if name == "js/solvers/randomx/solver.js" {
            String::from_utf8(bytes).context("Solver module is not UTF-8")?
          } else if let Ok(source) = String::from_utf8(bytes) {
            source
          } else {
            continue;
          };
          if source.contains("__WASM_REVISION__") {
            assets.insert(
              name.into(),
              AssetMetadata::memory(
                name,
                source.replace("__WASM_REVISION__", &revision).into_bytes(),
              ),
            );
          }
        }
      }
    }
    let supported = SupportedLocales::resolve(&config.i18n, translations.locales.keys())?;
    let client_i18n = client_translations(&translations, supported.tags())?;
    let client_i18n_json = client_i18n
      .iter()
      .map(|(locale, entries)| {
        Ok((
          locale.clone(),
          minijinja::Value::from_safe_string(shell::script_json(&**entries)?),
        ))
      })
      .collect::<Result<_>>()?;
    let public = Arc::new(PublicRilgateInfo::from(config));
    let mut snapshot = Self {
      generation,
      config: config.clone(),
      templates,
      translations,
      assets,
      supported,
      public,
      client_i18n,
      client_i18n_json,
      challenge_shells: BTreeMap::new(),
    };
    snapshot.validate()?;
    if !custom_templates {
      for locale in snapshot.supported.tags() {
        let public = snapshot.challenge_context(PublicChallengeInfo::default(), locale)?;
        let html = snapshot.render_with_bootstrap(
          "pages/challenge.html.jinja",
          &public,
          locale,
          BOOTSTRAP_SLOT.into(),
        )?;
        snapshot
          .challenge_shells
          .insert(locale.clone(), ChallengeShell::new(html)?);
      }
    }
    Ok(snapshot)
  }

  fn validate(&self) -> Result<()> {
    let challenge = PublicChallengeInfo {
      id: "validation".into(),
      token: "validation".into(),
      difficulty: Some(1),
      expires_at: Some(2_000_000_000),
      expires_in: Some(60),
      solver_module_url: format!(
        "{}/static/js/solvers/randomx/solver.js",
        self.config.base_path
      ),
      ..Default::default()
    };
    let mut public = TemplateContext {
      rilgate: self.public.clone(),
      page: PageInfo {
        title: "Validation".into(),
        description: String::new(),
      },
      challenge,
    };
    // Template iteration holds the loader lock, so names are collected before rendering.
    let names = self
      .templates
      .templates()
      .map(|(name, _)| name.to_string())
      .collect::<Vec<_>>();
    for name in names {
      for locale in self.supported.tags() {
        for (algorithm, difficulty) in [("fast", 1), ("randomx", 1000)] {
          public.challenge.algorithm = algorithm.into();
          public.challenge.difficulty = Some(difficulty);
          self.render(&name, &public, locale)?;
        }
      }
    }
    for page in [
      "pages/challenge.html.jinja",
      "pages/denied.html.jinja",
      "pages/error.html.jinja",
      "pages/challenge.json.jinja",
      "pages/denied.json.jinja",
    ] {
      self
        .templates
        .get_template(page)
        .with_context(|| format!("Missing page {page}"))?;
    }
    for name in [
      "js/challenge.bundle.js",
      "js/challenge.js",
      "js/challenge/runtime.js",
      "js/challenge/registry.js",
      "js/challenge/p2pool.js",
      "js/solvers/fast/solver.js",
      "js/solvers/fast/adapter.js",
      "js/solvers/fast/worker.js",
      "js/solvers/randomx/adapter.js",
      "js/solvers/randomx/loader.js",
      "js/solvers/randomx/solver.js",
      "js/solvers/randomx/randomx-worker.js",
      "js/solvers/randomx/randomx_wasm.wasm",
    ] {
      ensure!(
        self.assets.contains_key(name),
        "Embedded browser resource is missing: {name}"
      );
    }
    Ok(())
  }

  fn challenge_context(
    &self,
    challenge: PublicChallengeInfo,
    locale: &str,
  ) -> Result<TemplateContext> {
    Ok(TemplateContext {
      rilgate: self.public.clone(),
      page: PageInfo {
        title: self
          .translations
          .translate(locale, "challenge.title", &BTreeMap::new())?,
        description: self.translations.translate(
          locale,
          "challenge.description",
          &BTreeMap::new(),
        )?,
      },
      challenge,
    })
  }

  pub fn render_challenge(&self, challenge: &PublicChallengeInfo, locale: &str) -> Result<String> {
    if let Some(shell) = self.challenge_shells.get(locale) {
      return shell.render(&self.public.base_path, challenge);
    }
    let public = self.challenge_context(challenge.clone(), locale)?;
    self.render("pages/challenge.html.jinja", &public, locale)
  }

  pub fn render_challenge_json(
    &self,
    challenge: &PublicChallengeInfo,
    locale: &str,
  ) -> Result<String> {
    let public = self.challenge_context(challenge.clone(), locale)?;
    self.render("pages/challenge.json.jinja", &public, locale)
  }

  pub fn render(&self, name: &str, public: &TemplateContext, locale: &str) -> Result<String> {
    self.render_with_bootstrap(
      name,
      public,
      locale,
      shell::bootstrap(&public.rilgate.base_path, &public.challenge)?,
    )
  }

  fn render_with_bootstrap(
    &self,
    name: &str,
    public: &TemplateContext,
    locale: &str,
    bootstrap: String,
  ) -> Result<String> {
    let client_i18n = match self.client_i18n.get(locale) {
      Some(entries) => entries.clone(),
      None => {
        // Rendering is only ever asked for a served locale, all of which are
        // precomputed. Resolving here keeps an unexpected tag renderable.
        let mut resolved = client_translations(&self.translations, &[locale.to_string()])?;
        resolved
          .remove(locale)
          .unwrap_or_else(|| Arc::new(BTreeMap::new()))
      }
    };
    let client_i18n_json = match self.client_i18n_json.get(locale) {
      Some(json) => json.clone(),
      None => minijinja::Value::from_safe_string(shell::script_json(&*client_i18n)?),
    };
    let html = self
      .templates
      .get_template(name)?
      .render(context! {
        rilgate => &*public.rilgate, page => &public.page, challenge => &public.challenge,
        locale => locale, client_i18n => &*client_i18n,
        client_i18n_json => client_i18n_json, challenge_bootstrap => minijinja::Value::from_safe_string(bootstrap),
      })
      .with_context(|| format!("Rendering template {name} for locale {locale}"))?;
    crate::templates::json::validate(name, &html)?;
    Ok(html)
  }
}

/// Resolves the translations the browser is handed, for each served locale.
///
/// The visible key set is taken from the `en` map so every locale exposes the
/// same keys, which is what lets a page rely on a key existing.
fn client_translations(
  translations: &TranslationStore,
  locales: &[String],
) -> Result<BTreeMap<String, ClientTranslations>> {
  let keys = translations
    .locales
    .get("en")
    .into_iter()
    .flat_map(|map| map.keys())
    .filter(|key| {
      key.starts_with("challenge.") || key.starts_with("pow.") || key.starts_with("common.")
    })
    .cloned()
    .collect::<Vec<String>>();

  let mut resolved = BTreeMap::new();
  for locale in locales {
    let mut entries = BTreeMap::new();
    for key in &keys {
      entries.insert(
        key.clone(),
        translations.translate(locale, key, &BTreeMap::new())?,
      );
    }
    resolved.insert(locale.clone(), Arc::new(entries));
  }
  Ok(resolved)
}
