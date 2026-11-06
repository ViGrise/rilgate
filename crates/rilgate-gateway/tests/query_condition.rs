mod common;

use rilgate_config::schema::policy::{Action, Condition, Rule, StringMatch};
use rilgate_test_utils::gateway as app;
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::test]
async fn gateway_evaluates_the_request_query_string() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.upstream.url = format!("http://{}", listener.local_addr().unwrap());
  let rules = config.pow.policy.policy.rules.as_mut().unwrap();
  rules.insert(
    0,
    Rule {
      name: "module-metadata".into(),
      when: Condition {
        query: [(
          "go-get".to_string(),
          StringMatch {
            exact: Some("1".into()),
            ..Default::default()
          },
        )]
        .into(),
        ..Default::default()
      },
      action: Action::Allow,
      score: None,
      challenge: None,
    },
  );
  let upstream = tokio::spawn(async move {
    let router = axum::Router::new().fallback(|| async { "upstream-ok" });
    axum::serve(listener, router).await.unwrap();
  });
  let gateway = app::router(cfg).await.unwrap();
  let client = reqwest::Client::new();
  for (target, forwarded) in [
    ("/protected?go-get=1", true),
    ("/protected?go%2Dget=1", true),
    ("/protected?go-get=0", false),
    ("/protected", false),
  ] {
    let body = client
      .get(format!("http://{}{target}", gateway.address()))
      .send()
      .await
      .unwrap()
      .text()
      .await
      .unwrap();
    assert_eq!(body == "upstream-ok", forwarded, "{target}");
  }
  upstream.abort();
}
