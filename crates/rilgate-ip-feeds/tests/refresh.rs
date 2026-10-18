use axum::{
  Router,
  extract::State,
  http::{HeaderMap, StatusCode, header},
  response::{IntoResponse, Response},
  routing::get,
};
use rilgate_ip_feeds::{FeedClient, Outcome, Validators, next_delay, refresh_once};
use rilgate_policy::policy::schema::Definition;
use rilgate_policy::{Membership, PolicyEngine};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
fn delays_add_bounded_jitter_and_back_off_to_the_refresh_interval() {
  let day = Duration::from_secs(86_400);
  let hour = Duration::from_secs(3600);
  assert_eq!(next_delay(0, true, day, 0.0), day);
  assert_eq!(next_delay(0, true, day, 0.5), day + day / 20);
  assert_eq!(next_delay(1, true, day, 0.0), Duration::from_secs(300));
  assert_eq!(next_delay(2, true, day, 0.0), Duration::from_secs(600));
  assert_eq!(next_delay(20, true, day, 0.0), day);
  assert_eq!(next_delay(3, true, hour, 0.0), Duration::from_secs(1200));
  assert_eq!(next_delay(4, true, hour, 0.0), Duration::from_secs(2400));
  assert_eq!(next_delay(5, true, hour, 0.0), hour);
  assert_eq!(next_delay(1, true, day, 0.5), Duration::from_secs(315));
}

#[test]
fn sets_that_never_loaded_retry_quickly_up_to_the_first_retry() {
  let day = Duration::from_secs(86_400);
  assert_eq!(next_delay(1, false, day, 0.0), Duration::from_secs(15));
  assert_eq!(next_delay(2, false, day, 0.0), Duration::from_secs(30));
  assert_eq!(next_delay(5, false, day, 0.0), Duration::from_secs(240));
  assert_eq!(next_delay(6, false, day, 0.0), Duration::from_secs(300));
  assert_eq!(next_delay(40, false, day, 0.0), Duration::from_secs(300));
  let jittered = next_delay(1, false, day, 0.999);
  assert!(jittered > Duration::from_secs(15) && jittered < Duration::from_millis(16_500));
}

#[derive(Clone)]
struct FeedState {
  body: Arc<Mutex<&'static str>>,
}

async fn feed(State(state): State<FeedState>, headers: HeaderMap) -> Response {
  if headers
    .get(header::IF_NONE_MATCH)
    .and_then(|value| value.to_str().ok())
    == Some("\"v1\"")
  {
    return StatusCode::NOT_MODIFIED.into_response();
  }
  let body = *state.body.lock().unwrap();
  let mut response = body.to_string().into_response();
  response
    .headers_mut()
    .insert(header::ETAG, "\"v1\"".parse().unwrap());
  response
}

#[tokio::test]
async fn failures_keep_the_previous_snapshot() {
  let state = FeedState {
    body: Arc::new(Mutex::new(
      r#"{"prefixes":[{"ipv4Prefix":"192.0.2.0/24"}]}"#,
    )),
  };
  let app = Router::new()
    .route("/feed", get(feed))
    .with_state(state.clone());
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  tokio::spawn(async move {
    axum::serve(listener, app).await.unwrap();
  });

  let definition: Definition = serde_yaml_ng::from_str(&format!(
    "mode: custom\nip_sets:\n  feed: {{url: 'http://{address}/feed', format: ip_prefixes_json, refresh: 1h, max_stale: 2h}}\nrules: []\n"
  ))
  .unwrap();
  let engine = PolicyEngine::from_definition(&definition).unwrap();
  let set = engine.ip_sets().get("feed").unwrap().clone();
  let ip = "192.0.2.1".parse().unwrap();
  let client = FeedClient::new().unwrap();
  let mut validators = Validators::default();

  assert_eq!(
    refresh_once(&client, &set, &mut validators).await,
    Outcome::Updated
  );
  assert_eq!(set.membership(ip), Membership::Member);
  assert_eq!(
    refresh_once(&client, &set, &mut validators).await,
    Outcome::Unchanged
  );

  *state.body.lock().unwrap() = "<html>";
  validators = Validators::default();
  assert_eq!(
    refresh_once(&client, &set, &mut validators).await,
    Outcome::Failed
  );
  assert_eq!(set.membership(ip), Membership::Member);
  assert_eq!(set.status().refresh_failures, 1);
}
