use axum::{
  Router,
  extract::State,
  http::{HeaderMap, StatusCode, header},
  response::{IntoResponse, Response},
  routing::get,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use http::Method;
use rilgate_ip_feeds::{GeoRefresher, GeoState, Outcome};
use rilgate_policy::{PolicyEngine, conditions::Request, policy::schema::Definition};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../rilgate-policy/tests/data");
const GOOGLE_ADDRESS: &str = "1.0.0.1";
const TELSTRA_ADDRESS: &str = "1.128.0.1";

fn engine(geoip: &str) -> PolicyEngine {
  let text = format!(
    "mode: custom\ngeoip:\n  asn: {geoip}\nrules:\n  - {{name: google, when: {{asn: [15169]}}, action: score, score: 5}}\n  - {{name: telstra, when: {{asn: [1221]}}, action: score, score: 7}}\n"
  );
  PolicyEngine::from_definition(&serde_yaml_ng::from_str::<Definition>(&text).unwrap()).unwrap()
}

fn weight(engine: &PolicyEngine, ip: &str) -> i32 {
  let headers = http::HeaderMap::new();
  engine
    .evaluate_context(
      &Request {
        host: "example.test",
        headers: &headers,
        path: "/",
        method: &Method::GET,
        client_ip: Some(ip.parse().unwrap()),
        lookups: &rilgate_policy::Lookups::NONE,
      },
      0,
    )
    .weight
}

fn archive(database: &[u8]) -> Vec<u8> {
  let mut tar = tar::Builder::new(Vec::new());
  for (name, body) in [
    ("GeoLite2-ASN_20261029/COPYRIGHT.txt", b"notice".as_slice()),
    ("GeoLite2-ASN_20261029/GeoLite2-ASN.mmdb", database),
  ] {
    let mut header = tar::Header::new_gnu();
    header.set_size(body.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, name, body).unwrap();
  }
  let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
  std::io::Write::write_all(&mut gzip, &tar.into_inner().unwrap()).unwrap();
  gzip.finish().unwrap()
}

#[derive(Clone)]
struct Download {
  body: Arc<Vec<u8>>,
  authorization: Arc<Mutex<Option<String>>>,
}

async fn download(State(state): State<Download>, headers: HeaderMap) -> Response {
  *state.authorization.lock().unwrap() = headers
    .get(header::AUTHORIZATION)
    .map(|value| value.to_str().unwrap().to_string());
  if state.authorization.lock().unwrap().is_none() {
    return StatusCode::UNAUTHORIZED.into_response();
  }
  state.body.to_vec().into_response()
}

async fn serve(body: Vec<u8>) -> (String, Download) {
  let state = Download {
    body: Arc::new(body),
    authorization: Arc::default(),
  };
  let app = Router::new()
    .route("/{edition}/download", get(download))
    .with_state(state.clone());
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  tokio::spawn(async move {
    axum::serve(listener, app).await.unwrap();
  });
  (format!("http://{address}"), state)
}

const MAXMIND: &str =
  "{maxmind: {edition: GeoLite2-ASN, account_id: 123456, license_key: secret-key}}";

#[tokio::test]
async fn maxmind_download_extracts_the_database() {
  let database = std::fs::read(format!("{DATA}/GeoLite2-ASN-Test.mmdb")).unwrap();
  let (base, state) = serve(archive(&database)).await;
  let engine = engine(MAXMIND);
  let asn = engine.geoip().asn().unwrap();
  assert!(!asn.status().loaded);
  let refresher = GeoRefresher::new().unwrap().with_maxmind_base(base);
  let outcome = refresher.refresh_once(asn, &mut GeoState::default()).await;
  assert_eq!(outcome, Outcome::Updated);
  assert!(asn.status().loaded);
  assert_eq!(weight(&engine, GOOGLE_ADDRESS), 5);
  let expected = format!("Basic {}", STANDARD.encode("123456:secret-key"));
  assert_eq!(
    state.authorization.lock().unwrap().as_deref(),
    Some(expected.as_str())
  );
}

#[tokio::test]
async fn oversized_download_is_refused() {
  let database = std::fs::read(format!("{DATA}/GeoLite2-ASN-Test.mmdb")).unwrap();
  let body = archive(&database);
  let limit = body.len() - 1;
  let (base, _) = serve(body).await;
  let engine = engine(MAXMIND);
  let asn = engine.geoip().asn().unwrap();
  let refresher = GeoRefresher::new()
    .unwrap()
    .with_maxmind_base(base)
    .with_max_bytes(limit);
  let outcome = refresher.refresh_once(asn, &mut GeoState::default()).await;
  assert_eq!(outcome, Outcome::Failed);
  assert!(!asn.status().loaded);
  assert_eq!(asn.status().refresh_failures, 1);
}

#[tokio::test]
async fn path_source_reloads_a_changed_file() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("asn.mmdb");
  let country = std::fs::read(format!("{DATA}/GeoLite2-Country-Test.mmdb")).unwrap();
  let asn_bytes = std::fs::read(format!("{DATA}/GeoLite2-ASN-Test.mmdb")).unwrap();
  // A database of the wrong type is refused and the file stays unloaded.
  std::fs::write(&path, &country).unwrap();
  let engine = engine(&format!("{{path: '{}'}}", path.display()));
  let asn = engine.geoip().asn().unwrap();
  let refresher = GeoRefresher::new().unwrap();
  let mut state = GeoState::default();
  assert_eq!(
    refresher.refresh_once(asn, &mut state).await,
    Outcome::Failed
  );
  assert!(!asn.status().loaded);

  std::fs::write(&path, &asn_bytes).unwrap();
  let later = SystemTime::now() + Duration::from_secs(60);
  std::fs::File::options()
    .write(true)
    .open(&path)
    .unwrap()
    .set_modified(later)
    .unwrap();
  assert_eq!(
    refresher.refresh_once(asn, &mut state).await,
    Outcome::Updated
  );
  assert_eq!(weight(&engine, TELSTRA_ADDRESS), 7);
  assert_eq!(
    refresher.refresh_once(asn, &mut state).await,
    Outcome::Unchanged
  );
}
