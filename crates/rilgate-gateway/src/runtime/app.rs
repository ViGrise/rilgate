use super::state::AppState;
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
  pool: Arc<rilgate_pool::P2PoolProvider>,
) -> Result<Gateway> {
  cfg.validate()?;
  let proxy = Arc::new(ProxyService::new(cfg.upstream.clone())?);
  let web_config = cfg.web.clone();
  let web = tokio::task::spawn_blocking(move || rilgate_web::WebManager::new(web_config)).await??;
  let web_runtime = Arc::new(rilgate_web::runtime::watcher::runtime(&web));
  let limiter = Arc::new(RateLimiter::new(cfg.pow.rate_limit.clone())?);
  Ok(Gateway::new(AppState {
    cfg,
    pow,
    pool,
    proxy,
    web,
    web_runtime,
    limiter,
    shutdown: tokio_util::sync::CancellationToken::new(),
  }))
}
