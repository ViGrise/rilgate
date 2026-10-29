use super::{policy::PolicyServices, state::AppState};
use crate::{
  proxy::client::ProxyService,
  transport::{rate_limit::RateLimiter, server::Gateway},
};
use anyhow::Result;
use rilgate_config::Config;
use std::sync::Arc;

pub async fn build(
  cfg: Config,
  pow: Arc<rilgate_challenge::ChallengeService>,
  pool: Arc<rilgate_pool::StratumPoolProvider>,
) -> Result<Gateway> {
  cfg.validate()?;
  let proxy = Arc::new(ProxyService::new(cfg.upstream.clone())?);
  let web_config = cfg.web.clone();
  let web = tokio::task::spawn_blocking(move || rilgate_web::WebManager::new(web_config)).await??;
  let web_runtime = Arc::new(rilgate_web::runtime::watcher::runtime(&web));
  let limiter = Arc::new(RateLimiter::new(cfg.pow.rate_limit.clone())?);
  let policy_services = Arc::new(arc_swap::ArcSwap::from_pointee(PolicyServices::new(
    &cfg.pow.policy,
    &pow.lookup_plan(),
  )?));
  Ok(Gateway::new(AppState {
    connections: Arc::new(tokio::sync::Semaphore::new(cfg.server.max_connections)),
    requests: Arc::new(tokio::sync::Semaphore::new(cfg.server.max_requests)),
    probes: Arc::new(tokio::sync::Semaphore::new(1)),
    metrics: Arc::new(Default::default()),
    open_graph: Arc::new(crate::open_graph::OpenGraphService::new(
      cfg.open_graph.clone(),
      &pow.ip_sets(),
      &pow.geoip(),
    )?),
    cfg,
    policy_services,
    pow,
    pool,
    proxy,
    web,
    web_runtime,
    limiter,
    shutdown: tokio_util::sync::CancellationToken::new(),
  }))
}
