mod bootstrap;
use rilgate_config::Config;
pub async fn router(config: Config) -> anyhow::Result<axum::Router> {
  config.validate()?;
  let (challenge, pool) = bootstrap::services(&config)?;
  rilgate_gateway::app::router(config, challenge, pool).await
}
