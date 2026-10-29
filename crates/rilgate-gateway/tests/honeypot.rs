mod common;
use rilgate_config::{
  ChallengePolicy, PowAlgorithm,
  schema::policy::{Action, Condition, HoneypotDefinition, Rule},
};
use rilgate_test_utils::gateway as app;
use std::sync::Arc;
use tokio::net::TcpListener;

const BASE: &str = "/.well-known/test-pow";

struct Fixture {
  client: reqwest::Client,
  address: std::net::SocketAddr,
  operations_address: std::net::SocketAddr,
  _gateway: app::TestGateway,
  upstream: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
  fn drop(&mut self) {
    self.upstream.abort();
  }
}

async fn fixture(trust_proxy: bool, refresh: bool) -> Fixture {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.upstream.url = format!("http://{}", listener.local_addr().unwrap());
  config.server.metrics_enabled = true;
  if trust_proxy {
    config.server.trusted_proxies =
      vec!["::1/128".parse().unwrap(), "127.0.0.1/32".parse().unwrap()];
  }
  config.pow.policy.honeypot = Some(HoneypotDefinition::default());
  let rules = config.pow.policy.rules.as_mut().unwrap();
  if refresh {
    rules[0].challenge = Some(ChallengePolicy {
      algorithm: Some(PowAlgorithm::Refresh),
      difficulty: Some(1),
    });
  }
  rules.insert(
    0,
    Rule {
      name: "trapped".into(),
      when: Condition {
        honeypot: Some(true),
        ..Default::default()
      },
      action: Action::Deny,
      score: None,
      challenge: None,
    },
  );
  let upstream = tokio::spawn(async move {
    let router = axum::Router::new().fallback(|| async { "upstream-ok" });
    axum::serve(listener, router).await.unwrap();
  });
  let gateway = app::router(cfg).await.unwrap();
  Fixture {
    client: reqwest::Client::new(),
    address: gateway.address(),
    operations_address: gateway.operations_address(),
    _gateway: gateway,
    upstream,
  }
}

impl Fixture {
  async fn get(&self, path: &str, forwarded_for: Option<&str>) -> reqwest::Response {
    let mut request = self
      .client
      .get(format!("http://{}{path}", self.address))
      .header("host", "example.test")
      .header("accept", "text/html");
    if let Some(value) = forwarded_for {
      request = request.header("x-forwarded-for", value);
    }
    request.send().await.unwrap()
  }

  /// Returns the honeypot link of the challenge page for a protected path.
  async fn trap_url(&self) -> String {
    let body = self.get("/protected", None).await.text().await.unwrap();
    let attributes = r#"" rel="nofollow" hidden aria-hidden="true" tabindex="-1"></a>"#;
    let end = body.find(attributes).expect("honeypot link");
    let start = body[..end].rfind(r#"<a href=""#).unwrap() + r#"<a href=""#.len();
    body[start..end].replace("&#x2f;", "/")
  }
}

#[tokio::test]
async fn challenge_and_refresh_pages_link_the_trap() {
  for refresh in [false, true] {
    let fixture = fixture(false, refresh).await;
    let first = fixture.trap_url().await;
    let hex = first
      .strip_prefix(&format!("{BASE}/trap/"))
      .expect("trap path under the internal prefix");
    assert_eq!(hex.len(), 32, "{first}");
    assert!(hex.bytes().all(|byte| byte.is_ascii_hexdigit()), "{first}");
    assert_ne!(first, fixture.trap_url().await);
  }
}

#[tokio::test]
async fn trap_hit_flags_the_client() {
  let fixture = fixture(false, false).await;
  assert_eq!(fixture.get("/page", None).await.status(), 200);
  let trap = fixture.trap_url().await;
  assert_eq!(fixture.get(&trap, None).await.status(), 404);
  assert_eq!(fixture.get("/page", None).await.status(), 403);
  let metrics = fixture
    .client
    .get(format!(
      "http://{}{BASE}/api/metrics",
      fixture.operations_address
    ))
    .send()
    .await
    .unwrap()
    .text()
    .await
    .unwrap();
  assert!(
    metrics.contains("rilgate_honeypot_hits_total 1"),
    "{metrics}"
  );
}

#[tokio::test]
async fn ipv6_trap_hit_flags_the_slash_64() {
  let fixture = fixture(true, false).await;
  let trap = fixture.trap_url().await;
  assert_eq!(
    fixture.get(&trap, Some("2001:db8:1:2::5")).await.status(),
    404
  );
  let status = |forwarded_for| {
    let fixture = &fixture;
    async move { fixture.get("/page", Some(forwarded_for)).await.status() }
  };
  assert_eq!(status("2001:db8:1:2::9").await, 403);
  assert_eq!(status("2001:db8:1:3::9").await, 200);
}

#[tokio::test]
async fn trap_route_without_a_honeypot_is_unknown() {
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().pow.policy.rules = None;
  let gateway = app::router(cfg).await.unwrap();
  let response = reqwest::get(format!("http://{}{BASE}/trap/abc", gateway.address()))
    .await
    .unwrap();
  assert_eq!(response.status(), 404);
}
