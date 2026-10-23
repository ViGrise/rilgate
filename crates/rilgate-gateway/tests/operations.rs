mod common;
use rilgate_test_utils::gateway as app;
use std::{sync::Arc, time::Duration};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::{TcpListener, TcpStream},
};

#[tokio::test]
async fn readiness_tracks_required_dependencies_while_liveness_stays_available() {
  let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.upstream.url = format!("http://{}", origin.local_addr().unwrap());
  config.server.metrics_enabled = true;
  let gateway = app::router(cfg).await.unwrap();
  let client = reqwest::Client::new();
  let base = format!(
    "http://{}/.well-known/test-pow/api",
    gateway.operations_address()
  );
  let ready = client.get(format!("{base}/ready")).send().await.unwrap();
  assert_eq!(ready.status(), 200);
  let body: serde_json::Value = serde_json::from_slice(&ready.bytes().await.unwrap()).unwrap();
  assert_eq!(body["pool"], serde_json::Value::Null);
  drop(origin);
  assert_eq!(
    client
      .get(format!("{base}/ready"))
      .send()
      .await
      .unwrap()
      .status(),
    503
  );
  assert_eq!(
    client
      .get(format!("{base}/live"))
      .send()
      .await
      .unwrap()
      .status(),
    200
  );
  let metrics = client.get(format!("{base}/metrics")).send().await.unwrap();
  assert_eq!(metrics.status(), 200);
  assert_eq!(metrics.headers()["cache-control"], "no-store");
  let text = metrics.text().await.unwrap();
  assert!(
    text.contains("rilgate_http_requests_total{status_class=\"5xx\"} 1"),
    "{text}"
  );
  assert!(text.contains("rilgate_http_request_duration_seconds_bucket{le=\"+Inf\"}"));
  assert!(text.contains("rilgate_pool_sessions_active 0"));
  let mut family = "";
  let mut closed = std::collections::HashSet::new();
  for line in text.lines() {
    let name = if let Some(rest) = line.strip_prefix("# TYPE ") {
      rest.split_whitespace().next().unwrap()
    } else if line.starts_with('#') || line.is_empty() {
      continue;
    } else {
      line.split(['{', ' ']).next().unwrap()
    };
    let name = name
      .strip_suffix("_bucket")
      .or_else(|| name.strip_suffix("_count"))
      .or_else(|| name.strip_suffix("_sum"))
      .unwrap_or(name);
    if name != family {
      closed.insert(family);
      assert!(
        !closed.contains(name),
        "metric family {name} is interleaved"
      );
      family = name;
    }
  }
  assert!(closed.contains("rilgate_randomx_hash_total"));
  assert!(closed.contains("rilgate_randomx_hash_fallback_total"));
  assert!(text.contains("rilgate_verifier_queue_depth 0"));
}

#[tokio::test]
async fn randomx_readiness_requires_pool_connectivity_and_metrics_are_opt_in() {
  let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let pool = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.upstream.url = format!("http://{}", origin.local_addr().unwrap());
  config.pow.p2pool.port = pool.local_addr().unwrap().port();
  config.pow.policy.rules.as_mut().unwrap()[0]
    .challenge
    .as_mut()
    .unwrap()
    .algorithm = Some(rilgate_config::PowAlgorithm::RandomX);
  config.pow.policy.rules.as_mut().unwrap()[0]
    .challenge
    .as_mut()
    .unwrap()
    .difficulty = Some(1000);
  let gateway = app::router(cfg).await.unwrap();
  let client = reqwest::Client::new();
  let base = format!(
    "http://{}/.well-known/test-pow/api",
    gateway.operations_address()
  );
  assert_eq!(
    client
      .get(format!("{base}/ready"))
      .send()
      .await
      .unwrap()
      .status(),
    200
  );
  drop(pool);
  let response = client.get(format!("{base}/ready")).send().await.unwrap();
  assert_eq!(response.status(), 503);
  assert_eq!(
    serde_json::from_slice::<serde_json::Value>(&response.bytes().await.unwrap()).unwrap()["pool"],
    false
  );
  assert_eq!(
    client
      .get(format!("{base}/metrics"))
      .send()
      .await
      .unwrap()
      .status(),
    404
  );
}

#[tokio::test]
async fn connections_are_bounded_and_capacity_is_reclaimed() {
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().server.max_connections = 1;
  let gateway = app::router(cfg).await.unwrap();
  let mut first = TcpStream::connect(gateway.operations_address())
    .await
    .unwrap();
  first
    .write_all(b"GET /.well-known/test-pow/api/live HTTP/1.1\r\nHost: example.test\r\n\r\n")
    .await
    .unwrap();
  let mut buffer = [0; 4096];
  let n = first.read(&mut buffer).await.unwrap();
  assert!(buffer[..n].starts_with(b"HTTP/1.1 200"));
  let mut excess = TcpStream::connect(gateway.address()).await.unwrap();
  let read = tokio::time::timeout(Duration::from_secs(2), excess.read(&mut buffer))
    .await
    .unwrap();
  assert!(matches!(read, Ok(0)) || read.is_err());
  drop(first);
  let client = reqwest::Client::new();
  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      if client
        .get(format!(
          "http://{}/.well-known/test-pow/api/live",
          gateway.operations_address()
        ))
        .send()
        .await
        .is_ok_and(|r| r.status() == 200)
      {
        break;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .unwrap();
}

#[tokio::test]
async fn in_flight_requests_are_bounded_and_released_after_completion() {
  use axum::{Router, routing::get};
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config();
  let config = Arc::get_mut(&mut cfg).unwrap();
  config.server.max_requests = 1;
  config.upstream.url = format!("http://{}", listener.local_addr().unwrap());
  let entered = Arc::new(tokio::sync::Notify::new());
  let release = Arc::new(tokio::sync::Notify::new());
  let (signal, wait) = (entered.clone(), release.clone());
  let origin = tokio::spawn(async move {
    axum::serve(
      listener,
      Router::new().route(
        "/",
        get(move || {
          let (signal, wait) = (signal.clone(), wait.clone());
          async move {
            signal.notify_one();
            wait.notified().await;
            "done"
          }
        }),
      ),
    )
    .await
    .unwrap();
  });
  let gateway = app::router(cfg).await.unwrap();
  let client = reqwest::Client::new();
  let url = format!("http://{}/", gateway.address());
  let pending = tokio::spawn(client.get(&url).send());
  tokio::time::timeout(Duration::from_secs(2), entered.notified())
    .await
    .unwrap();
  let excess = client
    .get(format!(
      "http://{}/.well-known/test-pow/api/live",
      gateway.operations_address()
    ))
    .send()
    .await
    .unwrap();
  assert_eq!(excess.status(), 503);
  assert_eq!(excess.headers()["retry-after"], "1");
  release.notify_one();
  assert_eq!(
    pending.await.unwrap().unwrap().text().await.unwrap(),
    "done"
  );
  let response = client
    .get(format!(
      "http://{}/.well-known/test-pow/api/live",
      gateway.operations_address()
    ))
    .send()
    .await
    .unwrap();
  assert_eq!(response.status(), 200);
  origin.abort();
}

#[tokio::test]
async fn http2_advertises_the_configured_stream_limit() {
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().server.h2_max_streams = 2;
  let gateway = app::router(cfg).await.unwrap();
  let mut socket = TcpStream::connect(gateway.address()).await.unwrap();
  socket
    .write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n\0\0\0\x04\0\0\0\0\0")
    .await
    .unwrap();
  tokio::time::timeout(Duration::from_secs(2), async {
    loop {
      let mut header = [0u8; 9];
      socket.read_exact(&mut header).await.unwrap();
      let length =
        (usize::from(header[0]) << 16) | (usize::from(header[1]) << 8) | usize::from(header[2]);
      assert!(length <= 65536);
      let mut payload = vec![0; length];
      socket.read_exact(&mut payload).await.unwrap();
      if header[3] == 4 && header[4] == 0 {
        let setting = payload
          .as_chunks::<6>()
          .0
          .iter()
          .find(|setting| setting[..2] == [0, 3])
          .expect("MAX_CONCURRENT_STREAMS setting");
        assert_eq!(u32::from_be_bytes(setting[2..].try_into().unwrap()), 2);
        break;
      }
    }
  })
  .await
  .unwrap();
}

#[tokio::test]
async fn unloaded_remote_ip_sets_fail_readiness_and_report_metrics() {
  use rilgate_config::schema::policy::{
    Action, Condition, FeedFormat, IpMatch, IpSetDefinition, Rule,
  };

  let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config();
  {
    let config = Arc::get_mut(&mut cfg).unwrap();
    config.upstream.url = format!("http://{}", origin.local_addr().unwrap());
    config.server.metrics_enabled = true;
    config.pow.policy.ip_sets.insert(
      "feed".into(),
      IpSetDefinition {
        url: Some("http://127.0.0.1:9/".into()),
        format: Some(FeedFormat::IpPrefixesJson),
        ..Default::default()
      },
    );
    config.pow.policy.rules = Some(vec![Rule {
      name: "blocked".into(),
      when: Condition {
        client_ip: Some(IpMatch {
          in_sets: Some(vec!["feed".into()]),
          cidr: None,
        }),
        ..Default::default()
      },
      action: Action::Deny,
      score: None,
      challenge: None,
    }]);
  }
  let (challenge, pool) = app::services(&cfg).unwrap();
  let gateway = app::TestGateway::start(
    rilgate_gateway::app::build(cfg.clone(), challenge.clone(), pool)
      .await
      .unwrap(),
  )
  .await
  .unwrap();
  let client = reqwest::Client::new();
  let base = format!(
    "http://{}/.well-known/test-pow/api",
    gateway.operations_address()
  );

  let ready = client.get(format!("{base}/ready")).send().await.unwrap();
  assert_eq!(ready.status(), 503);
  let body: serde_json::Value = serde_json::from_slice(&ready.bytes().await.unwrap()).unwrap();
  assert_eq!(body["ip_sets"], serde_json::Value::Bool(false));

  let protected = client
    .get(format!("http://{}/protected", gateway.address()))
    .send()
    .await
    .unwrap();
  assert_eq!(protected.status(), 200);
  assert!(
    protected.headers()["content-type"]
      .to_str()
      .unwrap()
      .contains("text/html")
  );

  let metrics = client
    .get(format!("{base}/metrics"))
    .send()
    .await
    .unwrap()
    .text()
    .await
    .unwrap();
  assert!(
    metrics.contains(r#"rilgate_ip_set_stale{set="feed"} 1"#),
    "{metrics}"
  );
  assert!(
    metrics.contains(r#"rilgate_ip_set_prefixes{set="feed"} 0"#),
    "{metrics}"
  );
  assert!(
    metrics.contains(r#"rilgate_ip_set_refresh_failures_total{set="feed"} 0"#),
    "{metrics}"
  );
  assert!(
    metrics.contains("rilgate_policy_unresolved_decisions_total 1"),
    "{metrics}"
  );

  challenge
    .ip_sets()
    .get("feed")
    .unwrap()
    .publish(rilgate_policy::IpRanges::from_networks(["192.0.2.0/24"
      .parse()
      .unwrap()]));

  let ready = client.get(format!("{base}/ready")).send().await.unwrap();
  assert_eq!(ready.status(), 200);
  let body: serde_json::Value = serde_json::from_slice(&ready.bytes().await.unwrap()).unwrap();
  assert_eq!(body["ip_sets"], serde_json::Value::Bool(true));
  let metrics = client
    .get(format!("{base}/metrics"))
    .send()
    .await
    .unwrap()
    .text()
    .await
    .unwrap();
  assert!(
    metrics.contains(r#"rilgate_ip_set_stale{set="feed"} 0"#),
    "{metrics}"
  );
  assert!(
    metrics.contains(r#"rilgate_ip_set_age_seconds{set="feed"} 0"#),
    "{metrics}"
  );
  drop(origin);
}

#[tokio::test]
async fn operations_listener_isolates_routes_and_preserves_methods_and_metrics_gating() {
  for metrics_enabled in [false, true] {
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut cfg = common::config();
    let config = Arc::get_mut(&mut cfg).unwrap();
    config.operations_listen_addresses = vec!["127.0.0.1:0".parse().unwrap()];
    config.server.metrics_enabled = metrics_enabled;
    config.upstream.url = format!("http://{}", origin.local_addr().unwrap());
    let gateway = app::router(cfg).await.unwrap();
    let public_address = gateway.address();
    let operations_address = gateway.operations_address();
    let client = reqwest::Client::new();
    let prefix = "/.well-known/test-pow";
    for route in ["health", "ready", "live", "metrics"] {
      for method in [
        reqwest::Method::GET,
        reqwest::Method::HEAD,
        reqwest::Method::POST,
      ] {
        let public = client
          .request(
            method.clone(),
            format!("http://{public_address}{prefix}/api/{route}"),
          )
          .send()
          .await
          .unwrap();
        assert_eq!(public.status(), 404);
        assert_eq!(public.headers()["cache-control"], "no-store");
        let response = client
          .request(
            method.clone(),
            format!("http://{operations_address}{prefix}/api/{route}"),
          )
          .send()
          .await
          .unwrap();
        let expected = if route == "metrics" && !metrics_enabled {
          404
        } else if method == reqwest::Method::POST {
          405
        } else {
          200
        };
        assert_eq!(response.status(), expected, "{route} {method}");
        assert_eq!(response.headers()["cache-control"], "no-store");
        if method == reqwest::Method::HEAD {
          assert!(response.bytes().await.unwrap().is_empty());
        }
      }
    }
    assert_eq!(
      client
        .get(format!("http://{public_address}{prefix}/api/client-config"))
        .send()
        .await
        .unwrap()
        .status(),
      200
    );
    for path in [
      format!("{prefix}/api/client-config"),
      format!("{prefix}/challenge"),
      format!("{prefix}/static/style.css"),
      "/upstream".into(),
      "/challenge".into(),
      "/.well-known/test-pow-other/api/live".into(),
    ] {
      assert_eq!(
        client
          .get(format!("http://{operations_address}{path}"))
          .send()
          .await
          .unwrap()
          .status(),
        404,
        "{path}"
      );
    }
  }
}

#[tokio::test]
async fn socket_remote_readiness_probes_only_required_hashers() {
  use rilgate_config::{Environment, PowAlgorithm, PowEnforcement, schema::pow::Hasher};
  use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
  let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let pool = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let remote = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let endpoint = format!("http://{}", remote.local_addr().unwrap());
  let calls = Arc::new(AtomicUsize::new(0));
  let available = Arc::new(AtomicBool::new(true));
  let request_calls = calls.clone();
  let request_available = available.clone();
  let server = tokio::spawn(async move {
    axum::serve(
      remote,
      axum::Router::new().route(
        "/v1/ready",
        axum::routing::get(move || {
          let calls = request_calls.clone();
          let available = request_available.clone();
          async move {
            calls.fetch_add(1, Ordering::SeqCst);
            if available.load(Ordering::SeqCst) {
              http::StatusCode::OK
            } else {
              http::StatusCode::SERVICE_UNAVAILABLE
            }
          }
        }),
      ),
    )
    .await
    .unwrap();
  });
  for (enforce, algorithm, order, required) in [
    (
      PowEnforcement::LocalVerify,
      PowAlgorithm::RandomX,
      vec![Hasher::Remote],
      true,
    ),
    (
      PowEnforcement::LocalVerify,
      PowAlgorithm::Fast,
      vec![Hasher::Remote],
      false,
    ),
    (
      PowEnforcement::LocalVerify,
      PowAlgorithm::RandomX,
      vec![Hasher::Remote, Hasher::Local],
      false,
    ),
    (
      PowEnforcement::LocalVerify,
      PowAlgorithm::RandomX,
      vec![Hasher::Local, Hasher::Remote],
      false,
    ),
    (
      PowEnforcement::P2poolAccepted,
      PowAlgorithm::RandomX,
      vec![Hasher::Remote],
      false,
    ),
  ] {
    let mut cfg = common::config();
    let config = Arc::get_mut(&mut cfg).unwrap();
    config.env = Environment::Development;
    config.upstream.url = format!("http://{}", origin.local_addr().unwrap());
    config.pow.p2pool.port = pool.local_addr().unwrap().port();
    config.pow.enforce = enforce;
    config.pow.randomx_verify.order = order;
    config.pow.randomx_verify.remote.endpoint = Some(endpoint.clone());
    let challenge = config.pow.policy.rules.as_mut().unwrap()[0]
      .challenge
      .as_mut()
      .unwrap();
    challenge.algorithm = Some(algorithm);
    challenge.difficulty = Some(if algorithm == PowAlgorithm::RandomX {
      1000
    } else {
      1
    });
    let gateway = app::router(cfg).await.unwrap();
    let client = reqwest::Client::new();
    let url = format!(
      "http://{}/.well-known/test-pow/api/ready",
      gateway.operations_address()
    );
    for healthy in [true, false] {
      available.store(healthy, Ordering::SeqCst);
      let previous = calls.load(Ordering::SeqCst);
      let response = client.get(&url).send().await.unwrap();
      assert_eq!(
        response.status().as_u16(),
        if required && !healthy { 503 } else { 200 }
      );
      let body: serde_json::Value =
        serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
      assert_eq!(
        body["remote"],
        if required {
          serde_json::Value::Bool(healthy)
        } else {
          serde_json::Value::Null
        }
      );
      assert_eq!(
        calls.load(Ordering::SeqCst) - previous,
        usize::from(required)
      );
    }
  }
  server.abort();
}
