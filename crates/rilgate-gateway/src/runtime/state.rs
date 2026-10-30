use crate::proxy::client::ProxyService;
use rilgate_challenge::ChallengeService;
use rilgate_config::Config;
use std::sync::Arc;
#[derive(Clone)]
pub struct AppState {
  pub randomx_fallback_active: Arc<std::sync::atomic::AtomicBool>,
  pub cfg: Config,
  pub metrics: Arc<crate::observability::metrics::Metrics>,
  pub connections: Arc<tokio::sync::Semaphore>,
  pub requests: Arc<tokio::sync::Semaphore>,
  pub probes: Arc<tokio::sync::Semaphore>,
  pub pow: Arc<ChallengeService>,
  pub pool: Arc<rilgate_pool::StratumPoolProvider>,
  pub shutdown: tokio_util::sync::CancellationToken,
  pub limiter: Arc<crate::transport::rate_limit::RateLimiter>,
  pub open_graph: Arc<crate::open_graph::OpenGraphService>,
  pub policy_services: Arc<arc_swap::ArcSwap<super::policy::PolicyServices>>,
  pub proxy: Arc<ProxyService>,
  pub web: Arc<rilgate_web::WebManager>,
  pub web_runtime: Arc<rilgate_web::runtime::watcher::RuntimeHandle>,
}
