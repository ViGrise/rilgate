mod common;

use async_trait::async_trait;
use http::{HeaderMap, Method, StatusCode};
use rilgate_config::schema::policy::{Action, Condition, IpMatch, Rule};
use rilgate_dns::{DnsLookups, LookupError, Resolver};
use rilgate_gateway::{
  proxy::{client::ProxyService, gate::gate_with_client},
  runtime::state::AppState,
  transport::rate_limit::RateLimiter,
};
use rilgate_test_utils::gateway;
use std::{
  net::{IpAddr, Ipv4Addr},
  sync::Arc,
};

/// Gives two addresses the PTR name of a crawler, but only the crawler address
/// is among the forward records of that name.
struct CrawlerResolver;

const CRAWLER: &str = "66.249.66.1";
const IMPOSTOR: &str = "203.0.113.9";
const UNREACHABLE: &str = "198.51.100.7";

#[async_trait]
impl Resolver for CrawlerResolver {
  async fn ptr(&self, ip: IpAddr) -> Result<Vec<String>, LookupError> {
    match ip.to_string().as_str() {
      CRAWLER | IMPOSTOR => Ok(vec!["crawl.googlebot.com".into()]),
      _ => Err(LookupError::Failed),
    }
  }
  async fn addresses(&self, _: &str) -> Result<Vec<IpAddr>, LookupError> {
    Ok(vec![CRAWLER.parse().unwrap()])
  }
  async fn ipv4(&self, _: &str) -> Result<Vec<Ipv4Addr>, LookupError> {
    Err(LookupError::NotFound)
  }
}

#[tokio::test]
async fn gate_allows_only_forward_confirmed_crawlers() {
  let mut cfg = common::config();
  {
    let config = Arc::get_mut(&mut cfg).unwrap();
    config.pow.policy.policy.default_action = Some(Action::Deny);
    config.pow.policy.policy.rules = Some(vec![Rule {
      name: "crawler".into(),
      when: Condition {
        client_ip: Some(IpMatch {
          reverse_dns: Some(vec!["googlebot.com".into()]),
          ..Default::default()
        }),
        ..Default::default()
      },
      action: Action::Allow,
      score: None,
      challenge: None,
    }]);
  }
  let (service, pool) = gateway::services(&cfg).unwrap();
  let web = rilgate_web::WebManager::new(cfg.web.clone()).unwrap();
  let state = AppState {
    randomx_fallback_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    connections: Arc::new(tokio::sync::Semaphore::new(cfg.server.max_connections)),
    requests: Arc::new(tokio::sync::Semaphore::new(cfg.server.max_requests)),
    probes: Arc::new(tokio::sync::Semaphore::new(1)),
    metrics: Arc::new(Default::default()),
    open_graph: Arc::new(
      rilgate_gateway::open_graph::OpenGraphService::new(
        cfg.open_graph.clone(),
        &service.sets(),
        &service.geoip(),
      )
      .unwrap(),
    ),
    policy_services: rilgate_gateway::runtime::policy::PolicyServices {
      dns: Some(DnsLookups::new(
        &Default::default(),
        Arc::new(CrawlerResolver),
      )),
      honeypot: None,
      counters: Default::default(),
    }
    .into_shared(),
    proxy: Arc::new(ProxyService::new(cfg.upstream.clone()).unwrap()),
    limiter: Arc::new(RateLimiter::new(cfg.pow.rate_limit.clone()).unwrap()),
    web_runtime: Arc::new(rilgate_web::runtime::watcher::runtime(&web)),
    web,
    cfg: cfg.clone(),
    pow: service,
    pool,
    shutdown: tokio_util::sync::CancellationToken::new(),
  };
  assert!(!state.pow.lookup_plan().is_empty());

  let mut headers = HeaderMap::new();
  headers.insert("host", "example.test".parse().unwrap());
  let uri = "/protected".parse().unwrap();
  let mut status = Vec::new();
  for ip in [CRAWLER, IMPOSTOR, UNREACHABLE] {
    let outcome = gate_with_client(
      &state,
      &headers,
      &uri,
      "/protected",
      &Method::GET,
      Some(ip.parse().unwrap()),
      false,
      "",
    )
    .await
    .unwrap();
    status.push(outcome.map(|response| response.status()));
  }
  assert_eq!(
    status,
    [
      None,
      Some(StatusCode::FORBIDDEN),
      Some(StatusCode::FORBIDDEN)
    ]
  );
}
