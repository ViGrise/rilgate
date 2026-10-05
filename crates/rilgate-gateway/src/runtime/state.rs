use crate::proxy::client::ProxyService;
use rilgate_challenge::ChallengeService;
use rilgate_config::Config;
use std::sync::Arc;
#[derive(Clone)]
pub struct AppState {
  pub cfg: Config,
  pub pow: Arc<ChallengeService>,
  pub pool: Arc<rilgate_pool::P2PoolProvider>,
  pub proxy: Arc<ProxyService>,
  pub web: Arc<rilgate_web::WebManager>,
  pub web_runtime: Arc<rilgate_web::runtime::watcher::RuntimeHandle>,
}
