use anyhow::Result;
use rilgate_challenge::ChallengeService;
use rilgate_config::Config;
use rilgate_pool::StratumPoolProvider;
use rilgate_pow::AlgorithmRegistry;
use rilgate_pow_hash::{FastAlgorithm, FastConfig};
use rilgate_pow_randomx::{RandomXAlgorithm, RandomXConfig};
use std::sync::Arc;
pub fn services(config: &Config) -> Result<(Arc<ChallengeService>, Arc<StratumPoolProvider>)> {
  let cfg = &config.pow;
  let pool = Arc::new(StratumPoolProvider::new(cfg.stratum_pool.clone())?);
  let mut registry = AlgorithmRegistry::default();
  registry.register(Arc::new(FastAlgorithm::new(&FastConfig {
    ttl: cfg.challenge_ttl_seconds,
  })))?;
  registry.register(Arc::new(rilgate_pow::RefreshAlgorithm::new(
    cfg.challenge_ttl_seconds,
  )))?;
  if cfg.hashwx_enabled {
    registry.register(Arc::new(rilgate_pow_hashwx::HashWxAlgorithm::new(
      &cfg.hashwx,
    )?))?;
  }
  if cfg.randomx_enabled {
    registry.register(Arc::new(RandomXAlgorithm::with_order(
      &RandomXConfig {
        difficulty: cfg.difficulty.clone(),
        ttl: cfg.randomx_challenge_ttl_seconds,
        solver_module_path: cfg.browser.solver_module_path.clone(),
        max_workers: cfg.browser.max_workers,
        store_cache: cfg.browser.store_cache,
        job_details: cfg.browser.job_details,
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
pub fn service(config: &Config) -> Result<ChallengeService> {
  let (service, _) = services(config)?;
  Ok(
    Arc::try_unwrap(service)
      .ok()
      .expect("Service has a single owner"),
  )
}
pub async fn build(config: Config) -> Result<rilgate_gateway::transport::server::Gateway> {
  config.validate()?;
  let (service, pool) = services(&config)?;
  rilgate_gateway::app::build(config, service, pool).await
}
pub async fn router(config: Config) -> Result<TestGateway> {
  TestGateway::start(build(config).await?).await
}
#[derive(Clone)]
pub struct TestGateway(Arc<RunningGateway>);
struct RunningGateway {
  address: std::net::SocketAddr,
  operations_address: std::net::SocketAddr,
  stop: tokio_util::sync::CancellationToken,
}
impl Drop for RunningGateway {
  fn drop(&mut self) {
    self.stop.cancel();
  }
}
impl TestGateway {
  pub async fn start(gateway: rilgate_gateway::transport::server::Gateway) -> Result<Self> {
    let listener = tokio::net::TcpListener::bind("[::]:0").await?;
    let address = std::net::SocketAddr::new(
      std::net::Ipv6Addr::LOCALHOST.into(),
      listener.local_addr()?.port(),
    );
    let operations_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let operations_address = operations_listener.local_addr()?;
    let stop = tokio_util::sync::CancellationToken::new();
    let stopped = stop.clone();
    tokio::spawn(async move {
      gateway
        .serve(
          vec![listener],
          vec![operations_listener],
          stopped.cancelled(),
        )
        .await
        .expect("test listener");
    });
    Ok(Self(Arc::new(RunningGateway {
      address,
      operations_address,
      stop,
    })))
  }
  pub fn operations_address(&self) -> std::net::SocketAddr {
    self.0.operations_address
  }
  pub fn address(&self) -> std::net::SocketAddr {
    self.0.address
  }
}
impl tower::Service<http::Request<axum::body::Body>> for TestGateway {
  type Response = http::Response<axum::body::Body>;
  type Error = anyhow::Error;
  type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Response>> + Send>>;
  fn poll_ready(&mut self, _: &mut std::task::Context<'_>) -> std::task::Poll<Result<()>> {
    std::task::Poll::Ready(Ok(()))
  }
  fn call(&mut self, mut request: http::Request<axum::body::Body>) -> Self::Future {
    let gateway = self.clone();
    Box::pin(async move {
      let source = request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|peer| peer.0.ip());
      let target = std::net::SocketAddr::new(
        if source.is_some_and(|ip| ip.is_ipv4()) {
          std::net::Ipv4Addr::LOCALHOST.into()
        } else {
          std::net::Ipv6Addr::LOCALHOST.into()
        },
        gateway.address().port(),
      );
      let socket = if target.is_ipv4() {
        tokio::net::TcpSocket::new_v4()?
      } else {
        tokio::net::TcpSocket::new_v6()?
      };
      if let Some(ip) = source {
        socket.bind(std::net::SocketAddr::new(ip, 0))?;
      }
      let stream = socket.connect(target).await?;
      stream.set_nodelay(true)?;
      if !request.headers().contains_key(http::header::HOST) {
        let host = request
          .uri()
          .authority()
          .map(|a| a.as_str())
          .unwrap_or("example.test")
          .to_owned();
        request
          .headers_mut()
          .insert(http::header::HOST, host.parse()?);
      }
      use axum::body::HttpBody;
      let exact = request.body().size_hint().exact();
      if !request.headers().contains_key(http::header::CONTENT_LENGTH)
        && !request
          .headers()
          .contains_key(http::header::TRANSFER_ENCODING)
      {
        if let Some(size) = exact {
          request
            .headers_mut()
            .insert(http::header::CONTENT_LENGTH, size.into());
        } else if request.version() != http::Version::HTTP_2 {
          request
            .headers_mut()
            .insert(http::header::TRANSFER_ENCODING, "chunked".parse()?);
        }
      }
      let response = if request.version() == http::Version::HTTP_2 {
        let authority = request.headers()[http::header::HOST].to_str()?.to_owned();
        let uri = format!(
          "http://{authority}{}",
          request
            .uri()
            .path_and_query()
            .map(|v| v.as_str())
            .unwrap_or("/")
        );
        *request.uri_mut() = uri.parse()?;
        let (mut client, connection) = hyper::client::conn::http2::handshake(
          hyper_util::rt::TokioExecutor::new(),
          hyper_util::rt::TokioIo::new(stream),
        )
        .await?;
        tokio::spawn(async move {
          let _ = connection.await;
        });
        client.send_request(request).await?
      } else {
        let path = request
          .uri()
          .path_and_query()
          .map(|value| value.as_str())
          .unwrap_or("/")
          .to_owned();
        *request.uri_mut() = path.parse()?;
        let (mut client, connection) =
          hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(stream)).await?;
        tokio::spawn(async move {
          let _ = connection.await;
        });
        client.send_request(request).await?
      };
      Ok(response.map(|inner| {
        axum::body::Body::new(GuardedBody {
          inner,
          _gateway: gateway,
        })
      }))
    })
  }
}

struct GuardedBody {
  inner: hyper::body::Incoming,
  _gateway: TestGateway,
}
impl http_body::Body for GuardedBody {
  type Data = hyper::body::Bytes;
  type Error = hyper::Error;
  fn poll_frame(
    mut self: std::pin::Pin<&mut Self>,
    cx: &mut std::task::Context<'_>,
  ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
    std::pin::Pin::new(&mut self.inner).poll_frame(cx)
  }
  fn size_hint(&self) -> http_body::SizeHint {
    self.inner.size_hint()
  }
  fn is_end_stream(&self) -> bool {
    self.inner.is_end_stream()
  }
}
