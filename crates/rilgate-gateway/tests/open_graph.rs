mod common;

use axum::{
  body::{Body, to_bytes},
  http::{HeaderMap, Request, Uri},
  response::Html,
};
use rilgate_config::Config;
use rilgate_test_utils::gateway::{self as app, TestGateway};
use std::{
  sync::{Arc, Mutex},
  time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use tower::ServiceExt;

struct Upstream {
  requests: Arc<Mutex<Vec<(Uri, HeaderMap)>>>,
  release: Arc<Semaphore>,
  task: tokio::task::JoinHandle<()>,
  url: String,
}
impl Drop for Upstream {
  fn drop(&mut self) {
    self.task.abort();
  }
}
impl Upstream {
  async fn start(html: &str, paused: bool) -> Self {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/base", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let release = Arc::new(Semaphore::new(if paused { 0 } else { 1000 }));
    let calls = requests.clone();
    let ready = release.clone();
    let html = html.to_owned();
    let router = axum::Router::new().fallback(move |uri: Uri, headers: HeaderMap| {
      calls.lock().unwrap().push((uri, headers));
      let ready = ready.clone();
      let html = html.clone();
      async move {
        ready.acquire().await.unwrap().forget();
        Html(html)
      }
    });
    let task = tokio::spawn(async move {
      axum::serve(listener, router).await.unwrap();
    });
    Self {
      requests,
      release,
      task,
      url,
    }
  }
  fn config(&self) -> Config {
    let mut cfg = common::config();
    let config = Arc::get_mut(&mut cfg).unwrap();
    config.upstream.url = self.url.clone();
    config.open_graph.enabled = true;
    cfg
  }
  fn count(&self) -> usize {
    self.requests.lock().unwrap().len()
  }
}
async fn page(gateway: &TestGateway, path: &str) -> String {
  let response = gateway
    .clone()
    .oneshot(
      Request::builder()
        .uri(path)
        .header("host", "example.test")
        .body(Body::empty())
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), http::StatusCode::OK);
  String::from_utf8(
    to_bytes(response.into_body(), 1_000_000)
      .await
      .unwrap()
      .to_vec(),
  )
  .unwrap()
}

#[tokio::test]
async fn challenge_preserves_public_metadata_order_and_escapes_content() {
  let upstream = Upstream::start(
    r#"<html><head>
    <meta property="OG:TITLE" content='"><script>alert(1)</script>'>
    <meta property="og:image" content="first">
    <meta name="twitter:title" content="A &amp; B">
    <meta property="og:image" content="second">
    <meta name="private" content="secret">
    </head><body><meta property="og:title" content="body-secret"></body></html>"#,
    false,
  )
  .await;
  let gateway = app::router(upstream.config()).await.unwrap();
  let html = page(&gateway, "/protected/page").await;
  assert!(html.contains("<meta property=\"og:title\" content=\"&quot;&gt;&lt;script&gt;alert(1)&lt;&#x2f;script&gt;\">"), "{html}");
  assert!(html.contains("<meta name=\"twitter:title\" content=\"A &amp; B\">"));
  assert!(html.find("content=\"first\"").unwrap() < html.find("content=\"second\"").unwrap());
  assert!(!html.contains("body-secret"));
  assert!(!html.contains("content=\"secret\""));
  assert_eq!(
    upstream.requests.lock().unwrap()[0].0.path(),
    "/base/protected/page"
  );
}

#[tokio::test]
async fn prefetch_uses_only_configured_headers_and_preserves_host_setting() {
  let upstream = Upstream::start(
    "<head><meta name='description' content='public'></head>",
    false,
  )
  .await;
  for preserve_host in [false, true] {
    let mut cfg = upstream.config();
    let config = Arc::get_mut(&mut cfg).unwrap();
    config.upstream.preserve_host = preserve_host;
    config.open_graph.request_headers = [("x-preview-host".into(), "prefix-{host}".into())].into();
    let gateway = app::router(cfg).await.unwrap();
    let response = gateway
      .oneshot(
        Request::builder()
          .uri("/protected")
          .header("host", "EXAMPLE.test:1234")
          .header("cookie", "client-secret=1")
          .header("authorization", "Bearer client-secret")
          .header("user-agent", "client-agent")
          .header("x-client-secret", "secret")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    let html = to_bytes(response.into_body(), 1_000_000).await.unwrap();
    assert!(String::from_utf8_lossy(&html).contains("content=\"public\""));
    let requests = upstream.requests.lock().unwrap();
    let (_, headers) = requests.last().unwrap();
    assert_eq!(headers["x-preview-host"], "prefix-example.test:1234");
    assert_eq!(headers["accept"], "text/html,application/xhtml+xml");
    for name in [
      "cookie",
      "authorization",
      "user-agent",
      "x-client-secret",
      "x-forwarded-host",
      "x-forwarded-proto",
    ] {
      assert!(!headers.contains_key(name), "{name}");
    }
    let expected = if preserve_host {
      "example.test:1234".to_owned()
    } else {
      url::Url::parse(&upstream.url)
        .unwrap()
        .authority()
        .to_owned()
    };
    assert_eq!(headers["host"], expected);
  }
}

#[tokio::test]
async fn concurrent_query_variants_share_one_prefetch() {
  let upstream = Upstream::start(
    "<head><meta property='og:title' content='shared'></head>",
    true,
  )
  .await;
  let gateway = app::router(upstream.config()).await.unwrap();
  let mut requests = Vec::new();
  for index in 0..32 {
    let gateway = gateway.clone();
    requests.push(tokio::spawn(async move {
      page(&gateway, &format!("/protected/shared?q={index}")).await
    }));
  }
  tokio::time::timeout(Duration::from_secs(1), async {
    while upstream.count() == 0 {
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  upstream.release.add_permits(32);
  for request in requests {
    assert!(request.await.unwrap().contains("content=\"shared\""));
  }
  assert_eq!(upstream.count(), 1);
  assert_eq!(upstream.requests.lock().unwrap()[0].0.query(), None);
}

#[tokio::test]
async fn response_deadline_leaves_the_fetch_running_for_the_next_request() {
  let upstream = Upstream::start(
    "<head><meta property='og:title' content='eventual'></head>",
    true,
  )
  .await;
  let mut cfg = upstream.config();
  Arc::get_mut(&mut cfg).unwrap().open_graph.timeout = Duration::from_millis(100);
  let gateway = app::router(cfg).await.unwrap();
  let started = Instant::now();
  let html = page(&gateway, "/protected/slow").await;
  assert!(!html.contains("content=\"eventual\""));
  assert!(started.elapsed() >= Duration::from_millis(90));
  assert!(started.elapsed() < Duration::from_millis(600));
  assert_eq!(upstream.count(), 1);
  upstream.release.add_permits(1);
  let html = page(&gateway, "/protected/slow").await;
  assert!(html.contains("content=\"eventual\""));
  assert_eq!(upstream.count(), 1);
}

#[tokio::test]
async fn disabled_and_override_modes_send_no_prefetch() {
  let upstream = Upstream::start(
    "<head><meta property='og:title' content='upstream'></head>",
    false,
  )
  .await;
  let mut cfg = upstream.config();
  Arc::get_mut(&mut cfg).unwrap().open_graph.enabled = false;
  let gateway = app::router(cfg).await.unwrap();
  assert!(
    !page(&gateway, "/protected")
      .await
      .contains("content=\"upstream\"")
  );
  assert_eq!(upstream.count(), 0);
  let mut cfg = upstream.config();
  Arc::get_mut(&mut cfg).unwrap().open_graph.overrides = [
    ("OG:TITLE".into(), "configured".into()),
    ("twitter:title".into(), "preview".into()),
  ]
  .into();
  let gateway = app::router(cfg).await.unwrap();
  let html = page(&gateway, "/protected").await;
  assert!(html.contains("<meta property=\"og:title\" content=\"configured\">"));
  assert!(html.contains("<meta property=\"twitter:title\" content=\"preview\">"));
  assert_eq!(upstream.count(), 0);
}

#[test]
fn configuration_rejects_forbidden_headers_and_unknown_placeholders() {
  for (header, value, path) in [
    ("Host", "example.test", "host"),
    ("x-preview", "{unknown}", "x-preview"),
  ] {
    let text = format!(
      "server:\n  upstream:\n    url: http://127.0.0.1:3000\nsite:\n  open_graph:\n    request_headers:\n      {header}: '{value}'\n"
    );
    let error = rilgate_config::loader::parse(&text)
      .unwrap()
      .resolve()
      .unwrap_err();
    assert!(
      format!("{error:#}").contains(&format!("site.open_graph.request_headers.{path}:")),
      "{error:#}"
    );
  }
}

#[tokio::test]
async fn redirects_stay_on_origin_and_failed_lookups_are_cached() {
  let external = Upstream::start(
    "<head><meta property='og:title' content='external'></head>",
    false,
  )
  .await;
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let calls = Arc::new(Mutex::new(Vec::new()));
  let captured = calls.clone();
  let external_url = external.url.clone();
  let router = axum::Router::new().fallback(move |uri: Uri, headers: HeaderMap| {
    captured.lock().unwrap().push((uri.clone(), headers));
    let external_url = external_url.clone();
    async move {
      match uri.path() {
        "/base/protected/start" => axum::response::Response::builder()
          .status(302)
          .header("location", "/base/protected/end")
          .body(Body::empty())
          .unwrap(),
        "/base/protected/external" => axum::response::Response::builder()
          .status(302)
          .header("location", external_url)
          .body(Body::empty())
          .unwrap(),
        "/base/protected/loop" => axum::response::Response::builder()
          .status(302)
          .header("location", "/base/protected/loop")
          .body(Body::empty())
          .unwrap(),
        _ => axum::response::Response::builder()
          .header("content-type", "text/html")
          .body(Body::from(
            "<head><meta property='og:title' content='redirected'></head>",
          ))
          .unwrap(),
      }
    }
  });
  let task = tokio::spawn(async move {
    axum::serve(listener, router).await.unwrap();
  });
  let mut cfg = external.config();
  Arc::get_mut(&mut cfg).unwrap().upstream.url = format!("http://{address}/base");
  let gateway = app::router(cfg).await.unwrap();
  assert!(
    page(&gateway, "/protected/start")
      .await
      .contains("content=\"redirected\"")
  );
  assert_eq!(calls.lock().unwrap().len(), 2);
  assert!(!calls.lock().unwrap()[1].1.contains_key("referer"));
  for _ in 0..2 {
    assert!(
      !page(&gateway, "/protected/external")
        .await
        .contains("content=\"external\"")
    );
  }
  assert_eq!(external.count(), 0);
  assert_eq!(calls.lock().unwrap().len(), 3);
  for _ in 0..2 {
    assert!(
      !page(&gateway, "/protected/loop")
        .await
        .contains("content=\"redirected\"")
    );
  }
  assert_eq!(calls.lock().unwrap().len(), 7);
  task.abort();
}

#[tokio::test]
async fn metadata_limits_preserve_complete_tags_and_utf8_content() {
  let content = "é".repeat(513);
  let mut source = format!("<head><meta property='og:title' content='{content}'>");
  source.push_str(&format!(
    "<meta property='og:{}' content='long-name'>",
    "x".repeat(62)
  ));
  for index in 0..33 {
    source.push_str(&format!(
      "<meta property='og:image' content='image-{index}'>"
    ));
  }
  source.push_str("</head>");
  let upstream = Upstream::start(&source, false).await;
  let gateway = app::router(upstream.config()).await.unwrap();
  let html = page(&gateway, "/protected/limits").await;
  assert!(html.contains(&format!("content=\"{}\"", "é".repeat(512))));
  assert!(!html.contains(&content));
  assert!(!html.contains("long-name"));
  assert_eq!(html.matches("<meta property=\"og:").count(), 32);
  assert!(html.contains("content=\"image-30\""));
  assert!(!html.contains("content=\"image-31\""));

  let source = format!(
    "<head><meta name='description' content='early'>{}<meta property='og:title' content='late'></head>",
    " ".repeat(512 * 1024)
  );
  let upstream = Upstream::start(&source, false).await;
  let gateway = app::router(upstream.config()).await.unwrap();
  let html = page(&gateway, "/protected/large").await;
  assert!(html.contains("content=\"early\""));
  assert!(!html.contains("content=\"late\""));
}

#[tokio::test]
async fn saturated_fetches_skip_without_caching_the_skipped_path() {
  let upstream = Upstream::start(
    "<head><meta property='og:title' content='available'></head>",
    true,
  )
  .await;
  let mut cfg = upstream.config();
  Arc::get_mut(&mut cfg).unwrap().open_graph.timeout = Duration::from_millis(100);
  let gateway = app::router(cfg).await.unwrap();
  let mut pending = Vec::new();
  for index in 0..16 {
    let gateway = gateway.clone();
    pending.push(tokio::spawn(async move {
      page(&gateway, &format!("/protected/slot-{index}")).await
    }));
  }
  for request in pending {
    assert!(!request.await.unwrap().contains("content=\"available\""));
  }
  assert_eq!(upstream.count(), 16);
  assert!(
    !page(&gateway, "/protected/skipped")
      .await
      .contains("content=\"available\"")
  );
  assert_eq!(upstream.count(), 16);
  upstream.release.add_permits(100);
  tokio::time::timeout(Duration::from_secs(1), async {
    loop {
      if page(&gateway, "/protected/skipped")
        .await
        .contains("content=\"available\"")
      {
        break;
      }
      tokio::task::yield_now().await;
    }
  })
  .await
  .unwrap();
  assert_eq!(upstream.count(), 17);
}

#[tokio::test]
async fn query_and_host_keys_isolate_prefetches_and_expanded_header_limits_apply() {
  let upstream = Upstream::start(
    "<head><meta name='description' content='isolated'></head>",
    false,
  )
  .await;
  let mut cfg = upstream.config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.open_graph.include_query = true;
  config.open_graph.request_headers =
    [("x-too-long".into(), format!("{}{{host}}", "x".repeat(1020)))].into();
  let gateway = app::router(cfg).await.unwrap();
  for host in ["one.test", "two.test"] {
    for query in ["a", "b", "a"] {
      let response = gateway
        .clone()
        .oneshot(
          Request::builder()
            .uri(format!("/protected/key?q={query}"))
            .header("host", host)
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
      let html = to_bytes(response.into_body(), 1_000_000).await.unwrap();
      assert!(String::from_utf8_lossy(&html).contains("content=\"isolated\""));
    }
  }
  let requests = upstream.requests.lock().unwrap();
  assert_eq!(requests.len(), 4);
  assert_eq!(requests[0].0.query(), Some("q=a"));
  assert_eq!(requests[1].0.query(), Some("q=b"));
  assert!(
    requests
      .iter()
      .all(|(_, headers)| !headers.contains_key("x-too-long"))
  );
}

#[tokio::test]
async fn configured_tags_replace_prefixes_and_preserve_source_attributes() {
  let upstream = Upstream::start("<head><meta property='article:author' content='article'><meta name='article:section' content='section'><meta property='og:title' content='removed'><meta name='description' content='named'><meta property='description' name='author' content='property'></head>", false).await;
  let mut cfg = upstream.config();
  Arc::get_mut(&mut cfg).unwrap().open_graph.tags.prefixes = vec!["article:".into()];
  let gateway = app::router(cfg).await.unwrap();
  let html = page(&gateway, "/protected/tags").await;
  assert!(html.contains("<meta property=\"article:author\" content=\"article\">"));
  assert!(!html.contains("content=\"removed\""));
  assert!(html.contains("<meta name=\"article:section\" content=\"section\">"));
  assert!(html.contains("<meta name=\"description\" content=\"named\">"));
  assert!(html.contains("<meta property=\"description\" content=\"property\">"));
}

#[tokio::test]
async fn host_policy_matches_the_normalized_host_and_port() {
  use rilgate_config::schema::policy::{Action, Condition, StringMatch};
  let upstream = Upstream::start("allowed", false).await;
  let mut cfg = upstream.config();
  let rule = &mut Arc::get_mut(&mut cfg)
    .unwrap()
    .pow
    .policy
    .rules
    .as_mut()
    .unwrap()[0];
  rule.when = Condition {
    host: Some(StringMatch {
      exact: Some("example.test:8443".into()),
      ..Default::default()
    }),
    ..Default::default()
  };
  rule.action = Action::Deny;
  rule.challenge = None;
  let gateway = app::router(cfg).await.unwrap();
  for (host, status) in [
    ("EXAMPLE.TEST:8443", 403),
    ("example.test", 200),
    ("other.test:8443", 200),
  ] {
    let response = gateway
      .clone()
      .oneshot(
        Request::builder()
          .uri("/protected")
          .header("host", host)
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(response.status().as_u16(), status, "{host}");
  }
  assert_eq!(upstream.count(), 2);
}

#[tokio::test]
async fn request_selection_applies_to_fetches_overrides_and_lookup_metrics() {
  let upstream = Upstream::start(
    "<head><meta property='og:title' content='selected'></head>",
    false,
  )
  .await;
  for overrides in [false, true] {
    let mut cfg = upstream.config();
    let config = Arc::get_mut(&mut cfg).unwrap();
    config.server.metrics_enabled = true;
    config.pow.policy.rules.as_mut().unwrap()[0]
      .when
      .path
      .as_mut()
      .unwrap()
      .prefix = Some("/".into());
    config.open_graph.when = rilgate_config::loader::parse(
      "site:\n  open_graph:\n    when:\n      not: {path: {prefix: /admin/}}\n",
    )
    .unwrap()
    .site
    .open_graph
    .when;
    if overrides {
      config.open_graph.overrides = [("og:title".into(), "selected".into())].into();
    }
    let gateway = app::router(cfg).await.unwrap();
    assert!(
      !page(&gateway, "/admin/x")
        .await
        .contains("content=\"selected\"")
    );
    assert!(
      page(&gateway, "/blog/x")
        .await
        .contains("content=\"selected\"")
    );
    let metrics = reqwest::Client::new()
      .get(format!(
        "http://{}/.well-known/test-pow/api/metrics",
        gateway.operations_address()
      ))
      .send()
      .await
      .unwrap()
      .text()
      .await
      .unwrap();
    for result in ["hit", "fetched", "failed", "timeout", "busy", "override"] {
      let count = usize::from(result == if overrides { "override" } else { "fetched" });
      assert!(
        metrics.contains(&format!(
          "rilgate_open_graph_lookups_total{{result=\"{result}\"}} {count}\n"
        )),
        "{metrics}"
      );
    }
  }
  assert_eq!(upstream.count(), 1);
  assert_eq!(
    upstream.requests.lock().unwrap()[0].0.path(),
    "/base/blog/x"
  );
}

#[test]
fn score_is_rejected_at_its_nested_request_condition_path() {
  let document = rilgate_config::loader::parse("server:\n  upstream:\n    url: http://127.0.0.1:3000\nsite:\n  open_graph:\n    when:\n      all:\n        - path: {prefix: /}\n        - score: {gte: 1}\n").unwrap();
  let error = document.resolve().unwrap_err();
  assert!(
    format!("{error:#}")
      .contains("site.open_graph.when.all[1].score: score is only available in policy rules"),
    "{error:#}"
  );
}

#[tokio::test]
async fn request_conditions_observe_published_policy_ip_sets() {
  use rilgate_config::schema::policy::{Condition, FeedFormat, IpMatch, IpSetDefinition};
  let upstream = Upstream::start(
    "<head><meta property='og:title' content='loaded'></head>",
    false,
  )
  .await;
  let mut cfg = upstream.config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.pow.policy.ip_sets.insert(
    "feed".into(),
    IpSetDefinition {
      url: Some("http://127.0.0.1:9/ips.json".into()),
      format: Some(FeedFormat::IpPrefixesJson),
      ..Default::default()
    },
  );
  config.open_graph.when = Some(Condition {
    not: Some(Box::new(Condition {
      client_ip: Some(IpMatch {
        in_sets: Some(vec!["feed".into()]),
        ..Default::default()
      }),
      ..Default::default()
    })),
    ..Default::default()
  });
  let (challenge, pool) = app::services(&cfg).unwrap();
  let gateway = app::TestGateway::start(
    rilgate_gateway::app::build(cfg, challenge.clone(), pool)
      .await
      .unwrap(),
  )
  .await
  .unwrap();
  assert!(
    !page(&gateway, "/protected/ip")
      .await
      .contains("content=\"loaded\"")
  );
  assert_eq!(upstream.count(), 0);
  challenge
    .sets()
    .ip
    .get("feed")
    .unwrap()
    .publish(rilgate_policy::IpRanges::from_networks(["192.0.2.0/24"
      .parse()
      .unwrap()]));
  assert!(
    page(&gateway, "/protected/ip")
      .await
      .contains("content=\"loaded\"")
  );
  assert_eq!(upstream.count(), 1);
}

#[tokio::test]
async fn only_unencoded_html_responses_and_valid_tag_names_produce_metadata() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let router = axum::Router::new().fallback(move |uri: Uri| async move {
    let page = "<head><meta property='og:title' content='leaked'></head>";
    let builder = axum::response::Response::builder();
    let builder = match uri.path() {
      "/base/protected/plain" => builder.header("content-type", "text/plain"),
      "/base/protected/untyped" => builder,
      "/base/protected/encoded" => builder
        .header("content-type", "text/html")
        .header("content-encoding", "gzip"),
      _ => {
        return builder
          .header("content-type", "text/html; charset=utf-8")
          .body(Body::from(
            "<head><meta property='og:ti\"tle' content='quoted'>\
             <meta property='og:t&#x0a;itle' content='control'>\
             <meta property='og:title' content='a\u{7}b&#x0a;c'></head>",
          ))
          .unwrap();
      }
    };
    builder.body(Body::from(page)).unwrap()
  });
  let task = tokio::spawn(async move {
    axum::serve(listener, router).await.unwrap();
  });
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.upstream.url = format!("http://{address}/base");
  config.open_graph.enabled = true;
  let gateway = app::router(cfg).await.unwrap();
  for path in [
    "/protected/plain",
    "/protected/untyped",
    "/protected/encoded",
  ] {
    assert!(!page(&gateway, path).await.contains("leaked"), "{path}");
  }
  let html = page(&gateway, "/protected/names").await;
  assert!(
    !html.contains("quoted") && !html.contains("control"),
    "{html}"
  );
  assert!(
    html.contains("<meta property=\"og:title\" content=\"ab c\">"),
    "{html}"
  );
  task.abort();
}
