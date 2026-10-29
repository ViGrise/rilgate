//! Applies policy changes from the configuration file while the gateway runs.

use anyhow::Result;
use rilgate_config::{Config, Configuration, Overrides};
use rilgate_gateway::runtime::policy::PolicyHandle;
use rilgate_ip_feeds::Refreshers;

const HOST_LOAD_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

pub use rilgate_gateway::runtime::reload::ReloadOutcome;
use tokio::sync::{mpsc, oneshot};

struct ReloadRequest {
  reply: Option<oneshot::Sender<Result<ReloadOutcome>>>,
}

#[derive(Clone)]
pub struct ReloadTrigger(mpsc::UnboundedSender<ReloadRequest>);

impl ReloadTrigger {
  pub async fn reload(&self) -> Result<ReloadOutcome> {
    let (reply, result) = oneshot::channel();
    self
      .0
      .send(ReloadRequest { reply: Some(reply) })
      .map_err(|_| anyhow::anyhow!("Reload task stopped"))?;
    result
      .await
      .map_err(|_| anyhow::anyhow!("Reload task stopped"))?
  }

  pub fn request(&self) {
    let _ = self.0.send(ReloadRequest { reply: None });
  }
}

#[async_trait::async_trait]
impl rilgate_gateway::runtime::reload::ReloadControl for ReloadTrigger {
  async fn reload(&self) -> Result<ReloadOutcome> {
    self.reload().await
  }
}

/// Starts background policy services and serializes configuration reload requests.
pub fn spawn_reloader(
  handle: PolicyHandle,
  running: Config,
  overrides: Overrides,
  files: Vec<std::path::PathBuf>,
) -> Result<ReloadTrigger> {
  let (sender, mut requests) = mpsc::unbounded_channel::<ReloadRequest>();
  let trigger = ReloadTrigger(sender);
  let watcher = if running.server.watch_config {
    ConfigWatcher::start(files, trigger.clone())?
  } else {
    None
  };
  let mut reloader = Reloader {
    handle,
    running,
    overrides,
    refreshers: Refreshers::default(),
    sampler: None,
    watcher,
  };
  reloader.start_background_tasks()?;
  #[cfg(unix)]
  {
    let mut hangups = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let trigger = trigger.clone();
    tokio::spawn(async move {
      while hangups.recv().await.is_some() {
        trigger.request();
      }
    });
  }
  tokio::spawn(async move {
    while let Some(request) = requests.recv().await {
      let outcome = reloader.reload();
      if let Err(error) = &outcome {
        tracing::error!("Configuration reload failed, keeping the running policy: {error:#}");
      }
      if let Some(reply) = request.reply {
        let _ = reply.send(outcome);
      }
    }
  });
  Ok(trigger)
}

struct Reloader {
  handle: PolicyHandle,
  running: Config,
  overrides: Overrides,
  refreshers: Refreshers,
  sampler: Option<tokio::task::JoinHandle<()>>,
  watcher: Option<ConfigWatcher>,
}

impl Reloader {
  fn reload(&mut self) -> Result<ReloadOutcome> {
    let loaded = rilgate_config::loader::load(&self.overrides)?;
    // A watch failure leaves the earlier watch set in place and never blocks the reload.
    if let Some(watcher) = &mut self.watcher
      && let Err(error) = watcher.update(loaded.files)
    {
      tracing::warn!("Configuration watch update failed: {error:#}");
    }
    let config = loaded.document.resolve()?;
    config.validate()?;
    let sections = changed_sections(&self.running, &config);
    if !sections.is_empty() {
      tracing::warn!(
        sections = sections.join(", "),
        "Configuration change needs a restart, keeping the running configuration"
      );
      return Ok(ReloadOutcome::RestartRequired { sections });
    }
    if format!("{:?}", self.running.pow.policy) == format!("{:?}", config.pow.policy) {
      tracing::info!("Policy is unchanged");
      return Ok(ReloadOutcome::Unchanged);
    }
    self.handle.replace(&config.pow.policy)?;
    self.running = config;
    self.start_background_tasks()?;
    tracing::info!("Policy reloaded");
    Ok(ReloadOutcome::Reloaded)
  }

  /// Starts refreshers for the installed policy's remote sets and databases. The previous
  /// refreshers stop once the new ones run.
  fn start_background_tasks(&mut self) -> Result<()> {
    let challenge = self.handle.challenge();
    let mut refreshers = rilgate_ip_feeds::spawn(&challenge.ip_sets())?;
    let ip_sets = refreshers.len();
    refreshers.absorb(rilgate_ip_feeds::spawn_geoip(&challenge.geoip())?);
    let geoip_databases = refreshers.len() - ip_sets;
    if ip_sets > 0 {
      tracing::info!(refreshers = ip_sets, "remote IP set refreshers started");
    }
    if geoip_databases > 0 {
      tracing::info!(geoip_databases, "GeoIP database refreshers started");
    }
    self.refreshers = refreshers;
    if self.sampler.is_none() && (challenge.uses_load() || self.handle.open_graph_uses_load()) {
      self.sampler = Some(tokio::spawn(sample_host_load()));
    }
    Ok(())
  }
}

/// Names the configuration sections other than the policy that differ between two loads.
pub fn changed_sections(running: &Configuration, loaded: &Configuration) -> Vec<&'static str> {
  fn keys(config: &Configuration) -> Option<Vec<String>> {
    if config.secret_generated {
      return None;
    }
    let pow = &config.pow;
    let rotations = [&pow.signing_keys, &pow.auth_signing_keys];
    Some(
      [&pow.signing_key, &pow.auth_signing_key]
        .into_iter()
        .map(|key| key.expose().to_string())
        .chain(rotations.into_iter().flat_map(|rotation| {
          rotation
            .previous
            .iter()
            .map(|(id, key)| format!("{id}:{}", key.expose()))
        }))
        .collect(),
    )
  }
  fn pow_without_policy(config: &Configuration) -> String {
    let mut pow = config.pow.clone();
    pow.policy = Default::default();
    format!("{pow:?}")
  }
  let sections: [(&'static str, bool); 5] = [
    (
      "server",
      format!(
        "{:?}",
        (
          &running.env,
          &running.listen_addresses,
          &running.operations_listen_addresses,
          running.app_port,
          &running.server,
          &running.upstream,
        )
      ) != format!(
        "{:?}",
        (
          &loaded.env,
          &loaded.listen_addresses,
          &loaded.operations_listen_addresses,
          loaded.app_port,
          &loaded.server,
          &loaded.upstream,
        )
      ),
    ),
    (
      "security",
      running.secret_generated != loaded.secret_generated || keys(running) != keys(loaded),
    ),
    (
      "pow",
      pow_without_policy(running) != pow_without_policy(loaded),
    ),
    (
      "site",
      format!("{:?}", running.web) != format!("{:?}", loaded.web),
    ),
    (
      "open_graph",
      format!("{:?}", running.open_graph) != format!("{:?}", loaded.open_graph),
    ),
  ];
  sections
    .into_iter()
    .filter_map(|(name, changed)| changed.then_some(name))
    .collect()
}

async fn sample_host_load() {
  let mut interval = tokio::time::interval(HOST_LOAD_INTERVAL);
  let mut warned = false;
  loop {
    interval.tick().await;
    match rilgate_policy::read_host_load() {
      Some(averages) => rilgate_policy::HOST_LOAD.store(averages),
      None if !warned => {
        warned = true;
        tracing::warn!("host load average is unavailable, load conditions stay unknown");
      }
      None => {}
    }
  }
}

#[derive(Default)]
struct WatchedPaths {
  files: Vec<std::path::PathBuf>,
  links: Vec<(std::path::PathBuf, Option<std::path::PathBuf>)>,
}

struct ConfigWatcher {
  watcher: notify::RecommendedWatcher,
  paths: std::sync::Arc<std::sync::RwLock<WatchedPaths>>,
  directories: std::collections::HashSet<std::path::PathBuf>,
  task: tokio::task::JoinHandle<()>,
}

impl ConfigWatcher {
  fn start(files: Vec<std::path::PathBuf>, trigger: ReloadTrigger) -> Result<Option<Self>> {
    if files.is_empty() {
      tracing::warn!(
        "Configuration watch requested without a configuration file; there is nothing to watch"
      );
      return Ok(None);
    }
    let paths = std::sync::Arc::new(std::sync::RwLock::new(WatchedPaths::default()));
    let watched = paths.clone();
    let (events, mut receiver) = mpsc::channel(1);
    let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
      match event {
        Ok(event) if !event.kind.is_access() => {
          let mut paths = watched.write().unwrap();
          let direct = event.paths.iter().any(|path| paths.files.contains(path));
          // Symlink target changes identify recorded files replaced through a directory link.
          let mut replaced = false;
          for (file, target) in &mut paths.links {
            let current = file.canonicalize().ok();
            if current != *target {
              *target = current;
              replaced = true;
            }
          }
          if direct || replaced {
            let _ = events.try_send(());
          }
        }
        Err(error) => tracing::warn!(%error, "Configuration watcher error"),
        _ => {}
      }
    })?;
    let task = tokio::spawn(async move {
      while receiver.recv().await.is_some() {
        loop {
          match tokio::time::timeout(std::time::Duration::from_millis(500), receiver.recv()).await {
            Ok(Some(())) => continue,
            Ok(None) => return,
            Err(_) => break,
          }
        }
        trigger.request();
      }
    });
    let mut watch = Self {
      watcher,
      paths,
      directories: Default::default(),
      task,
    };
    watch.update(files)?;
    Ok(Some(watch))
  }

  fn update(&mut self, files: Vec<std::path::PathBuf>) -> Result<()> {
    use notify::Watcher;
    let directories: std::collections::HashSet<_> = files
      .iter()
      .filter_map(|path| path.parent().map(std::path::Path::to_path_buf))
      .collect();
    for directory in directories.difference(&self.directories) {
      self
        .watcher
        .watch(directory, notify::RecursiveMode::NonRecursive)?;
    }
    for directory in self.directories.difference(&directories) {
      self.watcher.unwatch(directory)?;
    }
    self.directories = directories;
    let links = files
      .iter()
      .filter(|file| file.is_symlink())
      .map(|file| (file.clone(), file.canonicalize().ok()))
      .collect();
    *self.paths.write().unwrap() = WatchedPaths { files, links };
    Ok(())
  }
}

impl Drop for ConfigWatcher {
  fn drop(&mut self) {
    self.task.abort();
  }
}
