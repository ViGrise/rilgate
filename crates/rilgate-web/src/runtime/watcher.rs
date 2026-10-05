use crate::{resources::loader, runtime::manager::WebManager};
use anyhow::{Context, Result};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{path::Path, sync::Arc};
use tokio::sync::mpsc;

pub struct WatchHandle {
  _watcher: RecommendedWatcher,
  manager: std::sync::Weak<WebManager>,
  task: tokio::task::JoinHandle<()>,
}
impl Drop for WatchHandle {
  fn drop(&mut self) {
    self.task.abort();
    if let Some(manager) = self.manager.upgrade() {
      manager.mark_watching(false);
    }
  }
}
fn relevant(root: &Path, path: &Path) -> bool {
  let Ok(relative) = path.strip_prefix(root) else {
    return false;
  };
  if relative.as_os_str().is_empty() {
    return true;
  }
  !loader::ignored(relative)
    && relative.components().next().is_some_and(|c| {
      matches!(
        c.as_os_str().to_str(),
        Some("templates" | "i18n" | "static")
      )
    })
}
pub fn start(manager: &Arc<WebManager>) -> Result<Option<WatchHandle>> {
  if !manager.config().watch.enabled {
    return Ok(None);
  }
  let Some(root) = &manager.config().root else {
    tracing::warn!("Web watch requested without web.root");
    return Ok(None);
  };
  let root = loader::canonical_root(root)?;
  let (tx, mut rx) = mpsc::channel(1);
  let watched = root.clone();
  let mut watcher =
    notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
      Ok(event)
        if !event.kind.is_access() && event.paths.iter().any(|path| relevant(&watched, path)) =>
      {
        tracing::debug!(paths = ?event.paths, "Web directory change detected");
        let _ = tx.try_send(());
      }
      Err(error) => tracing::warn!(%error, "Web watcher error"),
      _ => {}
    })?;
  watcher
    .watch(&root, RecursiveMode::Recursive)
    .context("Watching web root")?;
  let weak = Arc::downgrade(manager);
  let debounce = manager.config().watch.debounce;
  let task = tokio::spawn(async move {
    while rx.recv().await.is_some() {
      loop {
        match tokio::time::timeout(debounce, rx.recv()).await {
          Ok(Some(())) => continue,
          Ok(None) => return,
          Err(_) => break,
        }
      }
      let Some(manager) = weak.upgrade() else {
        break;
      };
      let _ = tokio::task::spawn_blocking(move || manager.reload()).await;
    }
  });
  manager.mark_watching(true);
  Ok(Some(WatchHandle {
    _watcher: watcher,
    manager: Arc::downgrade(manager),
    task,
  }))
}

pub struct RuntimeHandle {
  pub watch: Option<WatchHandle>,
  signal: Option<tokio::task::JoinHandle<()>>,
}
impl Drop for RuntimeHandle {
  fn drop(&mut self) {
    if let Some(task) = &self.signal {
      task.abort();
    }
  }
}
pub fn runtime(manager: &Arc<WebManager>) -> RuntimeHandle {
  let watch = start(manager).unwrap_or_else(|error| {
    tracing::warn!(%error, "Web watcher unavailable; manual reload remains available");
    None
  });
  #[cfg(unix)]
  let signal = {
    let weak = Arc::downgrade(manager);
    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup()) {
      Ok(mut signal) => Some(tokio::spawn(async move {
        while signal.recv().await.is_some() {
          let Some(manager) = weak.upgrade() else {
            break;
          };
          let _ = tokio::task::spawn_blocking(move || manager.reload()).await;
        }
      })),
      Err(error) => {
        tracing::warn!(%error, "SIGHUP web reload unavailable");
        None
      }
    }
  };
  #[cfg(not(unix))]
  let signal = None;
  RuntimeHandle { watch, signal }
}
