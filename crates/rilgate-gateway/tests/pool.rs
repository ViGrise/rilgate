mod common;

use rilgate_pool::{StratumPoolProvider, WorkProvider};
use serde_json::json;
use std::time::Duration;
use tokio::{
  io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
  net::{TcpListener, TcpStream},
};

fn job(id: &str) -> serde_json::Value {
  json!({"blob": "00".repeat(64), "job_id": id, "seed_hash": "ab".repeat(32), "target": "ffffffff", "algo": "rx/0"})
}

async fn login(socket: TcpStream, difficulty: u64) -> BufReader<TcpStream> {
  let mut reader = BufReader::new(socket);
  let mut line = String::new();
  reader.read_line(&mut line).await.unwrap();
  let request: serde_json::Value = serde_json::from_str(&line).unwrap();
  assert_eq!(request["params"]["login"], format!("x+{difficulty}"));
  let response = json!({"id": 1, "result": {"id": "miner", "job": job("first")}});
  reader
    .get_mut()
    .write_all(format!("{response}\n").as_bytes())
    .await
    .unwrap();
  reader
}

#[tokio::test]
async fn unpooled_fetches_connect_per_job_and_share_the_session_budget() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config().pow.stratum_pool.clone();
  cfg.pools[0].port = listener.local_addr().unwrap().port();
  cfg.job_session_mode = rilgate_pool::JobSessionMode::PerChallenge;
  cfg.max_sessions = 1;
  let server = tokio::spawn(async move {
    for difficulty in [1000, 2000] {
      let (socket, _) = listener.accept().await.unwrap();
      let mut socket = login(socket, difficulty).await;
      assert_eq!(socket.read(&mut [0; 1]).await.unwrap(), 0);
    }
  });
  let pool = StratumPoolProvider::new(cfg).unwrap();
  let permit = pool.acquire_slot().unwrap();
  assert!(pool.fetch(1000).await.is_err());
  drop(permit);
  for difficulty in [1000, 2000] {
    let fetched = tokio::time::timeout(Duration::from_secs(3), pool.fetch(difficulty))
      .await
      .unwrap()
      .unwrap();
    assert_eq!(fetched.job_id, "first");
  }
  tokio::time::timeout(Duration::from_secs(3), server)
    .await
    .unwrap()
    .unwrap();
  assert!(pool.acquire_slot().is_ok());
}

#[tokio::test]
async fn job_notifications_do_not_keep_unused_sessions_alive() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config().pow.stratum_pool.clone();
  cfg.pools[0].port = listener.local_addr().unwrap().port();
  cfg.max_sessions = 1;
  cfg.max_job_sessions = 1;
  cfg.job_session_idle = std::time::Duration::from_secs(1);
  let (rotated, rotation) = tokio::sync::oneshot::channel();
  let server = tokio::spawn(async move {
    let (socket, _) = listener.accept().await.unwrap();
    let mut socket = login(socket, 1000).await;
    let notification = format!("{}\n", json!({"method": "job", "params": job("rotated")}));
    socket
      .get_mut()
      .write_all(notification.as_bytes())
      .await
      .unwrap();
    rotated.send(()).unwrap();
    let mut buffer = [0; 1];
    loop {
      tokio::select! {
        read = socket.read(&mut buffer) => {
          // Closing a socket with unread notifications can produce a reset instead of EOF.
          assert!(matches!(read, Ok(0)) || read.is_err_and(|error| error.kind() == std::io::ErrorKind::ConnectionReset));
          break;
        }
        _ = tokio::time::sleep(Duration::from_millis(25)) => {
          if socket.get_mut().write_all(notification.as_bytes()).await.is_err() {
            break;
          }
        }
      }
    }
    listener
  });
  let pool = StratumPoolProvider::new(cfg).unwrap();
  pool.fetch(1000).await.unwrap();
  rotation.await.unwrap();
  tokio::time::timeout(Duration::from_secs(2), async {
    while pool.fetch(1000).await.unwrap().job_id != "rotated" {
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .unwrap();
  let listener = tokio::time::timeout(Duration::from_secs(3), server)
    .await
    .unwrap()
    .unwrap();
  let server = tokio::spawn(async move {
    let (socket, _) = listener.accept().await.unwrap();
    let mut socket = login(socket, 2000).await;
    assert_eq!(socket.read(&mut [0; 1]).await.unwrap(), 0);
  });
  assert_eq!(pool.fetch(2000).await.unwrap().job_id, "first");
  drop(pool);
  tokio::time::timeout(Duration::from_secs(3), server)
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn concurrent_fetches_share_one_initializing_session_and_follow_job_rotation() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config().pow.stratum_pool.clone();
  cfg.pools[0].port = listener.local_addr().unwrap().port();
  cfg.max_sessions = 1;
  cfg.max_job_sessions = 1;
  let (rotate, rotation) = tokio::sync::oneshot::channel();
  let server = tokio::spawn(async move {
    let (socket, _) = listener.accept().await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let mut socket = login(socket, 1000).await;
    rotation.await.unwrap();
    let notification = format!("{}\n", json!({"method":"job", "params":job("new")}));
    socket
      .get_mut()
      .write_all(notification.as_bytes())
      .await
      .unwrap();
    assert!(
      tokio::time::timeout(Duration::from_millis(150), listener.accept())
        .await
        .is_err()
    );
    assert_eq!(socket.read(&mut [0; 1]).await.unwrap(), 0);
  });
  let pool = std::sync::Arc::new(StratumPoolProvider::new(cfg).unwrap());
  let mut requests = tokio::task::JoinSet::new();
  for _ in 0..128 {
    let pool = pool.clone();
    requests.spawn(async move { pool.fetch(1000).await.unwrap().job_id });
  }
  while let Some(result) = requests.join_next().await {
    assert_eq!(result.unwrap(), "first");
  }
  rotate.send(()).unwrap();
  tokio::time::timeout(Duration::from_secs(2), async {
    while pool.fetch(1000).await.unwrap().job_id != "new" {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  assert!(pool.fetch(2000).await.is_err());
  assert_eq!(pool.failures(), (1, 1));
  assert!(!pool.acquire_slot().is_ok());
  drop(pool);
  tokio::time::timeout(Duration::from_secs(3), server)
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn disconnected_shared_sessions_are_replaced() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config().pow.stratum_pool.clone();
  cfg.pools[0].port = listener.local_addr().unwrap().port();
  cfg.max_sessions = 1;
  cfg.max_job_sessions = 1;
  let (disconnect, disconnected) = tokio::sync::oneshot::channel();
  let (closed, closure) = tokio::sync::oneshot::channel();
  let server = tokio::spawn(async move {
    let (socket, _) = listener.accept().await.unwrap();
    let socket = login(socket, 1000).await;
    disconnected.await.unwrap();
    drop(socket);
    closed.send(()).unwrap();
    let (socket, _) = listener.accept().await.unwrap();
    let mut socket = login(socket, 1000).await;
    let notification = format!("{}\n", json!({"method":"job", "params":job("replacement")}));
    socket
      .get_mut()
      .write_all(notification.as_bytes())
      .await
      .unwrap();
    assert_eq!(socket.read(&mut [0; 1]).await.unwrap(), 0);
  });
  let pool = StratumPoolProvider::new(cfg).unwrap();
  assert_eq!(pool.fetch(1000).await.unwrap().job_id, "first");
  disconnect.send(()).unwrap();
  closure.await.unwrap();
  tokio::time::timeout(Duration::from_secs(3), async {
    loop {
      if pool
        .fetch(1000)
        .await
        .is_ok_and(|job| job.job_id == "replacement")
      {
        break;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .unwrap();
  drop(pool);
  tokio::time::timeout(Duration::from_secs(3), server)
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn canceling_a_fetch_does_not_cancel_the_shared_connection() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config().pow.stratum_pool.clone();
  cfg.pools[0].port = listener.local_addr().unwrap().port();
  cfg.max_sessions = 1;
  cfg.max_job_sessions = 1;
  cfg.job_session_idle = std::time::Duration::from_secs(1);
  cfg.connect_timeout = std::time::Duration::from_secs(3);
  let (accepted, acceptance) = tokio::sync::oneshot::channel();
  let (release, released) = tokio::sync::oneshot::channel();
  let server = tokio::spawn(async move {
    let (socket, _) = listener.accept().await.unwrap();
    accepted.send(()).unwrap();
    released.await.unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let mut socket = login(socket, 1000).await;
    assert_eq!(socket.read(&mut [0; 1]).await.unwrap(), 0);
  });
  let pool = std::sync::Arc::new(StratumPoolProvider::new(cfg).unwrap());
  let canceled = {
    let pool = pool.clone();
    tokio::spawn(async move { pool.fetch(1000).await })
  };
  acceptance.await.unwrap();
  canceled.abort();
  assert!(canceled.await.unwrap_err().is_cancelled());
  release.send(()).unwrap();
  assert_eq!(
    tokio::time::timeout(Duration::from_secs(2), pool.fetch(1000))
      .await
      .unwrap()
      .unwrap()
      .job_id,
    "first"
  );
  assert_eq!(pool.failures(), (0, 0));
  drop(pool);
  tokio::time::timeout(Duration::from_secs(3), server)
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn all_pools_in_cooldown_still_retry_in_order() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut config = common::config().pow.stratum_pool.clone();
  config.pools[0].port = listener.local_addr().unwrap().port();
  let server = tokio::spawn(async move {
    let (stream, _) = listener.accept().await.unwrap();
    let mut stream = BufReader::new(stream);
    let mut line = String::new();
    stream.read_line(&mut line).await.unwrap();
    stream
      .get_mut()
      .write_all(b"{\"id\":1,\"error\":\"login-secret\"}\n")
      .await
      .unwrap();
    let (stream, _) = listener.accept().await.unwrap();
    login(stream, 1000).await;
  });
  let provider = StratumPoolProvider::new(config).unwrap();
  let error = provider.connect(1000).await.err().unwrap();
  assert!(!format!("{error:#}").contains("login-secret"));
  assert_eq!(
    provider.connect(1000).await.unwrap().current_job.job_id,
    "first"
  );
  server.await.unwrap();
}

#[tokio::test]
async fn proxy_failover_skips_failed_routes_and_empty_override_connects_directly() {
  let pool = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let port = pool.local_addr().unwrap().port();
  let refused = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let working = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut config = common::config().pow.stratum_pool.clone();
  config.pools[0].port = port;
  config.proxies = [(&refused, "refused"), (&working, "working")]
    .into_iter()
    .map(|(listener, name)| rilgate_pool::Proxy {
      name: name.into(),
      url: rilgate_config::Secret(format!("http://{}", listener.local_addr().unwrap())),
      username: None,
      password: None,
    })
    .collect();
  let rejecting = tokio::spawn(async move {
    let (mut stream, _) = refused.accept().await.unwrap();
    let mut byte = [0];
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
      stream.read_exact(&mut byte).await.unwrap();
      bytes.push(byte[0]);
    }
    stream
      .write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n")
      .await
      .unwrap();
  });
  let forwarding = tokio::spawn(async move {
    for _ in 0..2 {
      let (mut stream, _) = working.accept().await.unwrap();
      let mut byte = [0];
      let mut bytes = Vec::new();
      while !bytes.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).await.unwrap();
        bytes.push(byte[0]);
      }
      let mut target = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
      stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.unwrap();
      tokio::io::copy_bidirectional(&mut stream, &mut target)
        .await
        .unwrap();
    }
  });
  let server = tokio::spawn(async move {
    for _ in 0..3 {
      let (stream, _) = pool.accept().await.unwrap();
      login(stream, 1000).await;
    }
  });
  let provider = StratumPoolProvider::new(config.clone()).unwrap();
  for _ in 0..2 {
    provider.connect(1000).await.unwrap();
  }
  let metrics = provider.connection_metrics();
  assert!(
    metrics
      .contains("rilgate_stratum_proxy_connections_total{proxy=\"refused\",outcome=\"failed\"} 1"),
    "{metrics}"
  );
  assert!(metrics.contains(
    "rilgate_stratum_proxy_connections_total{proxy=\"working\",outcome=\"connected\"} 2"
  ));
  config.pools[0].proxies = Some(Vec::new());
  StratumPoolProvider::new(config)
    .unwrap()
    .connect(1000)
    .await
    .unwrap();
  rejecting.await.unwrap();
  forwarding.await.unwrap();
  server.await.unwrap();
}

#[tokio::test]
async fn difficulty_changes_count_once_per_connection_and_preserve_pool_targets() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut config = common::config().pow.stratum_pool.clone();
  config.pools[0].port = listener.local_addr().unwrap().port();
  let server = tokio::spawn(async move {
    let (stream, _) = listener.accept().await.unwrap();
    let mut stream = BufReader::new(stream);
    let mut line = String::new();
    stream.read_line(&mut line).await.unwrap();
    let mut initial = job("initial");
    initial["target"] = hex::encode((u32::MAX / 1000).to_le_bytes()).into();
    stream
      .get_mut()
      .write_all(
        format!(
          "{}\n",
          json!({"id": 1, "result": {"id": "miner", "job": initial}})
        )
        .as_bytes(),
      )
      .await
      .unwrap();
    for difficulty in [2000, 4000] {
      let mut next = job("changed");
      next["target"] = hex::encode((u32::MAX / difficulty).to_le_bytes()).into();
      stream
        .get_mut()
        .write_all(format!("{}\n", json!({"method": "job", "params": next})).as_bytes())
        .await
        .unwrap();
    }
  });
  let provider = StratumPoolProvider::new(config).unwrap();
  let mut session = provider.connect(1000).await.unwrap();
  assert!(
    provider
      .connection_metrics()
      .contains("rilgate_stratum_pool_difficulty_mismatch_total{pool=\"default\"} 0")
  );
  for difficulty in [2000, 4000] {
    session.read_event().await.unwrap();
    assert_eq!(
      session.current_job.target,
      hex::encode((u32::MAX / difficulty).to_le_bytes())
    );
  }
  assert!(
    provider
      .connection_metrics()
      .contains("rilgate_stratum_pool_difficulty_mismatch_total{pool=\"default\"} 1")
  );
  server.await.unwrap();
}

#[tokio::test]
async fn readiness_observes_healthy_backup_without_changing_connection_state() {
  let hanging = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let healthy = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut config = common::config().pow.stratum_pool.clone();
  config.pools[0].port = hanging.local_addr().unwrap().port();
  config.pools[0].tls.enabled = true;
  let mut backup = config.pools[0].clone();
  backup.name = "backup".into();
  backup.port = healthy.local_addr().unwrap().port();
  backup.tls.enabled = false;
  config.pools.push(backup);
  let provider = StratumPoolProvider::new(config).unwrap();
  let before = provider.connection_metrics();
  let server = tokio::spawn(async move {
    let (stream, _) = hanging.accept().await.unwrap();
    std::future::pending::<()>().await;
    drop(stream);
  });
  let results = tokio::time::timeout(
    Duration::from_millis(500),
    provider.readiness(Duration::from_millis(100)),
  )
  .await
  .unwrap();
  assert_eq!(
    results,
    [("default".into(), false), ("backup".into(), true)]
  );
  assert_eq!(provider.connection_metrics(), before);
  server.abort();
}

#[test]
fn invalid_pool_ca_stops_provider_creation_and_names_the_pool() {
  let directory = tempfile::tempdir().unwrap();
  for contents in [
    None,
    Some("invalid PEM"),
    Some("-----BEGIN CERTIFICATE-----\nAA==\n-----END CERTIFICATE-----\n"),
  ] {
    let path = directory.path().join("ca.pem");
    if let Some(contents) = contents {
      std::fs::write(&path, contents).unwrap();
    }
    let mut config = common::config().pow.stratum_pool.clone();
    config.pools[0].name = "primary".into();
    config.pools[0].tls.enabled = true;
    config.pools[0].tls.ca = Some(path);
    let error = StratumPoolProvider::new(config)
      .err()
      .expect("invalid CA must stop startup");
    assert!(format!("{error:#}").contains("primary"), "{error:#}");
  }
}

fn http_proxy(listener: &TcpListener, name: &str) -> rilgate_pool::Proxy {
  rilgate_pool::Proxy {
    name: name.into(),
    url: rilgate_config::Secret(format!("http://{}", listener.local_addr().unwrap())),
    username: None,
    password: None,
  }
}

async fn proxy_headers(stream: &mut TcpStream) {
  let mut bytes = Vec::new();
  while !bytes.ends_with(b"\r\n\r\n") {
    bytes.push(stream.read_u8().await.unwrap());
  }
}

#[tokio::test]
async fn pool_login_failures_preserve_proxy_health_and_fail_over_to_the_backup_pool() {
  for response in [
    Some("{\"id\":1,\"error\":\"rejected\"}\n"),
    Some("{\"id\":1,\"result\":{\"id\":\"miner\",\"job\":{}}}\n"),
    None,
  ] {
    let first = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let backup = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut config = common::config().pow.stratum_pool.clone();
    config.connect_timeout = Duration::from_millis(200);
    config.cooldown = Duration::from_secs(1);
    config.proxies = vec![http_proxy(&first, "first"), http_proxy(&second, "second")];
    let mut pool = config.pools[0].clone();
    pool.name = "backup".into();
    pool.port = backup.local_addr().unwrap().port();
    pool.proxies = Some(Vec::new());
    config.pools.push(pool);
    let server = tokio::spawn(async move {
      for _ in 0..2 {
        let (mut stream, _) = first.accept().await.unwrap();
        proxy_headers(&mut stream).await;
        stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        if let Some(response) = response {
          reader
            .get_mut()
            .write_all(response.as_bytes())
            .await
            .unwrap();
        } else {
          tokio::time::sleep(Duration::from_millis(250)).await;
        }
      }
    });
    let healthy = tokio::spawn(async move {
      for _ in 0..2 {
        let (stream, _) = backup.accept().await.unwrap();
        login(stream, 1000).await;
      }
    });
    let provider = StratumPoolProvider::new(config).unwrap();
    for attempt in 0..2 {
      if attempt > 0 {
        tokio::time::sleep(Duration::from_millis(1100)).await;
      }
      assert_eq!(
        provider.connect(1000).await.unwrap().current_job.job_id,
        "first"
      );
    }
    let metrics = provider.connection_metrics();
    assert!(
      metrics
        .contains("rilgate_stratum_proxy_connections_total{proxy=\"first\",outcome=\"failed\"} 0"),
      "{metrics}"
    );
    assert!(
      metrics.contains(
        "rilgate_stratum_proxy_connections_total{proxy=\"first\",outcome=\"connected\"} 2"
      ),
      "{metrics}"
    );
    assert!(
      tokio::time::timeout(Duration::from_millis(50), second.accept())
        .await
        .is_err()
    );
    server.await.unwrap();
    healthy.await.unwrap();
  }
}

#[tokio::test]
async fn transport_and_login_each_have_a_full_connect_timeout() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut config = common::config().pow.stratum_pool.clone();
  config.connect_timeout = Duration::from_millis(500);
  config.proxies = vec![http_proxy(&listener, "route")];
  let server = tokio::spawn(async move {
    let (mut stream, _) = listener.accept().await.unwrap();
    proxy_headers(&mut stream).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    login(stream, 1000).await;
  });
  let provider = StratumPoolProvider::new(config).unwrap();
  assert_eq!(
    provider.connect(1000).await.unwrap().current_job.job_id,
    "first"
  );
  server.await.unwrap();
}

#[tokio::test]
async fn randomx_fallback_cookie_expires_when_pool_work_can_resume() {
  use axum::{
    body::{Body, to_bytes},
    http::Request,
  };
  use rilgate_config::{PowAlgorithm, model::pow::RandomXFallback};
  use rilgate_test_utils::gateway as app;
  use std::sync::Arc;
  use tower::ServiceExt;

  let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let upstream = origin.local_addr().unwrap();
  let origin_task = tokio::spawn(async move {
    loop {
      let (mut socket, _) = origin.accept().await.unwrap();
      tokio::spawn(async move {
        let mut buffer = [0; 4096];
        let _ = socket.read(&mut buffer).await;
        let _ = socket
          .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
          .await;
      });
    }
  });
  for fallback in [true, false] {
    let reserved = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = reserved.local_addr().unwrap();
    drop(reserved);
    let mut cfg = common::config();
    let config = Arc::get_mut(&mut cfg).unwrap();
    config.upstream.url = format!("http://{upstream}");
    config.server.metrics_enabled = true;
    config.pow.stratum_pool.pools[0].port = address.port();
    config.pow.stratum_pool.cooldown = Duration::from_secs(1);
    let choice = config.pow.policy.rules.as_mut().unwrap()[0]
      .challenge
      .as_mut()
      .unwrap();
    choice.algorithm = Some(PowAlgorithm::RandomX);
    choice.difficulty = Some(1000);
    if fallback {
      config.pow.randomx_fallback = Some(RandomXFallback {
        algorithm: PowAlgorithm::Fast,
        difficulty: 2,
      });
    }
    let router = app::router(cfg).await.unwrap();
    let request = |path: &str| Request::builder().uri(path).header("host", "example.test");
    let page = router
      .clone()
      .oneshot(request("/protected").body(Body::empty()).unwrap())
      .await
      .unwrap();
    let html =
      String::from_utf8(to_bytes(page.into_body(), 100000).await.unwrap().to_vec()).unwrap();
    assert!(
      html.contains("window.POW_ALGORITHM = \"randomx\""),
      "{html}"
    );
    let page = router
      .clone()
      .oneshot(
        request("/protected")
          .header("accept", "application/json")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    let body: serde_json::Value =
      serde_json::from_slice(&to_bytes(page.into_body(), 100000).await.unwrap()).unwrap();
    let issue = |token: &str| {
      request("/.well-known/test-pow/api/challenge")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(json!({"presentation_token": token}).to_string()))
        .unwrap()
    };
    let failed = router
      .clone()
      .oneshot(issue(body["challenge"]["token"].as_str().unwrap()))
      .await
      .unwrap();
    assert_eq!(failed.status(), 503);
    assert_eq!(failed.headers()["retry-after"], "1");
    assert_eq!(failed.headers()["cache-control"], "no-store");
    assert_eq!(
      serde_json::from_slice::<serde_json::Value>(
        &to_bytes(failed.into_body(), 10000).await.unwrap()
      )
      .unwrap(),
      json!({"code": "work_unavailable"})
    );
    let client = reqwest::Client::new();
    let ready = client
      .get(format!(
        "http://{}/.well-known/test-pow/api/ready",
        router.operations_address()
      ))
      .send()
      .await
      .unwrap();
    assert_eq!(ready.status().as_u16(), if fallback { 200 } else { 503 });
    let readiness: serde_json::Value =
      serde_json::from_slice(&ready.bytes().await.unwrap()).unwrap();
    assert_eq!(readiness["pool"], false);
    assert_eq!(readiness["pools"][0][1], false);
    let page = router
      .clone()
      .oneshot(request("/protected").body(Body::empty()).unwrap())
      .await
      .unwrap();
    let html =
      String::from_utf8(to_bytes(page.into_body(), 100000).await.unwrap().to_vec()).unwrap();
    let algorithm = if fallback { "fast" } else { "randomx" };
    assert!(
      html.contains(&format!("window.POW_ALGORITHM = \"{algorithm}\"")),
      "{html}"
    );
    let page = router
      .clone()
      .oneshot(
        request("/protected")
          .header("accept", "application/json")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    let body: serde_json::Value =
      serde_json::from_slice(&to_bytes(page.into_body(), 100000).await.unwrap()).unwrap();
    let issued = router
      .clone()
      .oneshot(issue(body["challenge"]["token"].as_str().unwrap()))
      .await
      .unwrap();
    if !fallback {
      assert_eq!(issued.status(), 503);
      continue;
    }
    assert_eq!(issued.status(), 200);
    let challenge: serde_json::Value =
      serde_json::from_slice(&to_bytes(issued.into_body(), 100000).await.unwrap()).unwrap();
    assert_eq!(challenge["algorithm"], "fast");
    assert_eq!(challenge["difficulty"], 2);
    let nonce = (0..100000)
      .map(|n| n.to_string())
      .find(|nonce| {
        rilgate_pow_hash::sha256::verifier::verify_fast(
          challenge["challenge"].as_str().unwrap(),
          nonce,
          2,
        )
        .unwrap()
      })
      .unwrap();
    let verified = router.clone().oneshot(request("/.well-known/test-pow/api/verify").method("POST").header("content-type", "application/json").body(Body::from(json!({"challenge_token": challenge["challenge_token"], "nonce": nonce, "set_cookie": true}).to_string())).unwrap()).await.unwrap();
    assert_eq!(verified.status(), 200);
    let cookie = verified.headers()["set-cookie"]
      .to_str()
      .unwrap()
      .split(';')
      .next()
      .unwrap()
      .to_string();
    let passed = router
      .clone()
      .oneshot(
        request("/protected")
          .header("cookie", &cookie)
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(passed.status(), 200);
    let pool = TcpListener::bind(address).await.unwrap();
    let server = tokio::spawn(async move {
      let (socket, _) = pool.accept().await.unwrap();
      let _session = login(socket, 1000).await;
      std::future::pending::<()>().await;
    });
    tokio::time::sleep(Duration::from_secs(1)).await;
    let recovered = router
      .clone()
      .oneshot(
        request("/protected")
          .header("cookie", cookie)
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    let html = String::from_utf8(
      to_bytes(recovered.into_body(), 100000)
        .await
        .unwrap()
        .to_vec(),
    )
    .unwrap();
    assert!(html.contains("window.POW_ALGORITHM = \"randomx\""));
    let metrics = client
      .get(format!(
        "http://{}/.well-known/test-pow/api/metrics",
        router.operations_address()
      ))
      .send()
      .await
      .unwrap()
      .text()
      .await
      .unwrap();
    assert!(
      metrics.contains("rilgate_pow_fallback_total{from=\"randomx\",to=\"fast\"} 2"),
      "{metrics}"
    );
    server.abort();
  }
  origin_task.abort();
}
