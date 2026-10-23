use super::*;
use bytes::Bytes;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::{
  Request, Response, StatusCode,
  body::{Frame, Incoming},
  service::service_fn,
};
use openssl::{
  asn1::Asn1Time,
  bn::BigNum,
  hash::MessageDigest,
  pkey::{PKey, Private},
  rsa::Rsa,
  x509::{
    X509, X509NameBuilder,
    extension::{BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAlternativeName},
  },
};
use rustls::server::WebPkiClientVerifier;
use std::{
  convert::Infallible,
  sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
  },
};

struct Certificates {
  directory: tempfile::TempDir,
  ca: X509,
  server: X509,
  server_key: PKey<Private>,
}
fn certificate(
  name: &str,
  serial: u32,
  issuer: Option<(&X509, &PKey<Private>)>,
  client: bool,
) -> (X509, PKey<Private>) {
  let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
  let mut subject = X509NameBuilder::new().unwrap();
  subject.append_entry_by_text("CN", name).unwrap();
  let subject = subject.build();
  let mut cert = X509::builder().unwrap();
  cert.set_version(2).unwrap();
  cert
    .set_serial_number(&BigNum::from_u32(serial).unwrap().to_asn1_integer().unwrap())
    .unwrap();
  cert.set_subject_name(&subject).unwrap();
  cert
    .set_issuer_name(issuer.map_or(subject.as_ref(), |(ca, _)| ca.subject_name()))
    .unwrap();
  cert.set_pubkey(&key).unwrap();
  cert
    .set_not_before(&Asn1Time::days_from_now(0).unwrap())
    .unwrap();
  cert
    .set_not_after(&Asn1Time::days_from_now(1).unwrap())
    .unwrap();
  if issuer.is_none() {
    cert
      .append_extension(BasicConstraints::new().critical().ca().build().unwrap())
      .unwrap();
    cert
      .append_extension(KeyUsage::new().key_cert_sign().crl_sign().build().unwrap())
      .unwrap();
  } else {
    cert
      .append_extension(BasicConstraints::new().critical().build().unwrap())
      .unwrap();
    cert
      .append_extension(
        KeyUsage::new()
          .digital_signature()
          .key_encipherment()
          .build()
          .unwrap(),
      )
      .unwrap();
    let usage = if client {
      ExtendedKeyUsage::new().client_auth().build()
    } else {
      ExtendedKeyUsage::new().server_auth().build()
    };
    cert.append_extension(usage.unwrap()).unwrap();
    let san = SubjectAlternativeName::new()
      .dns(name)
      .build(&cert.x509v3_context(issuer.map(|(ca, _)| ca.as_ref()), None))
      .unwrap();
    cert.append_extension(san).unwrap();
  }
  cert
    .sign(issuer.map_or(&key, |(_, key)| key), MessageDigest::sha256())
    .unwrap();
  (cert.build(), key)
}
impl Certificates {
  fn new() -> Self {
    let directory = tempfile::tempdir().unwrap();
    let (ca, ca_key) = certificate("test CA", 1, None, false);
    let (server, server_key) = certificate("verifier.test", 2, Some((&ca, &ca_key)), false);
    let (client, client_key) = certificate("gateway.test", 3, Some((&ca, &ca_key)), true);
    for (name, pem) in [
      ("ca", ca.to_pem().unwrap()),
      ("client", client.to_pem().unwrap()),
      ("key", client_key.private_key_to_pem_pkcs8().unwrap()),
    ] {
      std::fs::write(directory.path().join(name), pem).unwrap();
    }
    Self {
      directory,
      ca,
      server,
      server_key,
    }
  }
  fn client(
    &self,
    endpoint: &str,
    name: &str,
    identity: bool,
    trusted: bool,
    timeout: Duration,
  ) -> RemoteHasher {
    RemoteHasher::new(
      endpoint,
      name,
      timeout,
      trusted.then(|| self.directory.path().join("ca")).as_deref(),
      identity
        .then(|| self.directory.path().join("client"))
        .as_deref(),
      identity
        .then(|| self.directory.path().join("key"))
        .as_deref(),
    )
    .unwrap()
  }
}
struct Server {
  endpoint: String,
  request: Arc<Mutex<Option<(String, serde_json::Value)>>>,
  task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
  fn drop(&mut self) {
    self.task.abort();
  }
}
async fn server(
  certs: &Certificates,
  status: StatusCode,
  body: String,
  delay: Duration,
  body_delay: Duration,
) -> Server {
  let provider = Arc::new(rustls::crypto::ring::default_provider());
  let mut roots = rustls::RootCertStore::empty();
  roots
    .add(CertificateDer::from(certs.ca.to_der().unwrap()))
    .unwrap();
  let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
    .build()
    .unwrap();
  let tls = rustls::ServerConfig::builder_with_provider(provider)
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_client_cert_verifier(verifier)
    .with_single_cert(
      vec![CertificateDer::from(certs.server.to_der().unwrap())],
      PrivateKeyDer::from_pem_slice(&certs.server_key.private_key_to_pem_pkcs8().unwrap()).unwrap(),
    )
    .unwrap();
  let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let endpoint = format!("https://{}", listener.local_addr().unwrap());
  let request = Arc::new(Mutex::new(None));
  let received = request.clone();
  let task = tokio::spawn(async move {
    let (stream, _) = listener.accept().await.unwrap();
    let Ok(stream) = acceptor.accept(stream).await else {
      return;
    };
    let service = service_fn(move |req: Request<Incoming>| {
      let received = received.clone();
      let body = body.clone();
      async move {
        let path = req.uri().path().to_owned();
        let bytes = req.into_body().collect().await.unwrap().to_bytes();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        *received.lock().unwrap() = Some((path, json));
        tokio::time::sleep(delay).await;
        let chunks: Vec<_> = body
          .as_bytes()
          .chunks(512)
          .map(Bytes::copy_from_slice)
          .collect();
        let stream = futures_util::stream::iter(chunks).then(move |chunk| async move {
          tokio::time::sleep(body_delay).await;
          Ok::<_, Infallible>(Frame::data(chunk))
        });
        let response_body = if body.is_empty() {
          Full::new(Bytes::new()).boxed_unsync()
        } else {
          StreamBody::new(stream).boxed_unsync()
        };
        Ok::<_, Infallible>(
          Response::builder()
            .status(status)
            .header("location", "/v1/randomx/hash")
            .body(response_body)
            .unwrap(),
        )
      }
    });
    let _ = hyper::server::conn::http1::Builder::new()
      .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
      .await;
  });
  Server {
    endpoint,
    request,
    task,
  }
}
fn hash_body() -> String {
  serde_json::json!({"hash": hex::encode([7; 32])}).to_string()
}
async fn hash(client: &RemoteHasher) -> Result<[u8; 32], HashError> {
  client
    .hash(
      vec![1; 32],
      vec![2; 76],
      Instant::now() + Duration::from_secs(5),
    )
    .await
}
#[tokio::test]
async fn socket_happy_path_and_absolute_deadline() {
  let certs = Certificates::new();
  let server = server(
    &certs,
    StatusCode::OK,
    hash_body(),
    Duration::ZERO,
    Duration::ZERO,
  )
  .await;
  let client = certs.client(
    &server.endpoint,
    "verifier.test",
    true,
    true,
    Duration::from_secs(2),
  );
  let wall = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
  let expiry = wall + Duration::from_millis(500);
  let deadline = Instant::now() + Duration::from_millis(490);
  assert_eq!(
    client
      .hash(vec![1; 32], vec![2; 76], deadline)
      .await
      .unwrap(),
    [7; 32]
  );
  let received = server.request.lock().unwrap();
  let (path, body) = received.as_ref().unwrap();
  assert_eq!(path, "/v1/randomx/hash");
  assert_eq!(body["seed"], hex::encode([1; 32]));
  assert_eq!(body["input"], hex::encode([2; 76]));
  let deadline = body["deadline_unix_ms"].as_u64().unwrap();
  assert!(deadline > wall.as_millis() as u64);
  assert!(deadline <= expiry.as_millis() as u64);
}
#[tokio::test]
async fn socket_remote_timeout_bounds_deadline() {
  let certs = Certificates::new();
  let server = server(
    &certs,
    StatusCode::OK,
    hash_body(),
    Duration::ZERO,
    Duration::ZERO,
  )
  .await;
  let client = certs.client(
    &server.endpoint,
    "verifier.test",
    true,
    true,
    Duration::from_secs(1),
  );
  let wall = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
  hash(&client).await.unwrap();
  let received = server.request.lock().unwrap();
  let deadline = received.as_ref().unwrap().1["deadline_unix_ms"]
    .as_u64()
    .unwrap();
  assert!(deadline <= (wall + Duration::from_millis(1100)).as_millis() as u64);
}
#[tokio::test]
async fn socket_status_and_protocol_outcomes() {
  let certs = Certificates::new();
  for (status, body, invalid) in [
    (StatusCode::BAD_REQUEST, String::new(), true),
    (StatusCode::SERVICE_UNAVAILABLE, String::new(), false),
    (StatusCode::INTERNAL_SERVER_ERROR, hash_body(), false),
    (StatusCode::FOUND, hash_body(), false),
    (StatusCode::OK, "malformed".into(), false),
    (
      StatusCode::OK,
      serde_json::json!({"hash": "z".repeat(64)}).to_string(),
      false,
    ),
    (
      StatusCode::OK,
      serde_json::json!({"hash": "00".repeat(31)}).to_string(),
      false,
    ),
    (
      StatusCode::OK,
      format!("{}{}", hash_body(), " ".repeat(4096)),
      false,
    ),
  ] {
    let server = server(&certs, status, body, Duration::ZERO, Duration::ZERO).await;
    let client = certs.client(
      &server.endpoint,
      "verifier.test",
      true,
      true,
      Duration::from_secs(2),
    );
    let result = hash(&client).await;
    assert!(
      if invalid {
        matches!(result, Err(HashError::Invalid(_)))
      } else {
        matches!(result, Err(HashError::Unavailable(_)))
      },
      "{status}: {result:?}"
    );
  }
}
#[tokio::test]
async fn socket_request_and_body_timeouts() {
  let certs = Certificates::new();
  for (headers, body) in [
    (Duration::from_millis(300), Duration::ZERO),
    (Duration::ZERO, Duration::from_millis(300)),
  ] {
    let server = server(&certs, StatusCode::OK, hash_body(), headers, body).await;
    let client = certs.client(
      &server.endpoint,
      "verifier.test",
      true,
      true,
      Duration::from_millis(100),
    );
    let result = hash(&client).await;
    assert!(
      matches!(result, Err(HashError::Unavailable(ref error)) if matches!(error.downcast_ref(), Some(UnavailableReason::Timeout)))
    );
  }
}
#[tokio::test]
async fn socket_tls_rejects_untrusted_server_wrong_name_and_missing_client_identity() {
  let certs = Certificates::new();
  for (name, identity, trust) in [
    ("verifier.test", true, false),
    ("wrong.test", true, true),
    ("verifier.test", false, true),
  ] {
    let server = server(
      &certs,
      StatusCode::OK,
      hash_body(),
      Duration::ZERO,
      Duration::ZERO,
    )
    .await;
    let client = certs.client(
      &server.endpoint,
      name,
      identity,
      trust,
      Duration::from_secs(2),
    );
    assert!(matches!(
      hash(&client).await,
      Err(HashError::Unavailable(_))
    ));
    assert!(server.request.lock().unwrap().is_none());
  }
}
struct Fallback {
  calls: Arc<AtomicUsize>,
}
impl RandomXHasher for Fallback {
  fn kind(&self) -> HasherKind {
    HasherKind::Local
  }
  fn hash(&self, _: Vec<u8>, _: Vec<u8>, _: Instant) -> BoxFuture<'_, Result<[u8; 32], HashError>> {
    self.calls.fetch_add(1, Ordering::SeqCst);
    Box::pin(async { Ok([9; 32]) })
  }
}
#[tokio::test]
async fn socket_ordered_remote_fallback_only_on_unavailable() {
  let certs = Certificates::new();
  for (status, fallback) in [
    (StatusCode::SERVICE_UNAVAILABLE, true),
    (StatusCode::BAD_REQUEST, false),
    (StatusCode::OK, false),
  ] {
    let server = server(&certs, status, hash_body(), Duration::ZERO, Duration::ZERO).await;
    let remote = certs.client(
      &server.endpoint,
      "verifier.test",
      true,
      true,
      Duration::from_secs(2),
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let ordered = super::super::OrderedHasher::new(vec![
      Arc::new(remote),
      Arc::new(Fallback {
        calls: calls.clone(),
      }),
    ]);
    let result = ordered
      .hash(
        vec![1; 32],
        vec![2],
        Instant::now() + Duration::from_secs(3),
      )
      .await;
    assert_eq!(calls.load(Ordering::SeqCst), usize::from(fallback));
    match status {
      StatusCode::OK => assert_eq!(result.unwrap(), [7; 32]),
      StatusCode::BAD_REQUEST => assert!(matches!(result, Err(HashError::Invalid(_)))),
      _ => assert_eq!(result.unwrap(), [9; 32]),
    }
    let stats = ordered.diagnostics();
    assert_eq!(stats.remote_duration.iter().sum::<u64>(), 1);
    assert_eq!(stats.fallbacks[1][2], u64::from(fallback));
  }
}
#[tokio::test]
async fn socket_ready_uses_authenticated_client_and_status() {
  let certs = Certificates::new();
  for (status, expected) in [
    (StatusCode::OK, true),
    (StatusCode::SERVICE_UNAVAILABLE, false),
    (StatusCode::FOUND, false),
  ] {
    let server = server(
      &certs,
      status,
      String::new(),
      Duration::ZERO,
      Duration::ZERO,
    )
    .await;
    let client = certs.client(
      &server.endpoint,
      "verifier.test",
      true,
      true,
      Duration::from_secs(2),
    );
    assert_eq!(client.ready(Duration::from_secs(1)).await, expected);
    assert_eq!(
      server.request.lock().unwrap().as_ref().unwrap().0,
      "/v1/ready"
    );
  }
}
#[test]
fn certificate_load_errors_are_startup_errors() {
  assert!(
    RemoteHasher::new(
      "https://verifier.test",
      "verifier.test",
      Duration::from_secs(1),
      Some(Path::new("/nonexistent/ca.pem")),
      None,
      None
    )
    .is_err()
  );
  let certs = Certificates::new();
  for file in ["ca", "client", "key"] {
    let original = std::fs::read(certs.directory.path().join(file)).unwrap();
    std::fs::write(certs.directory.path().join(file), "invalid PEM").unwrap();
    assert!(
      RemoteHasher::new(
        "https://verifier.test",
        "verifier.test",
        Duration::from_secs(1),
        Some(&certs.directory.path().join("ca")),
        Some(&certs.directory.path().join("client")),
        Some(&certs.directory.path().join("key"))
      )
      .is_err()
    );
    std::fs::write(certs.directory.path().join(file), original).unwrap();
  }
}

#[tokio::test]
async fn socket_tls_rejects_untrusted_client_identity() {
  let certs = Certificates::new();
  let foreign = Certificates::new();
  let server = server(
    &certs,
    StatusCode::OK,
    hash_body(),
    Duration::ZERO,
    Duration::ZERO,
  )
  .await;
  let client = RemoteHasher::new(
    &server.endpoint,
    "verifier.test",
    Duration::from_secs(2),
    Some(&certs.directory.path().join("ca")),
    Some(&foreign.directory.path().join("client")),
    Some(&foreign.directory.path().join("key")),
  )
  .unwrap();
  assert!(matches!(
    hash(&client).await,
    Err(HashError::Unavailable(_))
  ));
  assert!(server.request.lock().unwrap().is_none());
}

#[tokio::test]
async fn expired_deadline_and_invalid_dimensions_do_not_send_requests() {
  let client = RemoteHasher::new(
    "http://127.0.0.1:9",
    "127.0.0.1",
    Duration::from_secs(1),
    None,
    None,
    None,
  )
  .unwrap();
  assert!(matches!(
    client.hash(vec![1], vec![2], Instant::now()).await,
    Err(HashError::Unavailable(_))
  ));
  for (seed, input) in [
    (vec![], vec![1]),
    (vec![1], vec![]),
    (vec![1; 257], vec![1]),
    (vec![1], vec![1; 257]),
  ] {
    assert!(matches!(
      client
        .hash(seed, input, Instant::now() + Duration::from_secs(1))
        .await,
      Err(HashError::Invalid(_))
    ));
  }
}

#[tokio::test]
async fn socket_ready_uses_probe_timeout() {
  let certs = Certificates::new();
  let server = server(
    &certs,
    StatusCode::OK,
    String::new(),
    Duration::from_millis(300),
    Duration::ZERO,
  )
  .await;
  let client = certs.client(
    &server.endpoint,
    "verifier.test",
    true,
    true,
    Duration::from_secs(2),
  );
  let started = Instant::now();
  assert!(!client.ready(Duration::from_millis(100)).await);
  assert!(started.elapsed() < Duration::from_millis(300));
}

#[tokio::test]
async fn deadline_rejection_is_distinct_and_policy_controls_fallback() {
  use super::super::DeadlinePolicy;
  use super::super::OrderedHasher;
  let certs = Certificates::new();
  for policy in [DeadlinePolicy::Fallback, DeadlinePolicy::Reject] {
    let server = server(
      &certs,
      StatusCode::SERVICE_UNAVAILABLE,
      r#"{"reason":"deadline"}"#.into(),
      Duration::ZERO,
      Duration::ZERO,
    )
    .await;
    let client = certs.client(
      &server.endpoint,
      "verifier.test",
      true,
      true,
      Duration::from_secs(2),
    );
    let error = hash(&client).await.unwrap_err();
    assert!(
      matches!(error, HashError::Unavailable(ref e) if matches!(e.downcast_ref::<UnavailableReason>(), Some(UnavailableReason::Deadline))),
      "{error:?}"
    );
    let ordered = OrderedHasher::new(vec![Arc::new(client)]).with_deadline_policy(policy);
    let result = ordered
      .hash(
        vec![1; 32],
        vec![2; 76],
        Instant::now() + Duration::from_secs(2),
      )
      .await;
    assert!(match policy {
      DeadlinePolicy::Fallback => matches!(result, Err(HashError::Unavailable(_))),
      DeadlinePolicy::Reject => matches!(result, Err(HashError::Invalid(_))),
    });
  }
}

#[test]
fn default_trust_roots_are_populated() {
  assert!(!trust_roots(None).unwrap().is_empty());
}
