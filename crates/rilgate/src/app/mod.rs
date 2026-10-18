mod bootstrap;
use rilgate_config::Config;
pub async fn build(config: Config) -> anyhow::Result<rilgate_gateway::transport::server::Gateway> {
  config.validate()?;
  let (challenge, pool) = bootstrap::services(&config)?;
  let refreshers = rilgate_ip_feeds::spawn(challenge.ip_sets())?;
  if refreshers > 0 {
    tracing::info!(refreshers, "remote IP set refreshers started");
  }
  rilgate_gateway::app::build(config, challenge, pool).await
}

pub fn validate(config: &Config) -> anyhow::Result<()> {
  config.validate()?;
  bootstrap::services(config)?;
  Ok(())
}
