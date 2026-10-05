use crate::{config::WebConfig, resources::seed, runtime::snapshot::WebSnapshot};
use anyhow::Result;
use arc_swap::ArcSwap;
use serde::Serialize;
use std::{
  sync::{Arc, Mutex},
  time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize)]
pub struct WebDiagnostics {
  pub generation: u64,
  pub last_successful_reload: Option<u64>,
  pub last_reload_error: Option<String>,
  pub active_locales: Vec<String>,
  pub template_count: usize,
  pub watch_enabled: bool,
  pub reload_total: u64,
  pub reload_failures_total: u64,
}
pub struct WebManager {
  current: ArcSwap<WebSnapshot>,
  config: WebConfig,
  fallback: Arc<WebSnapshot>,
  reload_lock: Mutex<()>,
  diagnostics: Mutex<WebDiagnostics>,
}
impl WebManager {
  pub fn new(config: WebConfig) -> Result<Arc<Self>> {
    config.validate()?;
    if let Some(root) = &config.root
      && let Err(error) = seed::prepare(root, config.seed.empty_directory)
    {
      tracing::warn!(%error, "Custom web root cannot be initialized");
    }
    let mut fallback_config = config.clone();
    fallback_config.i18n.strict = false;
    let fallback = Arc::new(WebSnapshot::build(&fallback_config, 0, false)?);
    let mut startup_error = None;
    let snapshot = match WebSnapshot::build(&config, 1, true) {
      Ok(snapshot) => snapshot,
      Err(error) => {
        tracing::error!(%error, "Custom web snapshot invalid; loading embedded defaults");
        startup_error = Some(format!("{error:#}"));
        WebSnapshot::build(&config, 1, false)?
      }
    };
    let diagnostics = WebDiagnostics {
      generation: 1,
      last_successful_reload: Some(timestamp()),
      last_reload_error: startup_error,
      active_locales: snapshot
        .config
        .i18n
        .supported_locales
        .iter()
        .filter_map(|tag| crate::i18n::locale::normalize(tag).ok())
        .collect(),
      template_count: snapshot.templates.templates().count(),
      watch_enabled: false,
      reload_total: 0,
      reload_failures_total: 0,
    };
    Ok(Arc::new(Self {
      current: ArcSwap::from_pointee(snapshot),
      config,
      fallback,
      reload_lock: Mutex::new(()),
      diagnostics: Mutex::new(diagnostics),
    }))
  }
  pub fn fallback(&self) -> Arc<WebSnapshot> {
    self.fallback.clone()
  }
  pub fn current(&self) -> Arc<WebSnapshot> {
    self.current.load_full()
  }
  pub fn diagnostics(&self) -> WebDiagnostics {
    self.diagnostics.lock().unwrap().clone()
  }
  pub fn mark_watching(&self, enabled: bool) {
    self.diagnostics.lock().unwrap().watch_enabled = enabled;
  }
  pub fn config(&self) -> &WebConfig {
    &self.config
  }
  pub fn reload(&self) -> Result<()> {
    let _lock = self.reload_lock.lock().unwrap();
    let started = Instant::now();
    let generation = self.current().generation + 1;
    match WebSnapshot::build(&self.config, generation, true) {
      Ok(snapshot) => {
        let mut diagnostics = self.diagnostics.lock().unwrap();
        diagnostics.generation = generation;
        diagnostics.last_successful_reload = Some(timestamp());
        diagnostics.last_reload_error = None;
        diagnostics.active_locales = snapshot
          .config
          .i18n
          .supported_locales
          .iter()
          .filter_map(|tag| crate::i18n::locale::normalize(tag).ok())
          .collect();
        diagnostics.template_count = snapshot.templates.templates().count();
        diagnostics.reload_total += 1;
        self.current.store(Arc::new(snapshot));
        tracing::info!(
          generation,
          templates = diagnostics.template_count,
          locales = diagnostics.active_locales.len(),
          duration_ms = started.elapsed().as_millis(),
          "Web snapshot reloaded"
        );
        Ok(())
      }
      Err(error) => {
        let mut diagnostics = self.diagnostics.lock().unwrap();
        diagnostics.last_reload_error = Some(format!("{error:#}"));
        diagnostics.reload_failures_total += 1;
        tracing::error!(error = %format!("{error:#}"), generation_kept = generation - 1, "Web snapshot reload failed");
        Err(error)
      }
    }
  }
}
fn timestamp() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs()
}
