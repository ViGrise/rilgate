//! Applies policy changes from the configuration file while the gateway runs.

use anyhow::Result;
use rilgate_config::{Config, Configuration, Overrides};
use rilgate_gateway::runtime::policy::PolicyHandle;
use rilgate_ip_feeds::Refreshers;

const HOST_LOAD_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// Starts the policy refreshers and the host load sampler, then reloads the policy on SIGHUP.
/// The policy section is the only part of the configuration that changes at runtime. A
/// configuration whose other sections differ from the running one is left unapplied.
pub fn spawn_reloader(handle: PolicyHandle, running: Config, overrides: Overrides) -> Result<()> {
  let mut reloader = Reloader {
    handle,
    running,
    overrides,
    refreshers: Refreshers::default(),
    sampler: None,
  };
  reloader.start_background_tasks()?;
  #[cfg(unix)]
  let mut hangups = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
  tokio::spawn(async move {
    #[cfg(unix)]
    while hangups.recv().await.is_some() {
      if let Err(error) = reloader.reload() {
        tracing::error!("Configuration reload failed, keeping the running policy: {error:#}");
      }
    }
    #[cfg(not(unix))]
    std::future::pending::<()>().await;
  });
  Ok(())
}

struct Reloader {
  handle: PolicyHandle,
  running: Config,
  overrides: Overrides,
  refreshers: Refreshers,
  sampler: Option<tokio::task::JoinHandle<()>>,
}

impl Reloader {
  fn reload(&mut self) -> Result<()> {
    let config = rilgate_config::loader::load(&self.overrides)?
      .document
      .resolve()?;
    config.validate()?;
    let sections = changed_sections(&self.running, &config);
    if !sections.is_empty() {
      tracing::warn!(
        sections = sections.join(", "),
        "Configuration change needs a restart, keeping the running configuration"
      );
      return Ok(());
    }
    if format!("{:?}", self.running.pow.policy) == format!("{:?}", config.pow.policy) {
      tracing::info!("Policy is unchanged");
      return Ok(());
    }
    self.handle.replace(&config.pow.policy)?;
    self.running = config;
    self.start_background_tasks()?;
    tracing::info!("Policy reloaded");
    Ok(())
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
