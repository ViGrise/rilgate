mod common;

use rilgate_pool::{P2PoolProvider, WorkProvider};
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
  let mut cfg = common::config().pow.p2pool.clone();
  cfg.port = listener.local_addr().unwrap().port();
  cfg.job_session_mode = rilgate_pool::JobSessionMode::PerChallenge;
  cfg.max_sessions = 1;
  let server = tokio::spawn(async move {
    for difficulty in [1000, 2000] {
      let (socket, _) = listener.accept().await.unwrap();
      let mut socket = login(socket, difficulty).await;
      assert_eq!(socket.read(&mut [0; 1]).await.unwrap(), 0);
    }
  });
  let pool = P2PoolProvider::new(cfg).unwrap();
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
  let mut cfg = common::config().pow.p2pool.clone();
  cfg.port = listener.local_addr().unwrap().port();
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
  let pool = P2PoolProvider::new(cfg).unwrap();
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
  let mut cfg = common::config().pow.p2pool.clone();
  cfg.port = listener.local_addr().unwrap().port();
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
  let pool = std::sync::Arc::new(P2PoolProvider::new(cfg).unwrap());
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
  let mut cfg = common::config().pow.p2pool.clone();
  cfg.port = listener.local_addr().unwrap().port();
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
  let pool = P2PoolProvider::new(cfg).unwrap();
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
  let mut cfg = common::config().pow.p2pool.clone();
  cfg.port = listener.local_addr().unwrap().port();
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
  let pool = std::sync::Arc::new(P2PoolProvider::new(cfg).unwrap());
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
