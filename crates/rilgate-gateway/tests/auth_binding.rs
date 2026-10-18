mod common;
use rilgate_challenge::authorization::binding::{
  AuthBinding, BindingInput, BindingMismatch, ClientIpBinding, UserAgentBinding,
};
use rilgate_crypto::token::{self as crypto, TokenPurpose};
use rilgate_gateway::config::{PowAlgorithm, PowEnforcement};
use rilgate_protocol::claims::ReceiptClaims;
use rilgate_test_utils::{binding::unbound, gateway as app};
use std::{
  sync::Arc,
  time::{SystemTime, UNIX_EPOCH},
};
use tokio::net::TcpListener;

const CHROME_130: &str = "Mozilla/5.0 Chrome/130.0.6723.58 Safari/537.36";
const CHROME_131: &str = "Mozilla/5.0 Chrome/131.0.0.0 Safari/537.36";
const FIREFOX: &str = "Mozilla/5.0 (rv:132.0) Gecko/20100101 Firefox/132.0";

struct Fixture {
  client: reqwest::Client,
  address: std::net::SocketAddr,
  signing_key: String,
  _gateway: app::TestGateway,
  upstream: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
  fn drop(&mut self) {
    self.upstream.abort();
  }
}

async fn fixture(binding: AuthBinding, trust_peer: bool) -> Fixture {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.upstream.url = format!("http://{}", listener.local_addr().unwrap());
  config.server.metrics_enabled = true;
  config.pow.auth_binding = binding;
  if trust_peer {
    config.server.trusted_proxies =
      vec!["::1/128".parse().unwrap(), "127.0.0.1/32".parse().unwrap()];
  }
  let signing_key = config.pow.signing_key.expose().to_owned();
  let upstream = tokio::spawn(async move {
    let router = axum::Router::new().fallback(|| async { "upstream-ok" });
    axum::serve(listener, router).await.unwrap();
  });
  let gateway = app::router(cfg).await.unwrap();
  Fixture {
    client: reqwest::Client::new(),
    address: gateway.address(),
    signing_key,
    _gateway: gateway,
    upstream,
  }
}

impl Fixture {
  fn url(&self, path: &str) -> String {
    format!("http://{}{path}", self.address)
  }

  async fn solve(&self, headers: &[(&str, &str)]) -> String {
    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap()
      .as_secs() as i64;
    let receipt = crypto::sign_json(
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
      self.signing_key.as_bytes(),
      TokenPurpose::Receipt,
    )
    .unwrap();
    let mut request = self
      .client
      .post(self.url("/.well-known/test-pow/api/exchange"))
      .header("host", "example.test")
      .header("content-type", "application/json")
      .body(serde_json::json!({ "receipt": receipt }).to_string());
    for (name, value) in headers {
      request = request.header(*name, *value);
    }
    let response = request.send().await.unwrap();
    assert_eq!(response.status(), 200);
    let cookie = response.headers()["set-cookie"].to_str().unwrap();
    cookie.split(';').next().unwrap().to_owned()
  }

  /// Requests a protected page and reports whether the upstream served it.
  async fn reaches_upstream(&self, cookie: &str, headers: &[(&str, &str)]) -> bool {
    let mut request = self
      .client
      .get(self.url("/protected"))
      .header("host", "example.test")
      .header("cookie", cookie);
    for (name, value) in headers {
      request = request.header(*name, *value);
    }
    let response = request.send().await.unwrap();
    assert_eq!(response.status(), 200);
    let is_html = response.headers()["content-type"]
      .to_str()
      .unwrap()
      .starts_with("text/html");
    let body = response.text().await.unwrap();
    if body == "upstream-ok" {
      return true;
    }
    assert!(is_html, "{body}");
    false
  }

  async fn mismatches(&self) -> u64 {
    let text = self
      .client
      .get(self.url("/.well-known/test-pow/api/metrics"))
      .send()
      .await
      .unwrap()
      .text()
      .await
      .unwrap();
    text
      .lines()
      .find_map(|line| line.strip_prefix("rilgate_auth_binding_mismatches_total "))
      .unwrap_or_else(|| panic!("{text}"))
      .parse()
      .unwrap()
  }
}

#[tokio::test]
async fn cookies_bound_to_network_and_browser_family_reject_other_clients() {
  let binding = AuthBinding {
    client_ip: Some(ClientIpBinding::default()),
    user_agent: Some(UserAgentBinding::Family),
    ..Default::default()
  };
  let gateway = fixture(binding, true).await;
  let cookie = gateway
    .solve(&[
      ("x-forwarded-for", "203.0.113.10"),
      ("user-agent", CHROME_130),
    ])
    .await;

  let same_network_newer_browser = [
    ("x-forwarded-for", "203.0.113.200"),
    ("user-agent", CHROME_131),
  ];
  assert!(
    gateway
      .reaches_upstream(&cookie, &same_network_newer_browser)
      .await
  );
  assert_eq!(gateway.mismatches().await, 0);

  let other_network = [
    ("x-forwarded-for", "203.0.114.10"),
    ("user-agent", CHROME_130),
  ];
  assert!(!gateway.reaches_upstream(&cookie, &other_network).await);
  assert_eq!(gateway.mismatches().await, 1);

  let other_browser = [("x-forwarded-for", "203.0.113.10"), ("user-agent", FIREFOX)];
  assert!(!gateway.reaches_upstream(&cookie, &other_browser).await);
  assert_eq!(gateway.mismatches().await, 2);
}

#[tokio::test]
async fn cookies_are_portable_when_binding_is_not_configured() {
  let gateway = fixture(AuthBinding::default(), true).await;
  let cookie = gateway
    .solve(&[
      ("x-forwarded-for", "203.0.113.10"),
      ("user-agent", CHROME_130),
    ])
    .await;
  let elsewhere = [("x-forwarded-for", "198.51.100.7"), ("user-agent", FIREFOX)];
  assert!(gateway.reaches_upstream(&cookie, &elsewhere).await);
  assert_eq!(gateway.mismatches().await, 0);
}

#[tokio::test]
async fn proxy_headers_bind_only_when_the_peer_is_trusted() {
  let binding = AuthBinding {
    proxy_headers: vec!["x-ja4".into()],
    ..Default::default()
  };

  let trusted = fixture(binding.clone(), true).await;
  let cookie = trusted.solve(&[("x-ja4", "fingerprint-a")]).await;
  assert!(
    trusted
      .reaches_upstream(&cookie, &[("x-ja4", "fingerprint-a")])
      .await
  );
  assert!(
    !trusted
      .reaches_upstream(&cookie, &[("x-ja4", "fingerprint-b")])
      .await
  );
  assert_eq!(trusted.mismatches().await, 1);

  let untrusted = fixture(binding, false).await;
  let cookie = untrusted.solve(&[("x-ja4", "fingerprint-a")]).await;
  assert!(
    untrusted
      .reaches_upstream(&cookie, &[("x-ja4", "fingerprint-b")])
      .await
  );
  assert_eq!(untrusted.mismatches().await, 0);
}

#[tokio::test]
async fn service_stores_a_digest_only_when_binding_is_enabled() {
  let now = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap()
    .as_secs() as i64;
  let issue = |cfg: &rilgate_gateway::config::Config| {
    crypto::sign_json(
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
      cfg.pow.signing_key.expose().as_bytes(),
      TokenPurpose::Receipt,
    )
    .unwrap()
  };

  let mut bound_cfg = common::config();
  Arc::get_mut(&mut bound_cfg).unwrap().pow.auth_binding = AuthBinding {
    user_agent: Some(UserAgentBinding::Full),
    ..Default::default()
  };
  let bound = app::service(&bound_cfg).unwrap();
  let issued = bound
    .exchange_receipt_with_claims(&issue(&bound_cfg), "example.test", &unbound())
    .unwrap();
  assert!(issued.claims.binding.is_some());
  let mut headers = http::HeaderMap::new();
  headers.insert("user-agent", "other".parse().unwrap());
  let error = bound
    .verify_auth_token(
      &issued.token,
      "example.test",
      &BindingInput::unbound(&headers),
    )
    .unwrap_err();
  assert!(error.downcast_ref::<BindingMismatch>().is_some());

  let plain_cfg = common::config();
  let plain = app::service(&plain_cfg).unwrap();
  let issued = plain
    .exchange_receipt_with_claims(&issue(&plain_cfg), "example.test", &unbound())
    .unwrap();
  assert!(issued.claims.binding.is_none());

  // A cookie carrying a digest is accepted unchanged once binding is turned off.
  let claims = plain
    .verify_auth_token(&issued.token, "example.test", &unbound())
    .unwrap();
  assert!(claims.binding.is_none());
  let stored = crypto::sign_json(
    &rilgate_protocol::claims::AuthClaims {
      binding: Some("stale".into()),
      ..claims
    },
    plain_cfg.pow.auth_signing_key.expose().as_bytes(),
    TokenPurpose::Auth,
  )
  .unwrap();
  assert!(
    plain
      .verify_auth_token(&stored, "example.test", &unbound())
      .is_ok()
  );
}
