use anyhow::Result;
use rilgate_challenge::ChallengeService;
use rilgate_config::Config;
use rilgate_pool::StratumPoolProvider;
use rilgate_pow::AlgorithmRegistry;
use rilgate_pow_hash::{FastAlgorithm, FastConfig};
use rilgate_pow_hashwx::HashWxAlgorithm;
use rilgate_pow_randomx::{RandomXAlgorithm, RandomXConfig};
use std::sync::Arc;
pub(super) fn services(
  config: &Config,
) -> Result<(Arc<ChallengeService>, Arc<StratumPoolProvider>)> {
  let cfg = &config.pow;
  let pool = Arc::new(StratumPoolProvider::new(cfg.stratum_pool.clone())?);
  let mut registry = AlgorithmRegistry::default();
  registry.register(Arc::new(FastAlgorithm::new(&FastConfig {
    ttl: cfg.challenge_ttl_seconds,
  })))?;
  registry.register(Arc::new(HashWxAlgorithm::new(&cfg.hashwx)?))?;
  if cfg.randomx_enabled {
    registry.register(Arc::new(RandomXAlgorithm::with_order(
      &RandomXConfig {
        difficulty: cfg.difficulty.clone(),
        ttl: cfg.randomx_challenge_ttl_seconds,
        nonce_offset: cfg.nonce_offset,
        queue_capacity: cfg.verifier_queue_capacity,
        cache_idle_timeout: cfg.randomx_cache_idle_timeout,
      },
      pool.clone(),
      &cfg.randomx_verify.hashers(),
    )?))?;
  }
  Ok((
    Arc::new(ChallengeService::with_registry(
      Arc::new(cfg.challenge()),
      registry,
    )?),
    pool,
  ))
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;
  #[test]
  fn fast_only_does_not_read_remote_tls_files() {
    let mut config = rilgate_test_utils::config::config();
    let cfg = Arc::get_mut(&mut config).unwrap();
    cfg.pow.randomx_enabled = false;
    cfg.pow.randomx_verify.order = vec![rilgate_config::schema::pow::Hasher::Remote];
    cfg.pow.randomx_verify.remote.endpoint = Some("https://localhost:9443".into());
    cfg.pow.randomx_verify.remote.tls.ca = Some("/nonexistent/ca.pem".into());
    let (service, _) = super::services(&config).unwrap();
    assert!(
      service
        .algorithm_descriptor(rilgate_config::PowAlgorithm::RandomX)
        .is_err()
    );
    assert_eq!(service.algorithm_descriptors().len(), 2);
  }
}
