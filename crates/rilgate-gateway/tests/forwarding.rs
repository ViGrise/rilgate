mod common;
use rilgate_test_utils::gateway as app;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

type Seen = Arc<Mutex<Vec<(String, http::HeaderMap)>>>;

struct Fixture {
  client: reqwest::Client,
  address: std::net::SocketAddr,
  seen: Seen,
  _gateway: app::TestGateway,
  upstream: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
  fn drop(&mut self) {
    self.upstream.abort();
  }
}

async fn fixture(configure: impl FnOnce(&mut rilgate_config::Configuration)) -> Fixture {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.upstream.url = format!("http://{}", listener.local_addr().unwrap());
  config.pow.policy.policy.rules.as_mut().unwrap()[0]
    .when
    .path = Some(rilgate_config::schema::policy::StringMatch {
    prefix: Some("/admin/".into()),
    ..Default::default()
  });
  configure(config);
  let seen = Seen::default();
  let recorded = seen.clone();
  let upstream = tokio::spawn(async move {
    let router = axum::Router::new().fallback(move |request: axum::extract::Request| {
      let recorded = recorded.clone();
      async move {
        recorded
          .lock()
          .unwrap()
          .push((request.uri().path().to_string(), request.headers().clone()));
        "upstream-ok"
      }
    });
    axum::serve(listener, router).await.unwrap();
  });
  let gateway = app::router(cfg).await.unwrap();
  Fixture {
    client: reqwest::Client::new(),
    address: gateway.address(),
    seen,
    _gateway: gateway,
    upstream,
  }
}

impl Fixture {
  fn get(&self, path: &str) -> reqwest::RequestBuilder {
    self
      .client
      .get(format!("http://{}{path}", self.address))
      .header("host", "example.test")
      .header("accept", "application/json")
  }

  fn last(&self) -> (String, http::HeaderMap) {
    self
      .seen
      .lock()
      .unwrap()
      .last()
      .cloned()
      .expect("upstream request")
  }
}

#[tokio::test]
async fn path_variants_cannot_skip_a_path_rule() {
  let fixture = fixture(|_| {}).await;
  for path in ["//admin/panel", "/%61dmin/panel"] {
    let response = fixture.get(path).send().await.unwrap();
    assert_eq!(response.status(), 403, "{path}");
    assert_eq!(
      response.headers()["rilgate-challenge"],
      "required",
      "{path}"
    );
  }
  assert_eq!(
    fixture.get("/x/..;/admin").send().await.unwrap().status(),
    400
  );
  assert_eq!(
    fixture.get("//public/a").send().await.unwrap().status(),
    200
  );
  assert_eq!(fixture.last().0, "//public/a");
  assert_eq!(fixture.seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn client_address_headers_carry_the_resolved_address() {
  let spoofed = |fixture: &Fixture| {
    fixture
      .get("/page")
      .header("x-forwarded-for", "6.6.6.6")
      .header("x-real-ip", "6.6.6.6")
      .header("forwarded", "for=6.6.6.6")
  };
  let fixture_default = fixture(|_| {}).await;
  assert_eq!(
    spoofed(&fixture_default).send().await.unwrap().status(),
    200
  );
  let (_, headers) = fixture_default.last();
  assert_eq!(headers.get_all("x-forwarded-for").iter().count(), 1);
  let peer = fixture_default.address.ip();
  assert_eq!(headers["x-forwarded-for"], peer.to_string().as_str());
  assert_eq!(headers["x-real-ip"], peer.to_string().as_str());
  let forwarded = match peer {
    std::net::IpAddr::V4(ip) => format!("for={ip}"),
    std::net::IpAddr::V6(ip) => format!("for=\"[{ip}]\""),
  };
  assert_eq!(headers["forwarded"], forwarded.as_str());

  let fixture_empty = fixture(|config| config.upstream.client_ip_headers.clear()).await;
  assert_eq!(spoofed(&fixture_empty).send().await.unwrap().status(), 200);
  let (_, headers) = fixture_empty.last();
  assert_eq!(headers["x-forwarded-for"], "6.6.6.6");
  assert_eq!(headers["x-real-ip"], "6.6.6.6");
  assert_eq!(headers["forwarded"], "for=6.6.6.6");
}

#[tokio::test]
async fn access_cookie_is_removed_before_the_upstream() {
  let fixture = fixture(|_| {}).await;
  let response = fixture
    .get("/page")
    .header("cookie", "rilgate_auth=abc; app=1")
    .send()
    .await
    .unwrap();
  assert_eq!(response.status(), 200);
  let (_, headers) = fixture.last();
  assert_eq!(headers["cookie"], "app=1");
}

#[tokio::test]
async fn request_id_is_kept_only_from_trusted_proxies() {
  let untrusted = fixture(|_| {}).await;
  let response = untrusted
    .get("/page")
    .header("x-request-id", "abc")
    .send()
    .await
    .unwrap();
  assert_ne!(response.headers()["x-request-id"], "abc");

  let trusted = fixture(|config| {
    config.server.trusted_proxies =
      vec!["::1/128".parse().unwrap(), "127.0.0.1/32".parse().unwrap()];
  })
  .await;
  let response = trusted
    .get("/page")
    .header("x-request-id", "abc-1.2_3")
    .send()
    .await
    .unwrap();
  assert_eq!(response.headers()["x-request-id"], "abc-1.2_3");
  assert_eq!(trusted.last().1["x-request-id"], "abc-1.2_3");
  let response = trusted
    .get("/page")
    .header("x-request-id", "a b")
    .send()
    .await
    .unwrap();
  assert_ne!(response.headers()["x-request-id"], "a b");
}
