use rilgate_test_utils::gateway as app;
mod common;

use axum::{
  Router,
  body::{Body, Bytes, to_bytes},
  http::{Request, StatusCode},
  routing::any,
};
use futures_util::StreamExt;
use rilgate_gateway::config::Config;
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tower::ServiceExt;

async fn upstream(limit: usize) -> (Config, mpsc::Receiver<Bytes>, tokio::task::JoinHandle<()>) {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let (seen, received) = mpsc::channel(10);
  let router = Router::new().route(
    "/upload",
    any(move |body: Body| {
      let seen = seen.clone();
      async move {
        let mut stream = body.into_data_stream();
        let mut all = Vec::new();
        while let Some(frame) = stream.next().await {
          match frame {
            Ok(bytes) => {
              all.extend_from_slice(&bytes);
              seen.send(bytes).await.unwrap();
            }
            Err(_) => return (StatusCode::BAD_REQUEST, all),
          }
        }
        (StatusCode::OK, all)
      }
    }),
  );
  let task = tokio::spawn(async move {
    axum::serve(listener, router).await.unwrap();
  });
  let mut cfg = common::config();
  let configuration = Arc::get_mut(&mut cfg).unwrap();
  configuration.upstream.url = format!("http://{address}");
  configuration.upstream.max_body_bytes = limit;
  (cfg, received, task)
}

fn body(receiver: mpsc::Receiver<Bytes>) -> Body {
  Body::from_stream(futures_util::stream::unfold(
    receiver,
    |mut receiver| async move {
      receiver
        .recv()
        .await
        .map(|bytes| (Ok::<_, std::io::Error>(bytes), receiver))
    },
  ))
}

#[tokio::test]
async fn unknown_length_get_body_reaches_upstream_before_upload_is_complete() {
  let (cfg, mut seen, upstream) = upstream(1024).await;
  let gateway = app::router(cfg).await.unwrap();
  let (send, receive) = mpsc::channel(1);
  let response = tokio::spawn(async move {
    gateway
      .oneshot(
        Request::builder()
          .uri("/upload")
          .method("GET")
          .body(body(receive))
          .unwrap(),
      )
      .await
      .unwrap()
  });
  send.send(Bytes::from_static(b"first")).await.unwrap();
  assert_eq!(
    tokio::time::timeout(Duration::from_secs(2), seen.recv())
      .await
      .expect("upstream must receive data before the upload finishes")
      .unwrap(),
    "first"
  );
  assert!(!response.is_finished());
  send.send(Bytes::from_static(b"second")).await.unwrap();
  drop(send);
  let response = response.await.unwrap();
  assert_eq!(response.status(), StatusCode::OK);
  assert_eq!(
    to_bytes(response.into_body(), 100).await.unwrap(),
    "firstsecond"
  );
  upstream.abort();
}

#[tokio::test]
async fn a_chunked_upload_cannot_forward_more_than_the_limit() {
  let (cfg, mut seen, upstream) = upstream(8).await;
  let gateway = app::router(cfg).await.unwrap();
  let (send, receive) = mpsc::channel(1);
  let response = tokio::spawn(async move {
    gateway
      .oneshot(
        Request::builder()
          .uri("/upload")
          .method("POST")
          .body(body(receive))
          .unwrap(),
      )
      .await
      .unwrap()
  });
  send.send(Bytes::from_static(b"1234")).await.unwrap();
  assert_eq!(
    tokio::time::timeout(Duration::from_secs(2), seen.recv())
      .await
      .unwrap()
      .unwrap(),
    "1234"
  );
  send.send(Bytes::from_static(b"56789")).await.unwrap();
  drop(send);
  let response = tokio::time::timeout(Duration::from_secs(2), response)
    .await
    .unwrap()
    .unwrap();
  assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
  assert!(seen.try_recv().is_err());
  upstream.abort();
}

#[tokio::test]
async fn declared_oversized_upload_is_rejected_without_contacting_upstream() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().upstream.url = format!("http://{address}");
  let gateway = app::router(cfg).await.unwrap();
  let (_send, receive) = mpsc::channel(1);
  let response = gateway
    .oneshot(
      Request::builder()
        .uri("/upload")
        .method("POST")
        .header("content-length", "1025")
        .body(body(receive))
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
  assert!(
    tokio::time::timeout(Duration::from_millis(20), listener.accept())
      .await
      .is_err()
  );
}

#[tokio::test]
async fn an_upload_at_the_exact_limit_preserves_its_body() {
  let (cfg, _seen, upstream) = upstream(8).await;
  let response = app::router(cfg)
    .await
    .unwrap()
    .oneshot(
      Request::builder()
        .uri("/upload")
        .method("PUT")
        .header("content-length", "8")
        .body(Body::from("12345678"))
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::OK);
  assert_eq!(
    to_bytes(response.into_body(), 100).await.unwrap(),
    "12345678"
  );
  upstream.abort();
}
