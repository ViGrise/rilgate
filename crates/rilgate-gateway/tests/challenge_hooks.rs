use axum::{
  body::{Body, to_bytes},
  http::{Method, Request, StatusCode},
  routing::post,
};
use rilgate_config::{Config, schema::Document};
use rilgate_pow_hash::sha256::verifier::verify_fast;
use rilgate_test_utils::gateway::{self, TestGateway};
use serde_json::{Value, json};
use tower::ServiceExt;
const PREFIX: &str = "/.well-known/rilgate";
struct Origin {
  url: String,
  task: tokio::task::JoinHandle<()>,
}
impl Drop for Origin {
  fn drop(&mut self) {
    self.task.abort();
  }
}
async fn origin() -> Origin {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let url = format!("http://{}", listener.local_addr().unwrap());
  let task = tokio::spawn(async move {
    axum::serve(
      listener,
      axum::Router::new()
        .route(
          "/verify",
          post(|body: String| async move {
            axum::Json(json!({"success":body.contains("result=pass")}))
          }),
        )
        .fallback(|| async { "forwarded" }),
    )
    .await
    .unwrap();
  });
  Origin { url, task }
}
fn config(origin: &str, verify: &str, presentation: &str, steps: &str, fallback: bool) -> Config {
  let unavailable = if fallback {
    "{action: fallback, fallback: {algorithm: fast, difficulty: 2}, grant: until_expiry, after: 1}"
  } else {
    "{}"
  };
  let document: Document=serde_yaml_ng::from_str(&format!("environment: development\nserver:\n  upstream: {{url: '{origin}'}}\nchallenge_hooks:\n  captcha:\n    presentation: {presentation}\n    verify:\n      request: {{url: '{verify}/verify', fields: {{result: '{{result}}'}}}}\n      accept: [{{field: /success, equals: true}}]\n      retry: {{attempts: 1}}\n    unavailable: {unavailable}\npolicy:\n  default_action: allow\n  rules:\n    - name: protected\n      when: {{path: {{prefix: /protected}}}}\n      action: challenge\n      challenge: {steps}\n")).unwrap();
  document.resolve().unwrap()
}
async fn request(
  router: &TestGateway,
  method: Method,
  path: &str,
  cookie: Option<&str>,
  body: Option<Value>,
  html: bool,
) -> axum::response::Response {
  let mut builder = Request::builder()
    .method(method)
    .uri(path)
    .header("host", "example.test")
    .header(
      "accept",
      if html {
        "text/html"
      } else {
        "application/json"
      },
    );
  if let Some(cookie) = cookie {
    builder = builder.header("cookie", cookie);
  }
  let body = if let Some(value) = body {
    builder = builder.header("content-type", "application/json");
    Body::from(value.to_string())
  } else {
    Body::empty()
  };
  router
    .clone()
    .oneshot(builder.body(body).unwrap())
    .await
    .unwrap()
}
async fn body(out: axum::response::Response) -> Value {
  serde_json::from_slice(&to_bytes(out.into_body(), 1_048_576).await.unwrap()).unwrap()
}
fn cookie(out: &axum::response::Response) -> String {
  out.headers()["set-cookie"]
    .to_str()
    .unwrap()
    .split(';')
    .next()
    .unwrap()
    .into()
}
async fn solve(
  router: &TestGateway,
  token: &str,
  cookie: Option<&str>,
) -> axum::response::Response {
  let work = body(
    request(
      router,
      Method::POST,
      &format!("{PREFIX}/api/challenge"),
      cookie,
      Some(json!({"presentation_token":token})),
      false,
    )
    .await,
  )
  .await;
  let nonce = (0..100_000)
    .map(|n| n.to_string())
    .find(|n| {
      verify_fast(
        work["challenge"].as_str().unwrap(),
        n,
        work["difficulty"].as_u64().unwrap(),
      )
      .unwrap()
    })
    .unwrap();
  request(router,Method::POST,&format!("{PREFIX}/api/verify"),cookie,Some(json!({"presentation_token":token,"challenge_token":work["challenge_token"],"nonce":nonce,"set_cookie":true})),false).await
}
#[tokio::test]
async fn fast_then_redirect_hook_merges_grants_and_forwards() {
  let origin = origin().await;
  let cfg = config(
    &origin.url,
    &origin.url,
    "{mode: redirect, url: 'https://example.com/start?state={state}&return={callback}', result_param: code}",
    "{steps: [{algorithm: fast, difficulty: 1}, {hook: captcha}]}",
    false,
  );
  let router = gateway::router(cfg).await.unwrap();
  let challenge = body(request(&router, Method::GET, "/protected", None, None, false).await).await;
  let out = solve(
    &router,
    challenge["challenge"]["token"].as_str().unwrap(),
    None,
  )
  .await;
  assert_eq!(out.status(), StatusCode::OK);
  let first_cookie = cookie(&out);
  let verified = body(out).await;
  assert_eq!(verified["next"]["method"], "hook");
  assert_eq!(verified["next"]["hook"]["mode"], "redirect");
  let out = request(
    &router,
    Method::GET,
    "/protected",
    Some(&first_cookie),
    None,
    true,
  )
  .await;
  assert_eq!(out.status(), StatusCode::FOUND);
  let url = url::Url::parse(out.headers()["location"].to_str().unwrap()).unwrap();
  let query: std::collections::BTreeMap<_, _> = url.query_pairs().into_owned().collect();
  assert_eq!(
    query["return"],
    format!("http://example.test{PREFIX}/api/hook/callback")
  );
  let callback = format!(
    "{PREFIX}/api/hook/callback?{}",
    url::form_urlencoded::Serializer::new(String::new())
      .append_pair("state", &query["state"])
      .append_pair("code", "pass")
      .finish()
  );
  let out = request(
    &router,
    Method::GET,
    &callback,
    Some(&first_cookie),
    None,
    true,
  )
  .await;
  assert_eq!(out.status(), StatusCode::FOUND);
  assert_eq!(out.headers()["location"], "/protected");
  let merged_cookie = cookie(&out);
  let out = request(
    &router,
    Method::GET,
    "/protected",
    Some(&merged_cookie),
    None,
    true,
  )
  .await;
  assert_eq!(out.status(), StatusCode::OK);
  assert_eq!(to_bytes(out.into_body(), 1024).await.unwrap(), "forwarded");
}
#[tokio::test]
async fn embed_verification_rejects_failure_and_sets_cookie_on_pass() {
  let origin = origin().await;
  let cfg = config(
    &origin.url,
    &origin.url,
    "{mode: embed, script: {src: 'https://example.com/widget.js'}, csp: {script-src: ['https://example.com']}}",
    "{hook: captcha}",
    false,
  );
  let router = gateway::router(cfg).await.unwrap();
  let challenge = body(request(&router, Method::GET, "/protected", None, None, false).await).await;
  let token = &challenge["challenge"]["token"];
  let out = request(
    &router,
    Method::POST,
    &format!("{PREFIX}/api/hook/verify"),
    None,
    Some(json!({"presentation_token":token,"result":"fail"})),
    false,
  )
  .await;
  assert_eq!(out.status(), StatusCode::UNAUTHORIZED);
  assert_eq!(body(out).await["code"], "hook_rejected");
  let out = request(
    &router,
    Method::POST,
    &format!("{PREFIX}/api/hook/verify"),
    None,
    Some(json!({"presentation_token":token,"result":"pass"})),
    false,
  )
  .await;
  assert_eq!(out.status(), StatusCode::OK);
  assert!(out.headers().contains_key("set-cookie"));
  assert_eq!(body(out).await["cookie_set"], true);
}
#[tokio::test]
async fn unavailable_hook_presents_fallback_and_accepts_its_grant() {
  let origin = origin().await;
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let down = format!("http://{}", listener.local_addr().unwrap());
  drop(listener);
  let cfg = config(
    &origin.url,
    &down,
    "{mode: embed, script: {src: 'https://example.com/widget.js'}, csp: {script-src: ['https://example.com']}}",
    "{hook: captcha}",
    true,
  );
  let auth_key = cfg.pow.auth_signing_key.expose().as_bytes().to_vec();
  let router = gateway::router(cfg).await.unwrap();
  let challenge = body(request(&router, Method::GET, "/protected", None, None, false).await).await;
  let out = request(
    &router,
    Method::POST,
    &format!("{PREFIX}/api/hook/verify"),
    None,
    Some(json!({"presentation_token":challenge["challenge"]["token"],"result":"pass"})),
    false,
  )
  .await;
  assert_eq!(out.status(), StatusCode::SERVICE_UNAVAILABLE);
  let unavailable = body(out).await;
  assert_eq!(unavailable["code"], "hook_unavailable");
  assert_eq!(unavailable["next"]["method"], "pow");
  let challenge = body(request(&router, Method::GET, "/protected", None, None, false).await).await;
  assert_eq!(challenge["challenge"]["method"], "pow");
  assert_eq!(challenge["challenge"]["algorithm"], "fast");
  let out = solve(
    &router,
    challenge["challenge"]["token"].as_str().unwrap(),
    None,
  )
  .await;
  assert_eq!(out.status(), StatusCode::OK);
  let cookie = cookie(&out);
  let claims: rilgate_protocol::claims::AuthClaimsV2 = rilgate_crypto::token::verify_json(
    cookie.split_once('=').unwrap().1,
    &auth_key,
    rilgate_crypto::token::TokenPurpose::Auth,
  )
  .unwrap();
  assert!(claims.grants.iter().any(|grant| matches!(grant,
    rilgate_protocol::Grant::Pow(grant) if grant.fallback_for.as_deref() == Some("captcha"))));
  let out = request(
    &router,
    Method::GET,
    "/protected",
    Some(&cookie),
    None,
    true,
  )
  .await;
  assert_eq!(out.status(), StatusCode::OK);
}
#[tokio::test]
async fn callback_rejects_head() {
  let router = gateway::router(rilgate_test_utils::config::config())
    .await
    .unwrap();
  let out = request(
    &router,
    Method::HEAD,
    "/.well-known/test-pow/api/hook/callback",
    None,
    None,
    true,
  )
  .await;
  assert_eq!(out.status(), StatusCode::METHOD_NOT_ALLOWED);
  assert_eq!(out.headers()["allow"], "GET");
}

#[tokio::test]
async fn cheap_receipt_cannot_acquire_a_stronger_fallback_label() {
  let origin = origin().await;
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let down = format!("http://{}", listener.local_addr().unwrap());
  drop(listener);
  let cfg = config(
    &origin.url,
    &down,
    "{mode: embed, script: {src: 'https://example.com/widget.js'}, csp: {script-src: ['https://example.com']}}",
    "{hook: captcha}",
    true,
  );
  let mut cfg = cfg;
  let pow = &mut std::sync::Arc::get_mut(&mut cfg).unwrap().pow;
  let mut cheap = pow.policy.policy.rules.as_ref().unwrap()[0].clone();
  cheap.name = "cheap".into();
  cheap.when = serde_yaml_ng::from_str("{path: {prefix: /cheap}}").unwrap();
  cheap.challenge = Some(serde_yaml_ng::from_str("{algorithm: fast, difficulty: 1}").unwrap());
  pow.policy.policy.rules.as_mut().unwrap().push(cheap);
  let router = gateway::router(cfg).await.unwrap();
  let challenge = body(request(&router, Method::GET, "/protected", None, None, false).await).await;
  let out = request(
    &router,
    Method::POST,
    &format!("{PREFIX}/api/hook/verify"),
    None,
    Some(json!({"presentation_token":challenge["challenge"]["token"],"result":"pass"})),
    false,
  )
  .await;
  assert_eq!(out.status(), StatusCode::SERVICE_UNAVAILABLE);
  let unavailable = body(out).await;
  assert_eq!(unavailable["code"], "hook_unavailable");
  assert_eq!(unavailable["next"]["method"], "pow");
  let challenge = body(request(&router, Method::GET, "/protected", None, None, false).await).await;
  assert_eq!(challenge["challenge"]["method"], "pow");
  assert_eq!(challenge["challenge"]["algorithm"], "fast");
  let cheap = body(request(&router, Method::GET, "/cheap", None, None, false).await).await;
  let solved = solve(&router, cheap["challenge"]["token"].as_str().unwrap(), None).await;
  assert_eq!(solved.status(), StatusCode::OK);
  let receipt = body(solved).await["receipt"].clone();
  let out = request(
    &router,
    Method::POST,
    &format!("{PREFIX}/api/exchange"),
    None,
    Some(json!({"receipt":receipt,"presentation_token":challenge["challenge"]["token"]})),
    false,
  )
  .await;
  assert_eq!(out.status(), StatusCode::UNAUTHORIZED);
  assert!(!out.headers().contains_key("set-cookie"));
  let refusal = body(out).await;
  assert_eq!(refusal["status"], 401);
  assert!(
    refusal["message"]
      .as_str()
      .unwrap()
      .starts_with("Unauthorized:")
  );
}
