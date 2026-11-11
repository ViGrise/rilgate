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
  config.pow.policy.signals.honeypot = Some(HoneypotDefinition::default());
  let rules = config.pow.policy.policy.rules.as_mut().unwrap();
  if refresh {
    rules[0].challenge = Some(ChallengePolicy {
      hook: None,
      steps: None,
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
    self.trap_url_for(None).await
  }

  async fn trap_url_for(&self, ip: Option<&str>) -> String {
    let body = self.get("/protected", ip).await.text().await.unwrap();
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
    first
      .strip_prefix(&format!("{BASE}/trap/"))
      .expect("trap path under the internal prefix");
    assert_ne!(first, fixture.trap_url().await);
    assert_eq!(fixture.get(&first, None).await.status(), 404);
    assert_eq!(fixture.get("/page", None).await.status(), 403);
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
  let trap = fixture.trap_url_for(Some("2001:db8:1:2::5")).await;
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
  Arc::get_mut(&mut cfg).unwrap().pow.policy.policy.rules = None;
  let gateway = app::router(cfg).await.unwrap();
  let response = reqwest::get(format!("http://{}{BASE}/trap/abc", gateway.address()))
    .await
    .unwrap();
  assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn unverified_cross_site_trap_cannot_deny_a_visitor() {
  let fixture = fixture(true, false).await;
  assert_eq!(
    fixture.get("/protected", Some("192.0.2.1")).await.status(),
    200
  );
  let response = fixture
    .client
    .get(format!("http://{}{BASE}/trap/x", fixture.address))
    .header("host", "example.test")
    .header("x-forwarded-for", "192.0.2.1")
    .header("sec-fetch-site", "cross-site")
    .send()
    .await
    .unwrap();
  assert_eq!(response.status(), 404);
  assert_eq!(
    fixture.get("/protected", Some("192.0.2.1")).await.status(),
    200
  );
  assert_eq!(
    fixture.get("/protected", Some("192.0.2.2")).await.status(),
    200
  );
}

#[tokio::test]
async fn issued_link_cannot_flag_another_network() {
  let fixture = fixture(true, false).await;
  let trap = fixture.trap_url().await;
  assert_eq!(fixture.get(&trap, Some("192.0.2.2")).await.status(), 404);
  assert_eq!(
    fixture.get("/protected", Some("192.0.2.2")).await.status(),
    200
  );
}

#[tokio::test]
async fn signed_links_reject_cross_site_and_expired_requests() {
  let fixture = fixture(true, false).await;
  let trap = fixture.trap_url_for(Some("192.0.2.1")).await;
  assert_eq!(
    fixture
      .client
      .get(format!("http://{}{trap}", fixture.address))
      .header("host", "example.test")
      .header("x-forwarded-for", "192.0.2.1")
      .header("sec-fetch-site", "cross-site")
      .send()
      .await
      .unwrap()
      .status(),
    404
  );
  assert_eq!(
    fixture.get("/protected", Some("192.0.2.1")).await.status(),
    200
  );
  let cfg = common::config();
  let segment = rilgate_crypto::token::sign_json(
    &serde_json::json!({
      "network": "192.0.2.1", "expires_at": 1, "nonce": "expired"
    }),
    cfg.pow.signing_key.expose().as_bytes(),
    rilgate_crypto::token::TokenPurpose::Honeypot,
  )
  .unwrap();
  assert_eq!(
    fixture
      .get(&format!("{BASE}/trap/{segment}"), Some("192.0.2.1"))
      .await
      .status(),
    404
  );
  assert_eq!(
    fixture.get("/protected", Some("192.0.2.1")).await.status(),
    200
  );
}
