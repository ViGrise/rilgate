mod common;
use rilgate_config::{ChallengePolicy, PowAlgorithm, PowEnforcement};
use rilgate_crypto::token::{self as crypto, TokenPurpose};
use rilgate_protocol::claims::ReceiptClaims;
use rilgate_test_utils::gateway as app;
use std::{
  sync::Arc,
  time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::net::TcpListener;

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

async fn fixture(challenge_status: u16, delay: u64) -> Fixture {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.upstream.url = format!("http://{}", listener.local_addr().unwrap());
  config.web.responses = rilgate_config::loader::parse(&format!(
    "site: {{responses: {{challenge_status: {challenge_status}}}}}"
  ))
  .unwrap()
  .site
  .responses;
  let rule = &mut config.pow.policy.rules.as_mut().unwrap()[0];
  rule.when.path.as_mut().unwrap().prefix = Some("/".into());
  rule.challenge = Some(ChallengePolicy {
    algorithm: Some(PowAlgorithm::Refresh),
    difficulty: Some(delay),
  });
  let signing_key = config.pow.signing_key.expose().to_owned();
  let upstream = tokio::spawn(async move {
    let router = axum::Router::new().fallback(|| async { "upstream-ok" });
    axum::serve(listener, router).await.unwrap();
  });
  let gateway = app::router(cfg).await.unwrap();
  Fixture {
    client: reqwest::Client::builder()
      .redirect(reqwest::redirect::Policy::none())
      .build()
      .unwrap(),
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

  async fn get(&self, path: &str, host: &str, cookie: Option<&str>) -> reqwest::Response {
    let mut request = self
      .client
      .get(self.url(path))
      .header("host", host)
      .header("accept", "text/html");
    if let Some(cookie) = cookie {
      request = request.header("cookie", cookie);
    }
    request.send().await.unwrap()
  }

  /// Requests a protected path and returns the refresh URL from the challenge page.
  async fn refresh_url(&self, path: &str) -> String {
    let body = self
      .get(path, "example.test", None)
      .await
      .text()
      .await
      .unwrap();
    let marker = "url=";
    let start = body.find(marker).expect("meta refresh url") + marker.len();
    let end = body[start..].find('"').unwrap() + start;
    body[start..end]
      .replace("&#x2f;", "/")
      .replace("&amp;", "&")
  }
}

fn cookie_pair(response: &reqwest::Response) -> String {
  response.headers()["set-cookie"]
    .to_str()
    .unwrap()
    .split(';')
    .next()
    .unwrap()
    .to_owned()
}

#[tokio::test]
async fn refresh_page_has_a_meta_refresh_and_no_script() {
  let fixture = fixture(503, 1).await;
  let response = fixture.get("/protected", "example.test", None).await;
  assert_eq!(response.status(), 503);
  let body = response.text().await.unwrap();
  assert!(
    body.contains(r#"<meta http-equiv="refresh" content="1;url="#),
    "{body}"
  );
  assert!(
    body.contains("api&#x2f;refresh?presentation_token="),
    "{body}"
  );
  assert!(!body.contains("<script"), "{body}");
}

#[tokio::test]
async fn early_refresh_waits() {
  let fixture = fixture(200, 5).await;
  let url = fixture.refresh_url("/protected").await;
  let response = fixture.get(&url, "example.test", None).await;
  assert_eq!(response.status(), 200);
  assert!(!response.headers().contains_key("set-cookie"));
  let body = response.text().await.unwrap();
  let marker = r#"<meta http-equiv="refresh" content=""#;
  let start = body.find(marker).expect("meta refresh") + marker.len();
  let remaining: u64 = body[start..].split(';').next().unwrap().parse().unwrap();
  assert!((1..=5).contains(&remaining), "{body}");
}

#[tokio::test]
async fn refresh_after_the_delay_sets_the_cookie_and_redirects() {
  let fixture = fixture(200, 1).await;
  let url = fixture.refresh_url("/original/path").await;
  tokio::time::sleep(Duration::from_millis(1100)).await;
  let response = fixture.get(&url, "example.test", None).await;
  assert_eq!(response.status(), 303);
  assert_eq!(response.headers()["location"], "/original/path");
  let cookie = cookie_pair(&response);
  assert!(cookie.starts_with("rilgate_auth="), "{cookie}");
  let page = fixture
    .get("/original/path", "example.test", Some(&cookie))
    .await;
  assert_eq!(page.text().await.unwrap(), "upstream-ok");
}

#[tokio::test]
async fn refresh_redirect_stays_on_the_site() {
  let fixture = fixture(200, 1).await;
  let url = fixture.refresh_url("//evil.example/x").await;
  tokio::time::sleep(Duration::from_millis(1100)).await;
  let response = fixture.get(&url, "example.test", None).await;
  assert_eq!(response.status(), 303);
  assert_eq!(response.headers()["location"], "/");
  let foreign = fixture.get(&url, "other.test", None).await;
  assert_eq!(foreign.status(), 401);
  assert!(!foreign.headers().contains_key("set-cookie"));
}

#[tokio::test]
async fn proof_of_work_cookie_satisfies_a_refresh_requirement() {
  let fixture = fixture(200, 1).await;
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
    fixture.signing_key.as_bytes(),
    TokenPurpose::Receipt,
  )
  .unwrap();
  let exchanged = fixture
    .client
    .post(fixture.url("/.well-known/test-pow/api/exchange"))
    .header("host", "example.test")
    .header("content-type", "application/json")
    .body(serde_json::json!({ "receipt": receipt }).to_string())
    .send()
    .await
    .unwrap();
  assert_eq!(exchanged.status(), 200);
  let cookie = cookie_pair(&exchanged);
  let page = fixture
    .get("/protected", "example.test", Some(&cookie))
    .await;
  assert_eq!(page.text().await.unwrap(), "upstream-ok");
}

#[test]
fn refresh_is_refused_as_the_default_algorithm() {
  let error = rilgate_config::loader::parse(
    "server: {upstream: {url: 'http://127.0.0.1:3000'}}\nsecurity: {secret: '0123456789abcdef0123456789abcdef'}\npow: {default_algorithm: refresh}\n",
  )
  .and_then(|document| document.resolve())
  .unwrap_err();
  assert!(
    format!("{error:#}").contains("pow.default_algorithm: refresh"),
    "{error:#}"
  );
}
