use http::{HeaderMap, Method, Uri};
use rilgate_crypto::token::{TokenPurpose, sign_json};
use rilgate_gateway::{
  proxy::{client::ProxyService, gate::gate},
  runtime::state::AppState,
  transport::rate_limit::RateLimiter,
};
use rilgate_protocol::{PowAlgorithm, claims::AuthClaims};
use std::{
  alloc::{GlobalAlloc, Layout, System},
  hint::black_box,
  sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
  },
  time::{Instant, SystemTime, UNIX_EPOCH},
};

struct CountingAllocator;
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
  unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
    if COUNTING.load(Ordering::Relaxed) {
      ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    }
    unsafe { System.alloc(layout) }
  }

  unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
    unsafe { System.dealloc(pointer, layout) }
  }

  unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
    if COUNTING.load(Ordering::Relaxed) {
      ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    }
    unsafe { System.realloc(pointer, layout, size) }
  }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
  let iterations = std::env::args()
    .nth(1)
    .map(|value| value.parse::<u32>())
    .transpose()?
    .unwrap_or(200_000);
  anyhow::ensure!(iterations > 0, "iterations must be positive");
  let cfg = rilgate_test_utils::config::config();
  let (pow, pool) = rilgate_test_utils::gateway::services(&cfg)?;
  let web = rilgate_web::WebManager::new(cfg.web.clone())?;
  let state = AppState {
    randomx_fallback_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    metrics: Arc::new(Default::default()),
    connections: Arc::new(tokio::sync::Semaphore::new(cfg.server.max_connections)),
    requests: Arc::new(tokio::sync::Semaphore::new(cfg.server.max_requests)),
    probes: Arc::new(tokio::sync::Semaphore::new(1)),
    open_graph: Arc::new(rilgate_gateway::open_graph::OpenGraphService::new(
      cfg.open_graph.clone(),
      &pow.ip_sets(),
      &pow.geoip(),
    )?),
    policy_services: rilgate_gateway::runtime::policy::PolicyServices {
      dns: None,
      honeypot: None,
      counters: Default::default(),
    }
    .into_shared(),
    proxy: Arc::new(ProxyService::new(cfg.upstream.clone())?),
    limiter: Arc::new(RateLimiter::new(cfg.pow.rate_limit.clone())?),
    web_runtime: Arc::new(rilgate_web::runtime::watcher::runtime(&web)),
    shutdown: tokio_util::sync::CancellationToken::new(),
    cfg: cfg.clone(),
    pow,
    pool,
    web,
  };
  let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
  let token = sign_json(
    &AuthClaims {
      host: "example.test".into(),
      v: 1,
      algorithm: PowAlgorithm::Fast,
      difficulty: 1,
      enforce: cfg.pow.enforce,
      issued_at: now,
      expires_at: now + cfg.pow.auth_ttl_seconds as i64,
      binding: None,
    },
    cfg.pow.auth_signing_key.expose().as_bytes(),
    TokenPurpose::Auth,
  )?;
  let mut headers = HeaderMap::new();
  headers.insert("host", "example.test".parse()?);
  headers.append("cookie", "preference=light; lang=en".parse()?);
  headers.append(
    "cookie",
    format!("{}={token}", cfg.pow.auth_cookie_name).parse()?,
  );
  for path in ["/public", "/protected"] {
    let uri: Uri = path.parse()?;
    for _ in 0..1000 {
      assert!(gate(&state, &headers, &uri, &Method::GET).await?.is_none());
    }
    ALLOCATIONS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    for _ in 0..1000 {
      assert!(gate(&state, &headers, &uri, &Method::GET).await?.is_none());
    }
    COUNTING.store(false, Ordering::Relaxed);
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) as f64 / 1000.0;
    let mut samples = Vec::new();
    for _ in 0..5 {
      let started = Instant::now();
      for _ in 0..iterations {
        assert!(
          gate(
            black_box(&state),
            black_box(&headers),
            black_box(&uri),
            &Method::GET
          )
          .await?
          .is_none()
        );
      }
      samples.push(started.elapsed());
    }
    samples.sort_unstable();
    println!(
      "path={path} iterations={iterations} median_ns={:.2} allocations_per_request={allocations:.1}",
      samples[2].as_secs_f64() * 1e9 / f64::from(iterations)
    );
  }
  Ok(())
}
