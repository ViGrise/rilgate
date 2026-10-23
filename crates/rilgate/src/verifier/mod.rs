use anyhow::{Context, Result};
use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full, Limited};
use hyper::{body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use rilgate_config::assembly::verifier::VerifierConfig;
use rilgate_pow_randomx::{HashError, LocalHasher, RandomXHasher};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use serde::Deserialize;
use std::{
  collections::HashMap,
  convert::Infallible,
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
  },
  time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
  net::TcpListener,
  sync::{OwnedSemaphorePermit, Semaphore},
};
use tokio_rustls::TlsAcceptor;

const OUTCOMES: [&str; 5] = ["hashed", "invalid", "overloaded", "expired", "cancelled"];
struct State {
  hasher: Arc<dyn RandomXHasher>,
  stopping: AtomicBool,
  unhealthy_until: Mutex<Option<Instant>>,
  queue: Arc<Semaphore>,
  per_client: usize,
  clients: Mutex<HashMap<String, usize>>,
  counts: [AtomicU64; 5],
  metrics_enabled: bool,
}
struct Admission {
  state: Arc<State>,
  client: Option<String>,
  _permit: OwnedSemaphorePermit,
  outcome: usize,
}
impl Drop for Admission {
  fn drop(&mut self) {
    if let Some(client) = &self.client {
      let mut clients = self.state.clients.lock().unwrap_or_else(|e| e.into_inner());
      if let Some(count) = clients.get_mut(client) {
        *count -= 1;
        if *count == 0 {
          clients.remove(client);
        }
      }
    }
    self.state.counts[self.outcome].fetch_add(1, Ordering::Relaxed);
  }
}
impl State {
  fn new(hasher: Arc<dyn RandomXHasher>, queue: usize, metrics_enabled: bool) -> Arc<Self> {
    Arc::new(Self {
      hasher,
      stopping: AtomicBool::new(false),
      unhealthy_until: Mutex::new(None),
      queue: Arc::new(Semaphore::new(queue)),
      per_client: (queue / 2).max(1),
      clients: Mutex::new(HashMap::new()),
      counts: std::array::from_fn(|_| AtomicU64::new(0)),
      metrics_enabled,
    })
  }
  /// Admits a request against the shared queue. An authenticated client
  /// certificate also has its own share, so one gateway cannot fill the queue.
  fn admit(self: &Arc<Self>, client: Option<&str>) -> Option<Admission> {
    let permit = self.queue.clone().try_acquire_owned().ok()?;
    if let Some(client) = client {
      let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
      let count = clients.entry(client.into()).or_default();
      if *count >= self.per_client {
        return None;
      }
      *count += 1;
    }
    Some(Admission {
      state: self.clone(),
      client: client.map(Into::into),
      _permit: permit,
      outcome: 4,
    })
  }
  fn mark_unhealthy(&self) {
    // A short retry window permits recovery while readiness probes pause hash traffic.
    *self
      .unhealthy_until
      .lock()
      .unwrap_or_else(|e| e.into_inner()) = Some(Instant::now() + Duration::from_secs(1));
  }
  fn ready(&self) -> Response<Full<Bytes>> {
    response(
      if !self.stopping.load(Ordering::Relaxed)
        && self
          .unhealthy_until
          .lock()
          .unwrap_or_else(|e| e.into_inner())
          .is_none_or(|until| Instant::now() >= until)
      {
        StatusCode::OK
      } else {
        StatusCode::SERVICE_UNAVAILABLE
      },
      "{}",
    )
  }
  async fn handle<B>(
    self: Arc<Self>,
    request: Request<B>,
    client: Option<&str>,
  ) -> Response<Full<Bytes>>
  where
    B: hyper::body::Body<Data = Bytes> + Send,
    B::Error: std::error::Error + Send + Sync + 'static,
  {
    if request.method() == http::Method::GET && request.uri().path() == "/v1/ready" {
      return self.ready();
    }
    if request.method() != http::Method::POST || request.uri().path() != "/v1/randomx/hash" {
      return response(StatusCode::NOT_FOUND, "{}");
    }
    if self.stopping.load(Ordering::Relaxed) {
      return response(StatusCode::SERVICE_UNAVAILABLE, "{}");
    }
    let Some(mut admission) = self.admit(client) else {
      self.counts[2].fetch_add(1, Ordering::Relaxed);
      return response(StatusCode::SERVICE_UNAVAILABLE, "{}");
    };
    let json = request
      .headers()
      .get(http::header::CONTENT_TYPE)
      .and_then(|v| v.to_str().ok())
      .is_some_and(|v| {
        v.split(';')
          .next()
          .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"))
      });
    if !json {
      admission.outcome = 1;
      return response(StatusCode::BAD_REQUEST, "{}");
    }
    let body = tokio::time::timeout(
      Duration::from_secs(5),
      Limited::new(request.into_body(), 4096).collect(),
    )
    .await;
    // Only an oversized or malformed request is invalid. A stalled or broken
    // upload is a transport failure, so the gateway can try its next hasher.
    let body = match body {
      Ok(Ok(body)) => body.to_bytes(),
      Ok(Err(error)) if error.is::<http_body_util::LengthLimitError>() => {
        admission.outcome = 1;
        return response(StatusCode::BAD_REQUEST, "{}");
      }
      _ => return response(StatusCode::SERVICE_UNAVAILABLE, "{}"),
    };
    let parsed = serde_json::from_slice::<HashRequest>(&body)
      .ok()
      .and_then(HashRequest::decode);
    let Some((seed, input, wall_deadline)) = parsed else {
      admission.outcome = 1;
      return response(StatusCode::BAD_REQUEST, "{}");
    };
    let remaining = wall_deadline.saturating_sub(unix_ms());
    if remaining == 0 || remaining > 60_000 {
      admission.outcome = 3;
      return response(StatusCode::SERVICE_UNAVAILABLE, r#"{"reason":"deadline"}"#);
    }
    let deadline = Instant::now() + Duration::from_millis(remaining);
    let result =
      tokio::time::timeout_at(deadline.into(), self.hasher.hash(seed, input, deadline)).await;
    match result {
      Ok(Ok(_)) if Instant::now() >= deadline => {
        admission.outcome = 3;
        response(StatusCode::SERVICE_UNAVAILABLE, "{}")
      }
      Ok(Ok(hash)) => {
        admission.outcome = 0;
        *self
          .unhealthy_until
          .lock()
          .unwrap_or_else(|e| e.into_inner()) = None;
        response(
          StatusCode::OK,
          serde_json::json!({"hash":hex::encode(hash)}).to_string(),
        )
      }
      Ok(Err(_)) if Instant::now() >= deadline => {
        admission.outcome = 3;
        response(StatusCode::SERVICE_UNAVAILABLE, "{}")
      }
      Ok(Err(HashError::Invalid(_))) => {
        admission.outcome = 1;
        response(StatusCode::BAD_REQUEST, "{}")
      }
      Ok(Err(HashError::Unavailable(error))) => {
        admission.outcome = 2;
        if !error.is::<rilgate_core::error::CapacityExceeded>() {
          self.mark_unhealthy();
          tracing::error!("RandomX verifier unavailable: {error:#}");
        }
        response(StatusCode::SERVICE_UNAVAILABLE, "{}")
      }
      Err(_) => {
        admission.outcome = 3;
        response(StatusCode::SERVICE_UNAVAILABLE, "{}")
      }
    }
  }
  fn metrics(&self) -> Vec<u8> {
    let mut text = "# TYPE rilgate_verifier_service_requests_total counter\n".to_owned();
    for (index, outcome) in OUTCOMES.iter().enumerate() {
      text.push_str(&format!(
        "rilgate_verifier_service_requests_total{{outcome=\"{outcome}\"}} {}\n",
        self.counts[index].load(Ordering::Relaxed)
      ));
    }
    text.into_bytes()
  }
  fn operations(&self, path: &str) -> Response<Full<Bytes>> {
    match path {
      "/api/live" => response(StatusCode::OK, "{}"),
      "/api/ready" => self.ready(),
      "/api/metrics" if self.metrics_enabled => Response::builder()
        .header("content-type", "text/plain; version=0.0.4")
        .header("cache-control", "no-store")
        .body(Full::new(Bytes::from(self.metrics())))
        .unwrap(),
      _ => response(StatusCode::NOT_FOUND, "{}"),
    }
  }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HashRequest {
  seed: String,
  input: String,
  deadline_unix_ms: u64,
}
impl HashRequest {
  fn decode(self) -> Option<(Vec<u8>, Vec<u8>, u64)> {
    if self.seed.len() != 64 || !(2..=512).contains(&self.input.len()) {
      return None;
    }
    Some((
      hex::decode(self.seed).ok()?,
      hex::decode(self.input).ok()?,
      self.deadline_unix_ms,
    ))
  }
}
fn unix_ms() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_millis()
    .min(u64::MAX as u128) as u64
}
fn response(status: StatusCode, body: impl Into<Bytes>) -> Response<Full<Bytes>> {
  let mut builder = Response::builder()
    .status(status)
    .header("content-type", "application/json")
    .header("cache-control", "no-store");
  if status == StatusCode::SERVICE_UNAVAILABLE {
    builder = builder.header("retry-after", "1");
  }
  builder.body(Full::new(body.into())).unwrap()
}
fn tls(config: &VerifierConfig) -> Result<TlsAcceptor> {
  let tls = &config.serve.tls;
  let cert = std::fs::read(tls.cert.as_ref().context("verifier TLS cert")?)?;
  let certs = CertificateDer::pem_slice_iter(&cert).collect::<Result<Vec<_>, _>>()?;
  let key = PrivateKeyDer::from_pem_file(tls.key.as_ref().context("verifier TLS key")?)?;
  let builder =
    rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
      .with_safe_default_protocol_versions()?;
  let builder = if let Some(ca) = &tls.client_ca {
    let mut roots = rustls::RootCertStore::empty();
    for cert in CertificateDer::pem_file_iter(ca)? {
      roots.add(cert?)?;
    }
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
      Arc::new(roots),
      Arc::new(rustls::crypto::ring::default_provider()),
    )
    .build()?;
    builder.with_client_cert_verifier(verifier)
  } else {
    builder.with_no_client_auth()
  };
  Ok(TlsAcceptor::from(Arc::new(
    builder.with_single_cert(certs, key)?,
  )))
}
pub fn validate(config: &VerifierConfig) -> Result<()> {
  tls(config).map(|_| ())
}

pub async fn run(config: VerifierConfig) -> Result<()> {
  let tls = tls(&config)?;
  let state = State::new(
    Arc::new(LocalHasher::new(config.serve.queue, config.idle_timeout)),
    config.serve.queue,
    config.metrics_enabled,
  );
  let listener = crate::cli::serve::bind_all(&[config.listen])?.remove(0);
  let operations = crate::cli::serve::bind_all(&config.operations)?;
  let connections = Arc::new(Semaphore::new(config.serve.max_connections));
  let mut tasks = tokio::task::JoinSet::new();
  let mut operations_tasks = tokio::task::JoinSet::new();
  let (shutdown, _) = tokio::sync::watch::channel(false);
  for listener in operations {
    tracing::info!("Starting verifier operations on {}", listener.local_addr()?);
    let state = state.clone();
    operations_tasks.spawn(async move { operations_loop(listener, state).await });
  }
  tracing::info!("Starting RandomX verifier on {}", listener.local_addr()?);
  let stopped = crate::shutdown::shutdown_signal();
  tokio::pin!(stopped);
  loop {
    let (stream, _) = tokio::select! { _ = &mut stopped => break, accepted = accept_retry(|| listener.accept()) => accepted };
    let Ok(permit) = connections.clone().try_acquire_owned() else {
      tracing::debug!("Verifier connection capacity reached");
      continue;
    };
    let mut stopping = shutdown.subscribe();
    let tls = tls.clone();
    let state = state.clone();
    tasks.spawn(async move {
      let _permit = permit;
      let Ok(Ok(stream)) = tokio::time::timeout(Duration::from_secs(5), tls.accept(stream)).await
      else {
        return;
      };
      use sha2::{Digest, Sha256};
      let identity = stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|certs| certs.first())
        .map(|cert| hex::encode(Sha256::digest(cert.as_ref())));
      let service = service_fn(move |request: Request<Incoming>| {
        let state = state.clone();
        let identity = identity.clone();
        async move { Ok::<_, Infallible>(state.handle(request, identity.as_deref()).await) }
      });
      let connection = http1::Builder::new()
        .timer(hyper_util::rt::TokioTimer::new())
        .header_read_timeout(Duration::from_secs(5))
        .serve_connection(TokioIo::new(stream), service);
      tokio::pin!(connection);
      tokio::select! {
        _ = &mut connection => {},
        _ = stopping.changed() => {
          connection.as_mut().graceful_shutdown();
          let _ = connection.await;
        }
      }
    });
    while tasks.try_join_next().is_some() {}
  }
  // Closing the listener before the drain makes new connections fail fast.
  drop(listener);
  state.stopping.store(true, Ordering::Relaxed);
  let _ = shutdown.send(true);
  drain(&mut tasks, Duration::from_secs(30)).await;
  operations_tasks.abort_all();
  while operations_tasks.join_next().await.is_some() {}
  Ok(())
}
async fn operations_loop(listener: TcpListener, state: Arc<State>) {
  let connections = Arc::new(Semaphore::new(16));
  let mut tasks = tokio::task::JoinSet::new();
  loop {
    let (stream, _) = accept_retry(|| listener.accept()).await;
    let Ok(permit) = connections.clone().try_acquire_owned() else {
      tracing::debug!("Verifier connection capacity reached");
      continue;
    };
    let state = state.clone();
    tasks.spawn(async move {
      let _permit = permit;
      let service = service_fn(move |request: Request<Incoming>| {
        let response = if request.method() == http::Method::GET {
          state.operations(request.uri().path())
        } else {
          response(StatusCode::NOT_FOUND, "{}")
        };
        async { Ok::<_, Infallible>(response) }
      });
      let _ = http1::Builder::new()
        .timer(hyper_util::rt::TokioTimer::new())
        .header_read_timeout(Duration::from_secs(5))
        .serve_connection(TokioIo::new(stream), service)
        .await;
    });
    while tasks.try_join_next().is_some() {}
  }
}
async fn accept_retry<T, F, Fut>(mut accept: F) -> T
where
  F: FnMut() -> Fut,
  Fut: std::future::Future<Output = std::io::Result<T>>,
{
  loop {
    match accept().await {
      Ok(value) => return value,
      Err(error) => {
        tracing::warn!(%error, "Verifier accept failed; retrying");
        tokio::time::sleep(Duration::from_millis(100)).await;
      }
    }
  }
}
async fn drain(tasks: &mut tokio::task::JoinSet<()>, timeout: Duration) {
  if tokio::time::timeout(timeout, async {
    while tasks.join_next().await.is_some() {}
  })
  .await
  .is_err()
  {
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
  }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_tls;
