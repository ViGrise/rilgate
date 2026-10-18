use axum::{
  Router,
  body::Body,
  http::{HeaderMap, StatusCode, header},
  response::{IntoResponse, Redirect, Response},
  routing::get,
};
use bytes::Bytes;
use futures_util::stream;
use rilgate_ip_feeds::{FeedClient, Fetched, MAX_BODY_BYTES, Validators};

async fn feed(headers: HeaderMap) -> Response {
  if headers
    .get(header::IF_NONE_MATCH)
    .and_then(|value| value.to_str().ok())
    == Some("\"v1\"")
  {
    return StatusCode::NOT_MODIFIED.into_response();
  }
  let mut response = r#"{"prefixes":[{"ipv4Prefix":"192.0.2.0/24"}]}"#
    .to_string()
    .into_response();
  response
    .headers_mut()
    .insert(header::ETAG, "\"v1\"".parse().unwrap());
  response.headers_mut().insert(
    header::LAST_MODIFIED,
    "Fri, 16 Oct 2026 14:46:43 GMT".parse().unwrap(),
  );
  response
}

async fn big() -> Response {
  let chunk = vec![b'x'; MAX_BODY_BYTES + 1];
  Body::from_stream(stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
    chunk,
  ))]))
  .into_response()
}

async fn status() -> Response {
  StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

async fn redirect_loop() -> Response {
  Redirect::temporary("/loop").into_response()
}

async fn redirect_scheme(headers: HeaderMap) -> Response {
  let host = headers
    .get(header::HOST)
    .and_then(|value| value.to_str().ok())
    .unwrap_or("127.0.0.1");
  Redirect::temporary(&format!("https://{host}/feed")).into_response()
}

async fn hop1() -> Redirect {
  Redirect::temporary("/hop/2")
}

async fn hop2() -> Redirect {
  Redirect::temporary("/hop/3")
}

async fn hop3() -> Redirect {
  Redirect::temporary("/feed")
}

async fn serve() -> String {
  let app = Router::new()
    .route("/feed", get(feed))
    .route("/big", get(big))
    .route("/status", get(status))
    .route("/loop", get(redirect_loop))
    .route("/scheme", get(redirect_scheme))
    .route("/hop/1", get(hop1))
    .route("/hop/2", get(hop2))
    .route("/hop/3", get(hop3));
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  tokio::spawn(async move {
    axum::serve(listener, app).await.unwrap();
  });
  format!("http://{address}")
}

#[tokio::test]
async fn validators_are_sent_and_not_modified_is_reported() {
  let base = serve().await;
  let url = format!("{base}/feed");
  let client = FeedClient::new().unwrap();
  let Fetched::Modified { validators, .. } =
    client.fetch(&url, &Validators::default()).await.unwrap()
  else {
    panic!("expected a modified response");
  };
  assert_eq!(validators.etag.as_ref().unwrap(), "\"v1\"");
  assert!(validators.last_modified.is_some());
  assert!(matches!(
    client.fetch(&url, &validators).await.unwrap(),
    Fetched::NotModified
  ));
}

#[tokio::test]
async fn oversized_failed_and_redirected_responses_are_errors() {
  let base = serve().await;
  let client = FeedClient::new().unwrap();
  for (path, message) in [
    ("/big", "exceeds 4194304 bytes"),
    ("/status", "500"),
    ("/loop", "too many redirects"),
    ("/scheme", "redirect changes scheme"),
  ] {
    let error = format!(
      "{:#}",
      client
        .fetch(&format!("{base}{path}"), &Validators::default())
        .await
        .unwrap_err()
    );
    assert!(error.contains(message), "{path}: {error}");
  }
  assert!(
    client
      .fetch(&format!("{base}/hop/1"), &Validators::default())
      .await
      .is_ok()
  );
}
