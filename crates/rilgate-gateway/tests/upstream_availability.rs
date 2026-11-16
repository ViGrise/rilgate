mod common;

use rilgate_test_utils::gateway;
use std::{
  io::Write,
  sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
  fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
    self.0.lock().unwrap().extend_from_slice(bytes);
    Ok(bytes.len())
  }
  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}

fn events(capture: &Capture, message: &str) -> Vec<serde_json::Value> {
  String::from_utf8(capture.0.lock().unwrap().clone())
    .unwrap()
    .lines()
    .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
    .filter(|event| event["fields"]["message"] == message)
    .collect()
}

#[tokio::test]
async fn an_upstream_outage_logs_one_warning_and_one_recovery_at_info() {
  let capture = Capture::default();
  let writer = capture.clone();
  tracing::subscriber::set_global_default(
    tracing_subscriber::fmt()
      .json()
      .with_writer(move || writer.clone())
      .with_env_filter("rilgate=info,pingora=error")
      .finish(),
  )
  .unwrap();
  let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
  let upstream_address = reserved.local_addr().unwrap();
  drop(reserved);
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.upstream.url = format!("http://{upstream_address}");
  config.pow.policy.policy.rules = Some(vec![]);
  let app = gateway::router(cfg).await.unwrap();
  let url = format!("http://127.0.0.1:{}/page", app.address().port());
  let client = reqwest::Client::new();

  for _ in 0..3 {
    let response = client.get(&url).send().await.unwrap();
    assert_eq!(response.status(), 502);
  }
  let warnings = events(&capture, "Upstream requests are failing");
  assert_eq!(warnings.len(), 1, "{warnings:?}");
  assert_eq!(warnings[0]["level"], "WARN");
  assert_eq!(warnings[0]["fields"]["status"], 502);

  let listener = tokio::net::TcpListener::bind(upstream_address)
    .await
    .unwrap();
  let upstream = tokio::spawn(async move {
    axum::serve(listener, axum::Router::new().fallback(|| async { "ok" }))
      .await
      .unwrap();
  });
  for _ in 0..2 {
    let response = client.get(&url).send().await.unwrap();
    assert_eq!(response.text().await.unwrap(), "ok");
  }
  let recoveries = events(&capture, "Upstream requests succeed again");
  assert_eq!(recoveries.len(), 1, "{recoveries:?}");
  assert_eq!(recoveries[0]["level"], "INFO");
  assert_eq!(recoveries[0]["fields"]["failures"], 3);
  upstream.abort();
}
