mod common;
use futures_util::{SinkExt, StreamExt};
use rilgate_test_utils::gateway as app;
use std::{sync::Arc, time::Duration};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::{TcpListener, TcpStream},
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::test]
async fn application_websocket_uses_the_same_policy_gate() {
  use axum::{Router, extract::WebSocketUpgrade, routing::get};
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().upstream.url =
    format!("http://{}", listener.local_addr().unwrap());
  let upstream = tokio::spawn(async move {
    let router = Router::new().route(
      "/echo",
      get(|upgrade: WebSocketUpgrade| async {
        upgrade.on_upgrade(|mut socket| async move {
          while let Some(Ok(message)) = socket.recv().await {
            if socket.send(message).await.is_err() {
              break;
            }
          }
        })
      }),
    );
    axum::serve(listener, router).await.unwrap();
  });
  let gateway = app::router(cfg).await.unwrap();
  let (mut socket, _) = connect_async(format!("ws://{}/echo", gateway.address()))
    .await
    .unwrap();
  socket.send(Message::Text("hello".into())).await.unwrap();
  let echoed = tokio::time::timeout(Duration::from_secs(2), socket.next())
    .await
    .unwrap()
    .unwrap()
    .unwrap();
  assert_eq!(echoed.into_text().unwrap(), "hello");
  let rejected = connect_async(format!("ws://{}/protected", gateway.address()))
    .await
    .unwrap_err();
  assert!(
    matches!(rejected, tokio_tungstenite::tungstenite::Error::Http(response) if response.status() == http::StatusCode::OK)
  );
  socket.close(None).await.unwrap();
  upstream.abort();
}

#[tokio::test]
async fn truncated_upload_returns_a_client_error() {
  let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().upstream.url =
    format!("http://{}", upstream.local_addr().unwrap());
  let origin = tokio::spawn(async move {
    let (mut socket, _) = upstream.accept().await.unwrap();
    let mut bytes = Vec::new();
    let _ = socket.read_to_end(&mut bytes).await;
  });
  let gateway = app::router(cfg).await.unwrap();
  let mut socket = TcpStream::connect(gateway.address()).await.unwrap();
  socket
    .write_all(b"POST /upload HTTP/1.1\r\nHost: example.test\r\nContent-Length: 10\r\n\r\nx")
    .await
    .unwrap();
  socket.shutdown().await.unwrap();
  let mut response = Vec::new();
  tokio::time::timeout(Duration::from_secs(2), socket.read_to_end(&mut response))
    .await
    .unwrap()
    .unwrap();
  assert!(
    response.starts_with(b"HTTP/1.1 400"),
    "{}",
    String::from_utf8_lossy(&response)
  );
  origin.abort();
}

#[tokio::test]
async fn shutdown_closes_idle_connections_and_releases_the_listener() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let operations = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let operations_address = operations.local_addr().unwrap();
  let gateway = app::build(common::config()).await.unwrap();
  let (stop, stopped) = tokio::sync::oneshot::channel();
  let server = tokio::spawn(gateway.serve(vec![listener], vec![operations], async {
    let _ = stopped.await;
  }));
  let mut socket = TcpStream::connect(operations_address).await.unwrap();
  socket
    .write_all(b"GET /.well-known/test-pow/api/health HTTP/1.1\r\nHost: example.test\r\n\r\n")
    .await
    .unwrap();
  let mut buffer = [0; 4096];
  let read = socket.read(&mut buffer).await.unwrap();
  assert!(buffer[..read].starts_with(b"HTTP/1.1 200"));
  stop.send(()).unwrap();
  tokio::time::timeout(Duration::from_secs(2), server)
    .await
    .unwrap()
    .unwrap()
    .unwrap();
  assert!(TcpStream::connect(address).await.is_err());
  assert!(TcpStream::connect(operations_address).await.is_err());
}

#[tokio::test]
async fn ambiguous_request_headers_are_rejected_before_forwarding() {
  let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().upstream.url =
    format!("http://{}", upstream.local_addr().unwrap());
  let gateway = app::router(cfg).await.unwrap();
  for request in [
    &b"GET / HTTP/1.1\r\nHost: first.test\r\nHost: second.test\r\nConnection: close\r\n\r\n"[..],
    &b"POST / HTTP/1.1\r\nHost: example.test\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n0\r\n\r\n"[..],
  ] {
    let mut socket = TcpStream::connect(gateway.address()).await.unwrap();
    socket.write_all(request).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), socket.read_to_end(&mut response)).await.unwrap().unwrap();
    assert!(response.starts_with(b"HTTP/1.1 400"), "{}", String::from_utf8_lossy(&response));
  }
  assert!(
    tokio::time::timeout(Duration::from_millis(20), upstream.accept())
      .await
      .is_err()
  );
}

#[tokio::test]
async fn hostname_upstreams_can_connect_using_their_available_address_family() {
  use axum::{Router, routing::get};
  use tower::ServiceExt;
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().upstream.url =
    format!("http://localhost:{}", listener.local_addr().unwrap().port());
  let upstream = tokio::spawn(async move {
    axum::serve(
      listener,
      Router::new().route("/", get(|| async { "origin" })),
    )
    .await
    .unwrap();
  });
  let gateway = app::router(cfg).await.unwrap();
  let response = gateway
    .oneshot(
      http::Request::builder()
        .uri("/")
        .body(axum::body::Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), http::StatusCode::OK);
  assert_eq!(
    axum::body::to_bytes(response.into_body(), 100)
      .await
      .unwrap(),
    "origin"
  );
  upstream.abort();
}

#[tokio::test]
async fn public_listeners_hide_operational_routes_without_operations_configured() {
  let mut listeners = vec![TcpListener::bind("127.0.0.1:0").await.unwrap()];
  if let Ok(listener) = TcpListener::bind("[::1]:0").await {
    listeners.push(listener);
  }
  let addresses: Vec<_> = listeners.iter().map(|l| l.local_addr().unwrap()).collect();
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().server.metrics_enabled = true;
  assert!(cfg.operations_listen_addresses.is_empty());
  let gateway = app::build(cfg).await.unwrap();
  let (stop, stopped) = tokio::sync::oneshot::channel();
  let server = tokio::spawn(gateway.serve(listeners, vec![], async {
    let _ = stopped.await;
  }));
  let client = reqwest::Client::new();
  for address in addresses {
    for route in ["live", "health", "ready", "metrics"] {
      let response = client
        .get(format!("http://{address}/.well-known/test-pow/api/{route}"))
        .send()
        .await
        .unwrap();
      assert_eq!(response.status(), 404, "{address} {route}");
      assert_eq!(response.headers()["cache-control"], "no-store");
    }
    assert_eq!(
      client
        .get(format!(
          "http://{address}/.well-known/test-pow/api/client-config"
        ))
        .send()
        .await
        .unwrap()
        .status(),
      200
    );
  }
  stop.send(()).unwrap();
  server.await.unwrap().unwrap();
}
