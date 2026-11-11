mod common;
use rilgate_test_utils::gateway as app;
use std::{sync::Arc, time::Duration};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::TcpStream,
};

#[tokio::test]
async fn a_connection_without_bytes_has_a_header_deadline() {
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.server.header_timeout = Duration::from_millis(100);
  config.server.idle_timeout = Duration::from_millis(150);
  let gateway = app::router(cfg).await.unwrap();
  let mut socket = TcpStream::connect(gateway.address()).await.unwrap();
  let result = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut [0; 1])).await;
  assert!(
    matches!(result, Ok(Ok(0)) | Ok(Err(_))),
    "connection remained open"
  );
}

#[tokio::test]
async fn keepalive_connections_have_an_idle_deadline() {
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.server.header_timeout = Duration::from_millis(100);
  config.server.idle_timeout = Duration::from_millis(150);
  let gateway = app::router(cfg).await.unwrap();
  let mut socket = TcpStream::connect(gateway.address()).await.unwrap();
  socket
    .write_all(
      b"HEAD /.well-known/test-pow/api/client-config HTTP/1.1\r\nHost: example.test\r\n\r\n",
    )
    .await
    .unwrap();
  let mut response = Vec::new();
  while !response.ends_with(b"\r\n\r\n") {
    let mut byte = [0];
    socket.read_exact(&mut byte).await.unwrap();
    response.extend_from_slice(&byte);
  }
  assert!(response.starts_with(b"HTTP/1.1 200"));
  let result = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut [0; 1])).await;
  assert!(
    matches!(result, Ok(Ok(0)) | Ok(Err(_))),
    "keepalive remained open"
  );
}

#[tokio::test]
async fn one_client_network_leaves_connection_capacity_for_another() {
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().server.max_connections = 66;
  let gateway = app::router(cfg).await.unwrap();
  let mut held = Vec::new();
  for _ in 0..65 {
    held.push(TcpStream::connect(gateway.address()).await.unwrap());
  }
  let mut rejected = held.pop().unwrap();
  let result = tokio::time::timeout(Duration::from_secs(2), rejected.read(&mut [0; 1])).await;
  assert!(
    matches!(result, Ok(Ok(0)) | Ok(Err(_))),
    "network exceeded its connection quota"
  );
  let client = reqwest::Client::new();
  let response = client
    .get(format!(
      "http://127.0.0.1:{}/.well-known/test-pow/api/client-config",
      gateway.address().port()
    ))
    .send()
    .await
    .unwrap();
  assert_eq!(response.status(), 200);
  drop(held);
}

async fn request_headers(socket: &mut TcpStream, ip: &str) -> Vec<u8> {
  socket.write_all(format!("HEAD /.well-known/test-pow/api/client-config HTTP/1.1\r\nHost: example.test\r\nX-Forwarded-For: {ip}\r\n\r\n").as_bytes()).await.unwrap();
  tokio::time::timeout(Duration::from_secs(2), async {
    let mut response = Vec::new();
    while !response.ends_with(b"\r\n\r\n") {
      let mut byte = [0];
      socket.read_exact(&mut byte).await.unwrap();
      response.extend_from_slice(&byte);
    }
    response
  })
  .await
  .unwrap()
}

#[tokio::test]
async fn trusted_proxy_networks_have_independent_connection_slots() {
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.server.max_connections_per_client = 2;
  config.server.metrics_enabled = true;
  config.server.trusted_proxies = vec!["::1/128".parse().unwrap()];
  let gateway = app::router(cfg).await.unwrap();
  let mut held = Vec::new();
  for _ in 0..2 {
    let mut socket = TcpStream::connect(gateway.address()).await.unwrap();
    assert!(
      request_headers(&mut socket, "2001:db8:1::1")
        .await
        .starts_with(b"HTTP/1.1 200")
    );
    held.push(socket);
  }
  let mut rejected = TcpStream::connect(gateway.address()).await.unwrap();
  assert!(
    request_headers(&mut rejected, "2001:db8:1::2")
      .await
      .starts_with(b"HTTP/1.1 503")
  );
  let metrics = reqwest::get(format!(
    "http://{}/.well-known/test-pow/api/metrics",
    gateway.operations_address()
  ))
  .await
  .unwrap()
  .text()
  .await
  .unwrap();
  assert!(
    metrics.contains("rilgate_connections_rejected_total 1"),
    "{metrics}"
  );
  let mut other = TcpStream::connect(gateway.address()).await.unwrap();
  assert!(
    request_headers(&mut other, "2001:db8:2::1")
      .await
      .starts_with(b"HTTP/1.1 200")
  );
  // Reusing a proxy connection transfers its idle network slot.
  assert!(
    request_headers(&mut held[0], "2001:db8:3::1")
      .await
      .starts_with(b"HTTP/1.1 200")
  );
  let mut returned = TcpStream::connect(gateway.address()).await.unwrap();
  assert!(
    request_headers(&mut returned, "2001:db8:1::3")
      .await
      .starts_with(b"HTTP/1.1 200")
  );
}

#[tokio::test]
async fn partial_headers_cannot_extend_the_deadline_with_more_bytes() {
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().server.header_timeout = Duration::from_millis(200);
  let gateway = app::router(cfg).await.unwrap();
  let mut socket = TcpStream::connect(gateway.address()).await.unwrap();
  socket.write_all(b"GET /").await.unwrap();
  for _ in 0..10 {
    tokio::time::sleep(Duration::from_millis(50)).await;
    if socket.write_all(b"x").await.is_err() {
      break;
    }
  }
  let result = tokio::time::timeout(Duration::from_millis(100), socket.read(&mut [0; 1])).await;
  assert!(
    matches!(result, Ok(Ok(0)) | Ok(Err(_))),
    "partial header extended its deadline"
  );
}

#[tokio::test]
async fn http2_preface_without_request_headers_has_a_deadline() {
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().server.header_timeout = Duration::from_millis(200);
  let gateway = app::router(cfg).await.unwrap();
  let mut socket = TcpStream::connect(gateway.address()).await.unwrap();
  socket
    .write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n\x00\x00\x00\x04\x00\x00\x00\x00\x00")
    .await
    .unwrap();
  let result =
    tokio::time::timeout(Duration::from_secs(2), socket.read_to_end(&mut Vec::new())).await;
  assert!(
    result.is_ok(),
    "HTTP/2 connection remained open without request headers"
  );
}

#[tokio::test]
async fn configured_header_deadlines_can_exceed_one_minute() {
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().server.header_timeout = Duration::from_secs(70);
  let gateway = app::router(cfg).await.unwrap();
  let mut socket = TcpStream::connect(gateway.address()).await.unwrap();
  socket
    .write_all(b"HEAD /.well-known/test-pow/api/client-config HTTP/1.1\r\nHost: example.test\r\n")
    .await
    .unwrap();
  tokio::time::sleep(Duration::from_secs(61)).await;
  socket.write_all(b"\r\n").await.unwrap();
  let mut response = [0; 4096];
  let size = tokio::time::timeout(Duration::from_secs(2), socket.read(&mut response))
    .await
    .unwrap()
    .unwrap();
  assert!(
    response[..size].starts_with(b"HTTP/1.1 200"),
    "header closed before its configured deadline"
  );
}
