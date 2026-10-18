use rilgate_gateway::proof::websocket::messages::ServerWsMessage;
use rilgate_test_utils::binding::unbound;
use rilgate_test_utils::gateway as app;
use rilgate_test_utils::gateway::service as make_service;
mod common;

use axum::{
  body::Body,
  http::{Request, StatusCode},
};
use futures_util::{SinkExt, StreamExt};
use rilgate_gateway::{
  config::{Config, PowAlgorithm, PowEnforcement},
  transport::errors::{ApiError, CapacityExceeded},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{
  io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
  net::{TcpListener, TcpStream},
};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tower::ServiceExt;

type Browser = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

fn job(id: &str) -> Value {
  json!({"blob": "00".repeat(64), "job_id": id, "seed_hash": "ab".repeat(32), "target": "ffffffff", "algo": "rx/0"})
}

async fn read_json(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> Value {
  let mut line = String::new();
  assert!(reader.read_line(&mut line).await.unwrap() > 0);
  serde_json::from_str(&line).unwrap()
}

async fn write_json(writer: &mut tokio::net::tcp::OwnedWriteHalf, value: Value) {
  let mut bytes = serde_json::to_vec(&value).unwrap();
  bytes.push(b'\n');
  writer.write_all(&bytes).await.unwrap();
}

async fn login(
  listener: TcpListener,
) -> (
  BufReader<tokio::net::tcp::OwnedReadHalf>,
  tokio::net::tcp::OwnedWriteHalf,
) {
  let (socket, _) = listener.accept().await.unwrap();
  let (read, mut write) = socket.into_split();
  let mut reader = BufReader::new(read);
  let request = read_json(&mut reader).await;
  assert_eq!(request["params"]["login"], "x+1000");
  write_json(
    &mut write,
    json!({"id": 1, "result": {"id": "miner", "job": job("first")}}),
  )
  .await;
  (reader, write)
}

async fn browser(p2pool_port: u16) -> (Browser, Config, tokio::task::JoinHandle<()>) {
  let mut cfg = common::config();
  let pow = &mut Arc::get_mut(&mut cfg).unwrap().pow;
  pow.enforce = PowEnforcement::P2poolAccepted;
  pow.p2pool.port = p2pool_port;
  pow.policy.rules.as_mut().unwrap()[0]
    .challenge
    .as_mut()
    .unwrap()
    .algorithm = Some(PowAlgorithm::RandomX);
  pow.policy.rules.as_mut().unwrap()[0]
    .challenge
    .as_mut()
    .unwrap()
    .difficulty = Some(1000);
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let host = listener.local_addr().unwrap().to_string();
  let service = make_service(&cfg).unwrap();
  let decision = service.evaluate_request(&Default::default(), "/protected");
  let presentation = service
    .issue_presentation(host.clone(), "/protected".into(), &decision)
    .unwrap();
  let gateway = app::build(cfg.clone()).await.unwrap();
  let server = tokio::spawn(async move {
    gateway
      .serve(vec![listener], std::future::pending())
      .await
      .unwrap();
  });
  let url = format!(
    "ws://{host}{}/api/ws?presentation_token={presentation}",
    cfg.web.base_path
  );
  let (browser, _) = connect_async(url).await.unwrap();
  (browser, cfg, server)
}

async fn message(browser: &mut Browser) -> ServerWsMessage {
  let frame = tokio::time::timeout(Duration::from_secs(3), browser.next())
    .await
    .unwrap()
    .unwrap()
    .unwrap();
  serde_json::from_str(frame.to_text().unwrap()).unwrap()
}

async fn share(browser: &mut Browser, id: &str, result: &str) {
  browser
    .send(Message::Text(
      json!({"type": "share", "job_id": id, "nonce": "01020304", "result": result})
        .to_string()
        .into(),
    ))
    .await
    .unwrap();
}

#[tokio::test]
async fn receipt_requires_a_matching_accepted_submission_even_across_job_rotation() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let port = listener.local_addr().unwrap().port();
  let (release, wait) = tokio::sync::oneshot::channel();
  let p2pool = tokio::spawn(async move {
    let (mut reader, mut writer) = login(listener).await;
    let submit = read_json(&mut reader).await;
    assert_eq!(submit["method"], "submit");
    assert_eq!(submit["params"]["job_id"], "first");
    assert_eq!(submit["params"]["nonce"], "04030201");
    assert_eq!(submit["params"]["result"], "00".repeat(32));
    write_json(
      &mut writer,
      json!({"method": "job", "params": job("second")}),
    )
    .await;
    write_json(&mut writer, json!({"id": 999, "result": {"status": "OK"}})).await;
    wait.await.unwrap();
    write_json(
      &mut writer,
      json!({"id": submit["id"], "result": {"status": "OK"}}),
    )
    .await;
  });
  let (mut ws, cfg, server) = browser(port).await;
  assert!(matches!(
    message(&mut ws).await,
    ServerWsMessage::Job { .. }
  ));
  share(&mut ws, "first", &"ff".repeat(32)).await;
  assert!(matches!(
    message(&mut ws).await,
    ServerWsMessage::Rejected { .. }
  ));
  share(&mut ws, "stale", &"00".repeat(32)).await;
  assert!(matches!(
    message(&mut ws).await,
    ServerWsMessage::Rejected { .. }
  ));
  share(&mut ws, "first", &"00".repeat(32)).await;
  assert!(
    matches!(message(&mut ws).await, ServerWsMessage::Job { job, .. } if job.job_id == "second")
  );
  assert!(
    tokio::time::timeout(Duration::from_millis(30), ws.next())
      .await
      .is_err()
  );
  release.send(()).unwrap();
  let ServerWsMessage::Accepted { receipt } = message(&mut ws).await else {
    panic!("expected accepted share")
  };
  let claims: rilgate_protocol::claims::ReceiptClaims = rilgate_crypto::token::verify_json(
    &receipt,
    cfg.pow.signing_key.expose().as_bytes(),
    rilgate_crypto::token::TokenPurpose::Receipt,
  )
  .unwrap();
  let service = make_service(&cfg).unwrap();
  let token = service
    .exchange_receipt(&receipt, &claims.host, &unbound())
    .unwrap();
  assert_eq!(
    service
      .verify_auth_token(&token, &claims.host, &unbound())
      .unwrap()
      .enforce,
    PowEnforcement::P2poolAccepted
  );
  ws.close(None).await.unwrap();
  p2pool.await.unwrap();
  server.abort();
}

#[tokio::test]
async fn a_silent_p2pool_submission_expires() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let port = listener.local_addr().unwrap().port();
  let p2pool = tokio::spawn(async move {
    let (mut reader, _writer) = login(listener).await;
    read_json(&mut reader).await;
    std::future::pending::<()>().await;
  });
  let (mut ws, _, server) = browser(port).await;
  message(&mut ws).await;
  share(&mut ws, "first", &"00".repeat(32)).await;
  assert!(
    matches!(message(&mut ws).await, ServerWsMessage::Error { message } if message.contains("acceptance timed out"))
  );
  p2pool.abort();
  server.abort();
}

#[tokio::test]
async fn session_capacity_returns_503_and_releases_when_a_permit_is_dropped() {
  let mut cfg = common::config();
  let p2pool = &mut Arc::get_mut(&mut cfg).unwrap().pow.p2pool;
  p2pool.max_sessions = 1;
  // Pooled job sessions draw from the same permit budget, so the cap has to
  // follow max_sessions down or configuration validation rejects the pair.
  p2pool.max_job_sessions = 1;
  let service = rilgate_pool::P2PoolProvider::new(cfg.pow.p2pool.clone()).unwrap();
  let permit = service.acquire_slot().unwrap();
  let error = service.acquire_slot().unwrap_err();
  assert!(error.is::<CapacityExceeded>());
  assert_eq!(
    ApiError::proof(error).into_response().status(),
    StatusCode::SERVICE_UNAVAILABLE
  );
  drop(permit);
  assert!(service.acquire_slot().is_ok());
}

#[tokio::test]
async fn internal_json_requests_are_bounded_and_uncacheable() {
  let router = app::router(common::config()).await.unwrap();
  let response = router
    .oneshot(
      Request::builder()
        .uri("/.well-known/test-pow/api/verify")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from("x".repeat(16 * 1024 + 1)))
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
  assert_eq!(response.headers()["cache-control"], "no-store");
}
