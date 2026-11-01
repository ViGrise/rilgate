mod common;

use rilgate_config::{
  DecisionLog,
  schema::policy::{Action, Condition, Rule, StringMatch},
};
use rilgate_crypto::token::{self, TokenPurpose};
use rilgate_protocol::{PowAlgorithm, PowEnforcement, claims::ReceiptClaims};
use rilgate_test_utils::gateway;
use std::{
  io::Write,
  sync::{Arc, Mutex},
  time::{SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

#[tokio::test]
async fn challenge_and_deny_decisions_are_logged_with_the_client_ip() {
  let capture = Capture::default();
  let writer = capture.clone();
  let subscriber = tracing_subscriber::fmt()
    .json()
    .with_writer(move || writer.clone())
    .with_env_filter("info")
    .with_filter_reloading();
  let filter = subscriber.reload_handle();
  tracing::subscriber::set_global_default(subscriber.finish()).unwrap();
  let upstream_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let upstream_address = upstream_listener.local_addr().unwrap();
  let upstream = tokio::spawn(async move {
    axum::serve(
      upstream_listener,
      axum::Router::new().fallback(|| async { "upstream-ok" }),
    )
    .await
    .unwrap();
  });
  for (level, actions) in [
    (DecisionLog::Off, vec![]),
    (DecisionLog::Deny, vec!["deny"]),
    (
      DecisionLog::Challenge,
      vec!["deny", "challenge", "challenge"],
    ),
    (
      DecisionLog::All,
      vec!["deny", "challenge", "challenge", "allow", "allow"],
    ),
  ] {
    let mut cfg = common::config();
    let config = Arc::get_mut(&mut cfg).unwrap();
    config.server.decision_log = level;
    config.upstream.url = format!("http://{upstream_address}");
    config.pow.policy.policy.rules.as_mut().unwrap().insert(
      0,
      Rule {
        name: "bad-bot".into(),
        when: Condition {
          user_agent: Some(StringMatch {
            contains: Some("BadBot".into()),
            ..Default::default()
          }),
          ..Default::default()
        },
        action: Action::Deny,
        score: None,
        challenge: None,
      },
    );
    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_secs() as i64;
    let receipt = token::sign_json(
      &ReceiptClaims {
        host: "example.test".into(),
        v: 1,
        challenge_id: "proof".into(),
        algorithm: PowAlgorithm::Fast,
        difficulty: 1,
        enforce: PowEnforcement::LocalVerify,
        issued_at: now,
        expires_at: now + 30,
      },
      config.pow.signing_key.expose().as_bytes(),
      TokenPurpose::Receipt,
    )
    .unwrap();
    let app = gateway::router(cfg).await.unwrap();
    let url = format!("http://127.0.0.1:{}", app.address().port());
    let client = reqwest::Client::new();
    let exchanged = client
      .post(format!("{url}/.well-known/test-pow/api/exchange"))
      .header("host", "example.test")
      .header("content-type", "application/json")
      .body(serde_json::json!({"receipt": receipt}).to_string())
      .send()
      .await
      .unwrap();
    assert_eq!(exchanged.status(), 200);
    let cookie = exchanged.headers()["set-cookie"]
      .to_str()
      .unwrap()
      .split(';')
      .next()
      .unwrap()
      .to_owned();
    capture.0.lock().unwrap().clear();
    for (method, path, bad_bot, cookie) in [
      (reqwest::Method::GET, "/protected/denied", true, None),
      (reqwest::Method::GET, "/protected/challenge", false, None),
      (reqwest::Method::POST, "/protected/post", false, None),
      (reqwest::Method::GET, "/public", false, None),
      (
        reqwest::Method::GET,
        "/protected/cookie",
        false,
        Some(cookie.as_str()),
      ),
    ] {
      let mut request = client
        .request(method, format!("{url}{path}"))
        .header("host", "example.test");
      if bad_bot {
        request = request.header("user-agent", "BadBot");
      }
      if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
      }
      let response = request.send().await.unwrap();
      let status = response.status();
      let body = response.text().await.unwrap();
      match path {
        "/protected/denied" => assert_eq!(status, 403),
        "/protected/post" => assert_eq!(status, 401),
        "/public" | "/protected/cookie" => assert_eq!(body, "upstream-ok"),
        _ => assert!(body.contains("html")),
      }
    }
    let output = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    let decisions: Vec<serde_json::Value> = output
      .lines()
      .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
      .filter(|event| event["fields"].get("action").is_some())
      .collect();
    let logged_actions: Vec<_> = decisions
      .iter()
      .map(|event| event["fields"]["action"].as_str().unwrap())
      .collect();
    assert_eq!(logged_actions, actions, "{level:?}: {output}");
    for event in &decisions {
      let fields = &event["fields"];
      for name in [
        "request_id",
        "client_ip",
        "method",
        "host",
        "path",
        "action",
        "rule",
        "weight",
        "algorithm",
        "difficulty",
        "unresolved",
      ] {
        assert!(fields.get(name).is_some(), "Missing {name}: {event}");
      }
      assert_eq!(fields["client_ip"], "127.0.0.1");
      assert_eq!(fields["host"], "example.test");
      assert!(!fields["request_id"].as_str().unwrap().is_empty());
      if fields["action"] == "deny" {
        assert_eq!(fields["rule"], "bad-bot");
      }
    }
    if matches!(level, DecisionLog::All) {
      assert_eq!(
        decisions.last().unwrap()["fields"]["path"],
        "/protected/cookie"
      );
      filter
        .reload(tracing_subscriber::EnvFilter::new("debug"))
        .unwrap();
      capture.0.lock().unwrap().clear();
      for target in [
        format!("{url}/.well-known/test-pow/api/live"),
        format!(
          "http://127.0.0.1:{}/outside",
          app.operations_address().port()
        ),
      ] {
        client
          .get(target)
          .send()
          .await
          .unwrap()
          .bytes()
          .await
          .unwrap();
      }
      let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", app.address().port()))
        .await
        .unwrap();
      stream
        .write_all(
          b"GET /public/../protected HTTP/1.1\r\nHost: example.test\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
      let mut response = Vec::new();
      stream.read_to_end(&mut response).await.unwrap();
      assert!(response.starts_with(b"HTTP/1.1 400"));
      let output = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
      let completions: Vec<serde_json::Value> = output
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|event| event["fields"]["message"] == "HTTP request completed")
        .filter(|event| {
          matches!(
            event["fields"]["path"].as_str(),
            Some("/.well-known/test-pow/api/live" | "/outside" | "/public/../protected")
          )
        })
        .collect();
      assert_eq!(completions.len(), 3, "{output}");
      for event in completions {
        assert_eq!(event["fields"]["client_ip"], "Some(127.0.0.1)", "{event}");
      }
    }
  }
  upstream.abort();
}
