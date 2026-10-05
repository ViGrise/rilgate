use anyhow::Result;
use rilgate_challenge::ChallengeService;
use rilgate_config::Config;
use rilgate_pool::P2PoolProvider;
use rilgate_pow::AlgorithmRegistry;
use rilgate_pow_hash::{FastAlgorithm, FastConfig};
use rilgate_pow_randomx::{RandomXAlgorithm, RandomXConfig};
use std::sync::Arc;
pub fn services(config: &Config) -> Result<(Arc<ChallengeService>, Arc<P2PoolProvider>)> {
  let cfg = &config.pow;
  let pool = Arc::new(P2PoolProvider::new(cfg.p2pool.clone())?);
  let mut registry = AlgorithmRegistry::default();
  registry.register(Arc::new(FastAlgorithm::new(&FastConfig {
    ttl: cfg.challenge_ttl_seconds,
  })))?;
  registry.register(Arc::new(RandomXAlgorithm::new(
    &RandomXConfig {
      difficulty: cfg.difficulty.clone(),
      ttl: cfg.randomx_challenge_ttl_seconds,
      nonce_offset: cfg.nonce_offset,
      queue_capacity: cfg.verifier_queue_capacity,
    },
    pool.clone(),
  )))?;
  Ok((
    Arc::new(ChallengeService::with_registry(
      Arc::new(cfg.challenge()),
      registry,
    )?),
    pool,
  ))
}
pub fn service(config: &Config) -> Result<ChallengeService> {
  let (service, _) = services(config)?;
  Ok(
    Arc::try_unwrap(service)
      .ok()
      .expect("Service has a single owner"),
  )
}
pub async fn router(config: Config) -> Result<axum::Router> {
  config.validate()?;
  let (service, pool) = services(&config)?;
  rilgate_gateway::app::router(config, service, pool).await
}
