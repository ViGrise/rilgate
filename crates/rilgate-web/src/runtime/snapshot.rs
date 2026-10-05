use crate::{
  assets::{AssetIndex, AssetMetadata},
  config::WebConfig,
  i18n::{loader as i18n_loader, locale, store::TranslationStore},
  resources::embedded::EmbeddedWeb,
  resources::loader,
  templates::context::{PageInfo, PublicChallengeInfo, PublicRilgateInfo, TemplateContext},
  templates::renderer,
};
use anyhow::{Context, Result, ensure};
use minijinja::{Environment, context};
use std::{collections::BTreeMap, sync::Arc};

pub struct WebSnapshot {
  pub generation: u64,
  pub config: WebConfig,
  pub templates: Environment<'static>,
  pub translations: Arc<TranslationStore>,
  pub assets: AssetIndex,
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
          sources.insert(name.clone(), loader::read_checked(&root, path)?);
        } else if name.starts_with("i18n/") {
          custom_i18n.insert(name.clone(), loader::read_checked(&root, path)?);
        }
      }
    }
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
      if let Some(solver) = assets.get("js/solvers/randomx/solver.js") {
        let source = String::from_utf8(solver.bytes()?).context("Solver module is not UTF-8")?;
        assets.insert(
          "js/solvers/randomx/solver.js".into(),
          AssetMetadata::memory(
            "js/solvers/randomx/solver.js",
            source.replace("__WASM_REVISION__", &revision).into_bytes(),
          ),
        );
      }
    }
    let snapshot = Self {
      generation,
      config: config.clone(),
      templates,
      translations,
      assets,
    };
    snapshot.validate()?;
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
      rilgate: PublicRilgateInfo::from(&self.config),
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
      for locale in &self.config.i18n.supported_locales {
        for (algorithm, difficulty) in [("fast", 1), ("randomx", 1000)] {
          public.challenge.algorithm = algorithm.into();
          public.challenge.difficulty = Some(difficulty);
          self.render(&name, &public, &locale::normalize(locale)?)?;
        }
      }
    }
    for page in [
      "pages/challenge.html",
      "pages/denied.html",
      "pages/error.html",
    ] {
      self
        .templates
        .get_template(page)
        .with_context(|| format!("Missing page {page}"))?;
    }
    for name in [
      "js/challenge.js",
      "js/challenge/runtime.js",
      "js/challenge/registry.js",
      "js/challenge/p2pool.js",
      "js/solvers/fast/solver.js",
      "js/solvers/fast/adapter.js",
      "js/solvers/fast/worker.js",
      "js/solvers/randomx/adapter.js",
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

  pub fn render(&self, name: &str, public: &TemplateContext, locale: &str) -> Result<String> {
    let mut client_i18n = BTreeMap::new();
    for key in self
      .translations
      .locales
      .get("en")
      .into_iter()
      .flat_map(|map| map.keys())
      .filter(|key| {
        key.starts_with("challenge.") || key.starts_with("pow.") || key.starts_with("common.")
      })
    {
      client_i18n.insert(
        key.clone(),
        self.translations.translate(locale, key, &BTreeMap::new())?,
      );
    }
    let html = self
      .templates
      .get_template(name)?
      .render(context! {
        rilgate => &public.rilgate, page => &public.page, challenge => &public.challenge,
        locale => locale, client_i18n => client_i18n,
      })
      .with_context(|| format!("Rendering template {name} for locale {locale}"))?;
    Ok(html)
  }
}
