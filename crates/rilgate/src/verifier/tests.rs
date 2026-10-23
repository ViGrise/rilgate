use super::*;
use futures_util::future::BoxFuture;
use rilgate_pow_randomx::HasherKind;
pub(super) struct Fake;
impl RandomXHasher for Fake {
  fn kind(&self) -> HasherKind {
    HasherKind::Local
  }
  fn hash(&self, _: Vec<u8>, _: Vec<u8>, _: Instant) -> BoxFuture<'_, Result<[u8; 32], HashError>> {
    Box::pin(async { Ok([7; 32]) })
  }
}
fn state() -> Arc<State> {
  State::new(Arc::new(Fake), 4, true)
}
#[tokio::test]
async fn protocol_validates_dimensions_deadlines_and_body_limits() {
  let state = state();
  for (seed, input, deadline, expected) in [
    ("ab".repeat(32), "01".into(), unix_ms() + 1000, 200),
    ("ab".repeat(31), "01".into(), unix_ms() + 1000, 400),
    ("ab".repeat(32), "".into(), unix_ms() + 1000, 400),
    ("ab".repeat(32), "01".repeat(257), unix_ms() + 1000, 400),
    ("ab".repeat(32), "01".into(), 1, 503),
    ("ab".repeat(32), "01".into(), unix_ms() + 61000, 503),
  ] {
    let body = serde_json::to_vec(
      &serde_json::json!({"seed":seed,"input":input,"deadline_unix_ms":deadline}),
    )
    .unwrap();
    let response = state
      .clone()
      .handle(
        Request::post("/v1/randomx/hash")
          .header("content-type", "application/json")
          .body(Full::new(Bytes::from(body)))
          .unwrap(),
        Some("client"),
      )
      .await;
    assert_eq!(response.status().as_u16(), expected);
  }
  let response = state
    .clone()
    .handle(
      Request::post("/v1/randomx/hash")
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(vec![0; 4097])))
        .unwrap(),
      Some("client"),
    )
    .await;
  assert_eq!(response.status(), StatusCode::BAD_REQUEST);
  assert_eq!(state.counts[0].load(Ordering::Relaxed), 1);
}
#[test]
fn admission_is_global_and_per_certificate() {
  let state = state();
  let first = state.admit(Some("a")).unwrap();
  let second = state.admit(Some("a")).unwrap();
  assert!(state.admit(Some("a")).is_none());
  let third = state.admit(Some("b")).unwrap();
  let fourth = state.admit(Some("b")).unwrap();
  assert!(state.admit(Some("c")).is_none());
  drop((first, second, third, fourth));
  assert!(state.clients.lock().unwrap().is_empty());
  assert!(state.admit(Some("a")).is_some());
}
#[test]
fn anonymous_clients_share_only_the_global_queue() {
  let state = state();
  let admitted: Vec<_> = (0..4).map(|_| state.admit(None).unwrap()).collect();
  assert!(state.admit(None).is_none());
  assert!(state.clients.lock().unwrap().is_empty());
  drop(admitted);
}
#[tokio::test]
async fn readiness_is_cold_and_operations_are_separate() {
  let state = state();
  assert_eq!(state.operations("/api/ready").status(), StatusCode::OK);
  assert_eq!(
    state.operations("/v1/ready").status(),
    StatusCode::NOT_FOUND
  );
  assert!(
    String::from_utf8(state.metrics())
      .unwrap()
      .contains("rilgate_verifier_service_requests_total{outcome=\"cancelled\"} 0")
  );
  state.mark_unhealthy();
  assert_eq!(
    state.operations("/api/ready").status(),
    StatusCode::SERVICE_UNAVAILABLE
  );
  assert_eq!(state.operations("/api/live").status(), StatusCode::OK);
}

struct Waiting(tokio::sync::Notify);
impl RandomXHasher for Waiting {
  fn kind(&self) -> HasherKind {
    HasherKind::Local
  }
  fn hash(&self, _: Vec<u8>, _: Vec<u8>, _: Instant) -> BoxFuture<'_, Result<[u8; 32], HashError>> {
    Box::pin(async {
      self.0.notify_one();
      std::future::pending().await
    })
  }
}
fn hash_request(deadline: u64) -> Request<Full<Bytes>> {
  Request::post("/v1/randomx/hash")
    .header("content-type", "application/json")
    .body(Full::new(Bytes::from(
      serde_json::json!({"seed":"ab".repeat(32),"input":"01","deadline_unix_ms":deadline})
        .to_string(),
    )))
    .unwrap()
}
#[tokio::test]
async fn deadline_and_cancellation_release_admission() {
  let state = State::new(Arc::new(Waiting(tokio::sync::Notify::new())), 4, true);
  let response = state
    .clone()
    .handle(hash_request(unix_ms() + 20), Some("client"))
    .await;
  assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
  assert_eq!(response.headers()["retry-after"], "1");
  assert_eq!(state.counts[3].load(Ordering::Relaxed), 1);
  assert!(state.clients.lock().unwrap().is_empty());
  let waiting = Arc::new(Waiting(tokio::sync::Notify::new()));
  let state = State::new(waiting.clone(), 4, true);
  let request = tokio::spawn(
    state
      .clone()
      .handle(hash_request(unix_ms() + 5000), Some("client")),
  );
  waiting.0.notified().await;
  request.abort();
  assert!(request.await.unwrap_err().is_cancelled());
  assert_eq!(state.counts[4].load(Ordering::Relaxed), 1);
  assert!(state.clients.lock().unwrap().is_empty());
}
#[tokio::test]
async fn closed_http_connection_cancels_pending_work() {
  use tokio::io::AsyncWriteExt;
  let waiting = Arc::new(Waiting(tokio::sync::Notify::new()));
  let state = State::new(waiting.clone(), 4, true);
  let (mut client, server) = tokio::io::duplex(8192);
  let state_for_connection = state.clone();
  let connection = tokio::spawn(async move {
    let service = service_fn(move |request: Request<Incoming>| {
      let state = state_for_connection.clone();
      async { Ok::<_, Infallible>(state.handle(request, Some("client")).await) }
    });
    let _ = http1::Builder::new()
      .serve_connection(TokioIo::new(server), service)
      .await;
  });
  let body =
    serde_json::json!({"seed":"ab".repeat(32),"input":"01","deadline_unix_ms":unix_ms()+5000})
      .to_string();
  client.write_all(format!("POST /v1/randomx/hash HTTP/1.1\r\nHost: verifier\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
  waiting.0.notified().await;
  drop(client);
  tokio::time::timeout(Duration::from_secs(1), connection)
    .await
    .unwrap()
    .unwrap();
  assert_eq!(state.counts[4].load(Ordering::Relaxed), 1);
  assert!(state.clients.lock().unwrap().is_empty());
}

#[tokio::test]
async fn overloaded_requests_do_not_reach_the_hasher() {
  let state = state();
  let first = state.admit(Some("client")).unwrap();
  let second = state.admit(Some("client")).unwrap();
  let response = state
    .clone()
    .handle(hash_request(unix_ms() + 1000), Some("client"))
    .await;
  assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
  assert_eq!(response.headers()["retry-after"], "1");
  assert_eq!(state.counts[2].load(Ordering::Relaxed), 1);
  assert_eq!(state.counts[0].load(Ordering::Relaxed), 0);
  drop((first, second));
}

#[tokio::test]
async fn rejected_deadlines_have_a_reason_and_expired_metrics() {
  let state = state();
  for deadline in [1, unix_ms() + 61_000] {
    let response = state
      .clone()
      .handle(hash_request(deadline), Some("client"))
      .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
      serde_json::from_slice::<serde_json::Value>(&body).unwrap()["reason"],
      "deadline"
    );
  }
  assert_eq!(state.counts[3].load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn unhealthy_window_recovers_without_hash_traffic_and_shutdown_stays_unready() {
  let state = state();
  state.mark_unhealthy();
  assert_eq!(state.ready().status(), StatusCode::SERVICE_UNAVAILABLE);
  *state.unhealthy_until.lock().unwrap() = Some(Instant::now());
  assert_eq!(state.ready().status(), StatusCode::OK);
  state.stopping.store(true, Ordering::Relaxed);
  assert_eq!(state.ready().status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn accept_errors_retry_with_backoff() {
  let started = Instant::now();
  let mut attempts = 0;
  let value = accept_retry(|| {
    attempts += 1;
    std::future::ready(if attempts < 3 {
      Err(std::io::Error::from_raw_os_error(24))
    } else {
      Ok(7)
    })
  })
  .await;
  assert_eq!(value, 7);
  assert_eq!(attempts, 3);
  assert!(started.elapsed() >= Duration::from_millis(100));
}

#[tokio::test]
async fn drain_completes_work_and_bounds_pending_tasks() {
  let mut tasks = tokio::task::JoinSet::new();
  let completed = Arc::new(AtomicBool::new(false));
  let done = completed.clone();
  tasks.spawn(async move {
    tokio::time::sleep(Duration::from_millis(10)).await;
    done.store(true, Ordering::Relaxed);
  });
  drain(&mut tasks, Duration::from_secs(1)).await;
  assert!(completed.load(Ordering::Relaxed));
  tasks.spawn(std::future::pending());
  drain(&mut tasks, Duration::from_millis(10)).await;
  assert!(tasks.is_empty());
}

#[tokio::test]
async fn operations_budget_is_independent_of_hash_connections() {
  use tokio::io::{AsyncReadExt, AsyncWriteExt};
  let hash_budget = Arc::new(Semaphore::new(1));
  let _occupied = hash_budget.acquire().await.unwrap();
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let task = tokio::spawn(operations_loop(listener, state()));
  let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
  client
    .write_all(b"GET /api/live HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
    .await
    .unwrap();
  let mut response = String::new();
  tokio::time::timeout(Duration::from_secs(1), client.read_to_string(&mut response))
    .await
    .unwrap()
    .unwrap();
  assert!(response.starts_with("HTTP/1.1 200"));
  task.abort();
  let _ = task.await;
}
#[tokio::test]
async fn body_transport_errors_are_unavailable() {
  let body = http_body_util::StreamBody::new(futures_util::stream::iter([Err::<
    hyper::body::Frame<Bytes>,
    std::io::Error,
  >(
    std::io::Error::other("connection reset"),
  )]));
  let response = state()
    .handle(
      Request::post("/v1/randomx/hash")
        .header("content-type", "application/json")
        .body(body)
        .unwrap(),
      Some("client"),
    )
    .await;
  assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
