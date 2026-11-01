use axum::{Router, extract::State, routing::get};
use rilgate_ip_feeds::{spawn, spawn_geoip};
use rilgate_policy::{Membership, PolicyEngine, policy::schema::Definition};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../rilgate-policy/tests/data");
const DEADLINE: Duration = Duration::from_secs(30);
const QUIET_WINDOW: Duration = Duration::from_millis(1500);

struct Feed {
  base: String,
  hits: Arc<AtomicUsize>,
}

async fn serve(State(hits): State<Arc<AtomicUsize>>, body: &'static str) -> &'static str {
  hits.fetch_add(1, Ordering::SeqCst);
  body
}

async fn feed_server() -> Feed {
  let hits = Arc::new(AtomicUsize::new(0));
  let app = Router::new()
    .route(
      "/first",
      get(|state| serve(state, r#"{"prefixes":[{"ipv4Prefix":"192.0.2.0/24"}]}"#)),
    )
    .route(
      "/second",
      get(|state| serve(state, r#"{"prefixes":[{"ipv4Prefix":"198.51.100.0/24"}]}"#)),
    )
    .with_state(hits.clone());
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let base = format!("http://{}", listener.local_addr().unwrap());
  tokio::spawn(async move {
    axum::serve(listener, app).await.unwrap();
  });
  Feed { base, hits }
}

fn definition(text: &str) -> Definition {
  serde_yaml_ng::from_str(text).unwrap()
}

fn feed_policy(url: &str, rule_action: &str) -> Definition {
  definition(&format!(
    "mode: custom\nip_sets:\n  feed: {{url: '{url}', format: ip_prefixes_json, refresh: 1h, max_stale: 2h}}\nrules:\n  - {{name: listed, when: {{client_ip: {{in_sets: [feed]}}}}, action: {rule_action}}}\n"
  ))
}

async fn until(what: &str, condition: impl Fn() -> bool) {
  let deadline = Instant::now() + DEADLINE;
  while !condition() {
    assert!(Instant::now() < deadline, "timed out waiting for {what}");
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
}

#[tokio::test]
async fn reload_keeps_an_unchanged_remote_ip_set_loaded() {
  let feed = feed_server().await;
  let url = format!("{}/first", feed.base);
  let first = PolicyEngine::from_definition(&feed_policy(&url, "allow")).unwrap();
  let _first_refreshers = spawn(&first.sets().ip).unwrap();
  let set = first.sets().ip.get("feed").unwrap().clone();
  until("the first load", || set.status().loaded).await;

  let second =
    PolicyEngine::from_definition_with(&feed_policy(&url, "deny"), Some(&first)).unwrap();
  let carried = second.sets().ip.get("feed").unwrap().clone();
  let ip = "192.0.2.1".parse().unwrap();
  assert!(carried.status().loaded);
  assert_eq!(carried.membership(ip), Membership::Member);
  assert!(second.sets().ip.all_remote_loaded());

  let _second_refreshers = spawn(&second.sets().ip).unwrap();
  tokio::time::sleep(QUIET_WINDOW).await;
  assert_eq!(feed.hits.load(Ordering::SeqCst), 1);
  assert_eq!(carried.membership(ip), Membership::Member);
}

#[tokio::test]
async fn reload_reloads_a_set_whose_source_changed() {
  let feed = feed_server().await;
  let first =
    PolicyEngine::from_definition(&feed_policy(&format!("{}/first", feed.base), "allow")).unwrap();
  let _first_refreshers = spawn(&first.sets().ip).unwrap();
  let set = first.sets().ip.get("feed").unwrap().clone();
  until("the first load", || set.status().loaded).await;

  let moved = feed_policy(&format!("{}/second", feed.base), "allow");
  let second = PolicyEngine::from_definition_with(&moved, Some(&first)).unwrap();
  let replaced = second.sets().ip.get("feed").unwrap().clone();
  assert!(!replaced.status().loaded);

  let _second_refreshers = spawn(&second.sets().ip).unwrap();
  until("the new source to load", || replaced.status().loaded).await;
  assert_eq!(
    replaced.membership("198.51.100.1".parse().unwrap()),
    Membership::Member
  );
  assert_eq!(
    replaced.membership("192.0.2.1".parse().unwrap()),
    Membership::NotMember
  );
}

fn geoip_policy(path: &Path, score: i32) -> Definition {
  definition(&format!(
    "mode: custom\ngeoip:\n  asn: {{path: '{}'}}\nrules:\n  - {{name: google, when: {{asn: [15169]}}, action: score, score: {score}}}\n",
    path.display()
  ))
}

#[tokio::test]
async fn reload_keeps_an_unchanged_geoip_database_without_reading_it_again() {
  let directory = tempfile::tempdir().unwrap();
  let file = directory.path().join("asn.mmdb");
  std::fs::copy(format!("{DATA}/GeoLite2-ASN-Test.mmdb"), &file).unwrap();
  let first = PolicyEngine::from_definition(&geoip_policy(&file, 5)).unwrap();
  let _first_refreshers = spawn_geoip(first.geoip()).unwrap();
  let database = first.geoip().asn().unwrap().clone();
  until("the first load", || database.status().loaded).await;

  // A new read of the missing file would record a failure.
  std::fs::remove_file(&file).unwrap();
  let second = PolicyEngine::from_definition_with(&geoip_policy(&file, 7), Some(&first)).unwrap();
  let carried = second.geoip().asn().unwrap().clone();
  assert!(carried.status().loaded);
  assert!(second.geoip().all_loaded());

  let _second_refreshers = spawn_geoip(second.geoip()).unwrap();
  tokio::time::sleep(QUIET_WINDOW).await;
  let status = carried.status();
  assert!(status.loaded);
  assert_eq!(status.refresh_failures, 0);
}

#[tokio::test]
async fn reload_reloads_a_geoip_database_whose_path_changed() {
  let directory = tempfile::tempdir().unwrap();
  let (old, new) = (
    directory.path().join("old.mmdb"),
    directory.path().join("new.mmdb"),
  );
  for file in [&old, &new] {
    std::fs::copy(format!("{DATA}/GeoLite2-ASN-Test.mmdb"), file).unwrap();
  }
  let first = PolicyEngine::from_definition(&geoip_policy(&old, 5)).unwrap();
  let _first_refreshers = spawn_geoip(first.geoip()).unwrap();
  let database = first.geoip().asn().unwrap().clone();
  until("the first load", || database.status().loaded).await;

  let second = PolicyEngine::from_definition_with(&geoip_policy(&new, 5), Some(&first)).unwrap();
  let replaced = second.geoip().asn().unwrap().clone();
  assert!(!replaced.status().loaded);

  std::fs::remove_file(&old).unwrap();
  let _second_refreshers = spawn_geoip(second.geoip()).unwrap();
  until("the new file to load", || replaced.status().loaded).await;
  assert_eq!(replaced.status().refresh_failures, 0);
}
