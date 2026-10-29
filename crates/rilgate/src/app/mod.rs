mod bootstrap;
use rilgate_config::Config;
pub async fn build(config: Config) -> anyhow::Result<rilgate_gateway::transport::server::Gateway> {
  config.validate()?;
  let (challenge, pool) = bootstrap::services(&config)?;
  let refreshers = rilgate_ip_feeds::spawn(challenge.ip_sets())?;
  if refreshers > 0 {
    tracing::info!(refreshers, "remote IP set refreshers started");
  }
  let open_graph_uses_load = config
    .open_graph
    .when
    .as_ref()
    .is_some_and(rilgate_policy::policy::schema::Condition::uses_load);
  if challenge.uses_load() || open_graph_uses_load {
    tokio::spawn(sample_host_load());
  }
  rilgate_gateway::app::build(config, challenge, pool).await
}

const HOST_LOAD_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

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

pub fn validate(config: &Config) -> anyhow::Result<()> {
  config.validate()?;
  bootstrap::services(config)?;
  Ok(())
}
