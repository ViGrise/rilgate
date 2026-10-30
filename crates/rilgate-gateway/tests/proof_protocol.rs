use rilgate_gateway::proof::websocket::messages::ServerWsMessage;
use rilgate_test_utils::binding::unbound;
use rilgate_test_utils::gateway as app;
use rilgate_test_utils::gateway::service as make_service;
mod common;

use axum::http::{HeaderMap, Uri};
use rilgate_crypto::token::{self as crypto, TokenPurpose};
use rilgate_gateway::config::{PowAlgorithm, PowEnforcement};
use rilgate_gateway::transport::context::request_host;
use rilgate_protocol::{
  ChallengeResponse, StratumJob,
  claims::{ChallengeClaims, PresentationClaims, ReceiptClaims},
};
use serde_json::{Value, json};
use std::{
  sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
  },
  time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
  io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
  net::{TcpListener, TcpStream},
};

fn now() -> i64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap()
    .as_secs() as i64
}

fn receipt(issued_at: i64) -> ReceiptClaims {
  ReceiptClaims {
    host: "example.test".into(),
    v: 1,
    challenge_id: "proof".into(),
    algorithm: PowAlgorithm::Fast,
    difficulty: 1,
    enforce: PowEnforcement::LocalVerify,
    issued_at,
    expires_at: issued_at + 30,
  }
}

#[tokio::test]
async fn receipts_and_access_are_host_bound_and_exchange_preserves_proof_time() {
  let cfg = common::config();
  let proof = receipt(now() - 5);
  let token = crypto::sign_json(
    &proof,
    cfg.pow.signing_key.expose().as_bytes(),
    TokenPurpose::Receipt,
  )
  .unwrap();
  let service = make_service(&cfg).unwrap();
  for host in ["other.test", ""] {
    assert!(service.exchange_receipt(&token, host, &unbound()).is_err());
  }
  let auth = service
    .exchange_receipt(&token, "example.test", &unbound())
    .unwrap();
  let claims = service
    .verify_auth_token(&auth, "example.test", &unbound())
    .unwrap();
  assert_eq!(claims.issued_at, proof.issued_at);
  assert_eq!(
    claims.expires_at,
    proof.issued_at + cfg.pow.auth_ttl_seconds as i64
  );
  assert_eq!(
    auth,
    service
      .exchange_receipt(&token, "example.test", &unbound())
      .unwrap()
  );
  for host in ["other.test", ""] {
    assert!(service.verify_auth_token(&auth, host, &unbound()).is_err());
  }
  let cookie = rilgate_gateway::auth::cookies::auth_cookie_header(
    &service,
    &cfg,
    &auth,
    "example.test",
    &unbound(),
  )
  .unwrap();
  let max_age: i64 = cookie
    .to_str()
    .unwrap()
    .split("Max-Age=")
    .nth(1)
    .unwrap()
    .split(';')
    .next()
    .unwrap()
    .parse()
    .unwrap();
  assert!(max_age <= cfg.pow.auth_ttl_seconds as i64 - 5);
}

#[tokio::test]
async fn signed_claims_require_supported_version_and_valid_lifetime() {
  let cfg = common::config();
  let service = make_service(&cfg).unwrap();
  let mut invalid = vec![];
  let mut claims = receipt(now());
  claims.v = 2;
  invalid.push(claims);
  let mut claims = receipt(now() + 60);
  claims.expires_at += 60;
  invalid.push(claims);
  let mut claims = receipt(now());
  claims.expires_at = claims.issued_at;
  invalid.push(claims);
  let mut claims = receipt(now());
  claims.expires_at += 1;
  invalid.push(claims);
  for claims in invalid {
    let token = crypto::sign_json(
      &claims,
      cfg.pow.signing_key.expose().as_bytes(),
      TokenPurpose::Receipt,
    )
    .unwrap();
    assert!(
      service
        .exchange_receipt(&token, "example.test", &unbound())
        .is_err()
    );
  }
  let legacy_auth = json!({"v": 1, "algorithm": "fast", "difficulty": 1, "enforce": "local_verify", "issued_at": now(), "expires_at": now() + 300});
  let token = crypto::sign_json(
    &legacy_auth,
    cfg.pow.auth_signing_key.expose().as_bytes(),
    TokenPurpose::Auth,
  )
  .unwrap();
  assert!(
    service
      .verify_auth_token(&token, "example.test", &unbound())
      .is_err()
  );
  let decision = service.evaluate_request(&HeaderMap::new(), "/protected");
  let token = service
    .issue_presentation("example.test".into(), "/protected".into(), &decision)
    .unwrap();
  assert!(service.verify_presentation(&token, "").is_err());
  let mut presentation: PresentationClaims = crypto::verify_json(
    &token,
    cfg.pow.signing_key.expose().as_bytes(),
    TokenPurpose::Presentation,
  )
  .unwrap();
  presentation.v = 2;
  let token = crypto::sign_json(
    &presentation,
    cfg.pow.signing_key.expose().as_bytes(),
    TokenPurpose::Presentation,
  )
  .unwrap();
  assert!(service.verify_presentation(&token, "example.test").is_err());
}

#[tokio::test]
async fn old_receipts_cannot_mint_access_after_the_proof_access_window() {
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().pow.auth_ttl_seconds = 1;
  let proof = receipt(now() - 5);
  let token = crypto::sign_json(
    &proof,
    cfg.pow.signing_key.expose().as_bytes(),
    TokenPurpose::Receipt,
  )
  .unwrap();
  assert!(
    make_service(&cfg)
      .unwrap()
      .exchange_receipt(&token, "example.test", &unbound())
      .is_err()
  );
}

#[test]
fn request_authority_supports_http2_and_normalizes_case() {
  let uri: Uri = "https://EXAMPLE.test:8443/protected".parse().unwrap();
  assert_eq!(request_host(&HeaderMap::new(), &uri), "example.test:8443");
  let mut headers = HeaderMap::new();
  headers.insert("host", "Other.test".parse().unwrap());
  assert_eq!(request_host(&headers, &uri), "other.test");
}

#[tokio::test]
async fn identical_pool_jobs_produce_distinct_local_work_with_exact_difficulty() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let port = listener.local_addr().unwrap().port();
  let mut cfg = common::config();
  let pow = &mut Arc::get_mut(&mut cfg).unwrap().pow;
  pow.stratum_pool.pools[0].port = port;
  pow.policy.rules.as_mut().unwrap()[0]
    .challenge
    .as_mut()
    .unwrap()
    .algorithm = Some(PowAlgorithm::RandomX);
  pow.policy.rules.as_mut().unwrap()[0]
    .challenge
    .as_mut()
    .unwrap()
    .difficulty = Some(u64::MAX);

  // Issuance pools its Stratum sessions, so the mock has to behave like P2Pool
  // and keep each connection open rather than closing it after the login reply.
  // Every connection is retained so the count of accepted logins can be checked.
  let accepted = Arc::new(AtomicUsize::new(0));
  let sessions: Arc<tokio::sync::Mutex<Vec<BufReader<TcpStream>>>> =
    Arc::new(tokio::sync::Mutex::new(Vec::new()));
  let pool = {
    let (accepted, sessions) = (accepted.clone(), sessions.clone());
    tokio::spawn(async move {
      while let Ok((socket, _)) = listener.accept().await {
        accepted.fetch_add(1, Ordering::SeqCst);
        let mut reader = BufReader::new(socket);
        let mut line = String::new();
        if reader.read_line(&mut line).await.is_err() {
          continue;
        }
        let response = json!({"id": 1, "result": {"id": "miner", "job": {
          "blob": "00".repeat(64), "seed_hash": "ab".repeat(32), "target": "ffffffff", "job_id": "same", "algo": "rx/0"
        }}});
        if reader
          .get_mut()
          .write_all(format!("{response}\n").as_bytes())
          .await
          .is_err()
        {
          continue;
        }
        // Retaining the reader keeps both halves of the socket alive, so the
        // pooled session sees a connection that stays open.
        sessions.lock().await.push(reader);
      }
    })
  };

  let service = make_service(&cfg).unwrap();
  let decision = service.evaluate_request(&HeaderMap::new(), "/protected");
  let presentation = service
    .issue_presentation("example.test".into(), "/protected".into(), &decision)
    .unwrap();

  let mut blobs = vec![];
  for _ in 0..2 {
    let response = service
      .issue_challenge(&presentation, "example.test")
      .await
      .unwrap();
    let wire = serde_json::to_value(&response).unwrap();
    assert_eq!(wire["difficulty"], u64::MAX.to_string());
    let ChallengeResponse::Randomx {
      job, expires_at, ..
    } = response
    else {
      panic!("expected RandomX")
    };
    assert!(expires_at >= now() + cfg.pow.randomx_challenge_ttl_seconds as i64 - 1);
    assert_eq!(&job.blob[..128], "00".repeat(64));
    assert_eq!(job.blob.len() / 2, 64 + 7 + 16);
    assert_eq!(job.seed_hash, "ab".repeat(32));
    blobs.push(job.blob);
  }

  // The job id is identical across issuances, so distinct work can only come
  // from the per-issuance challenge binding.
  assert_ne!(blobs[0], blobs[1]);

  // Both issuances must have reused one pooled session, which is what keeps
  // issuance from opening a connection per challenge.
  assert_eq!(accepted.load(Ordering::SeqCst), 1);

  pool.abort();
}

#[tokio::test]
async fn an_unreachable_pool_reports_the_backend_as_unavailable() {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let port = listener.local_addr().unwrap().port();
  drop(listener);

  let mut cfg = common::config();
  let pow = &mut Arc::get_mut(&mut cfg).unwrap().pow;
  pow.stratum_pool.pools[0].port = port;
  pow.policy.rules.as_mut().unwrap()[0]
    .challenge
    .as_mut()
    .unwrap()
    .algorithm = Some(PowAlgorithm::RandomX);
  pow.policy.rules.as_mut().unwrap()[0]
    .challenge
    .as_mut()
    .unwrap()
    .difficulty = Some(1000);

  let service = make_service(&cfg).unwrap();
  let decision = service.evaluate_request(&HeaderMap::new(), "/protected");
  let presentation = service
    .issue_presentation("example.test".into(), "/protected".into(), &decision)
    .unwrap();

  let error = service
    .issue_challenge(&presentation, "example.test")
    .await
    .unwrap_err();
  assert!(error.is::<rilgate_pow::WorkUnavailable>());
}

#[test]
fn websocket_difficulty_round_trips_without_javascript_precision_loss() {
  let job = StratumJob {
    blob: "00".repeat(64),
    job_id: "job".into(),
    target: "ffffffff".into(),
    seed_hash: "ab".repeat(32),
    height: None,
    algo: None,
  };
  for difficulty in [1000, 9_007_199_254_740_991, 9_007_199_254_740_992, u64::MAX] {
    let message = ServerWsMessage::Job {
      job: job.clone(),
      difficulty,
      nonce_offset: 39,
    };
    let wire = serde_json::to_string(&message).unwrap();
    let value: Value = serde_json::from_str(&wire).unwrap();
    assert_eq!(
      value["difficulty"].is_string(),
      difficulty > 9_007_199_254_740_991
    );
    let decoded: ServerWsMessage = serde_json::from_str(&wire).unwrap();
    assert!(
      matches!(decoded, ServerWsMessage::Job { difficulty: actual, .. } if actual == difficulty)
    );
  }
}

#[tokio::test]
async fn a_fast_challenge_cannot_cross_enforcement_modes() {
  let cfg = common::config();
  let service = make_service(&cfg).unwrap();
  let decision = service.evaluate_request(&HeaderMap::new(), "/protected");
  let presentation = service
    .issue_presentation("example.test".into(), "/protected".into(), &decision)
    .unwrap();
  let ChallengeResponse::Fast {
    challenge_token,
    challenge,
    difficulty,
  } = service
    .issue_challenge(&presentation, "example.test")
    .await
    .unwrap()
  else {
    panic!("expected fast")
  };
  let nonce = (0..100_000)
    .map(|n| n.to_string())
    .find(|n| rilgate_pow_hash::sha256::verifier::verify_fast(&challenge, n, difficulty).unwrap())
    .unwrap();
  assert!(
    service
      .verify_challenge(&challenge_token, &nonce, "example.test")
      .await
      .is_ok()
  );
  drop(service);
  let mut changed = cfg;
  let pow = &mut Arc::get_mut(&mut changed).unwrap().pow;
  pow.enforce = PowEnforcement::StratumPoolAccepted;
  let selection = pow.policy.rules.as_mut().unwrap()[0]
    .challenge
    .as_mut()
    .unwrap();
  selection.algorithm = Some(PowAlgorithm::RandomX);
  selection.difficulty = Some(1000);
  let service = make_service(&changed).unwrap();
  assert!(
    service
      .verify_challenge(&challenge_token, &nonce, "example.test")
      .await
      .is_err()
  );
}

#[tokio::test]
async fn http2_exchange_and_proxy_preserve_host_scope() {
  use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::get,
  };
  use tower::ServiceExt;
  let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let mut cfg = common::config();
  Arc::get_mut(&mut cfg).unwrap().pow.auth_cookie_name = "custom_access".into();
  Arc::get_mut(&mut cfg).unwrap().upstream.url =
    format!("http://{}", upstream.local_addr().unwrap());
  let server = tokio::spawn(async move {
    axum::serve(
      upstream,
      Router::new().route("/protected", get(|| async { "upstream" })),
    )
    .await
    .unwrap();
  });
  let token = crypto::sign_json(
    &receipt(now()),
    cfg.pow.signing_key.expose().as_bytes(),
    TokenPurpose::Receipt,
  )
  .unwrap();
  let router = app::router(cfg.clone()).await.unwrap();
  let exchange_url = format!("https://example.test{}/api/exchange", cfg.web.base_path);
  let body = json!({"receipt": token}).to_string();
  let wrong = router
    .clone()
    .oneshot(
      Request::builder()
        .method("POST")
        .uri(exchange_url.clone())
        .header("host", "other.test")
        .header("content-type", "application/json")
        .body(Body::from(body.clone()))
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
  let response = router
    .clone()
    .oneshot(
      Request::builder()
        .method("POST")
        .uri(exchange_url)
        .version(axum::http::Version::HTTP_2)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap(),
    )
    .await
    .unwrap();
  assert_eq!(response.status(), StatusCode::OK);
  let cookie = response.headers()["set-cookie"]
    .to_str()
    .unwrap()
    .split(';')
    .next()
    .unwrap()
    .to_owned();
  assert!(cookie.starts_with("custom_access="));
  let default_cookie = cookie.replacen("custom_access=", "rilgate_auth=", 1);
  for (host, request_cookie, expected) in [
    ("example.test", &cookie, "upstream"),
    ("other.test", &cookie, "challenge"),
    ("example.test", &default_cookie, "challenge"),
  ] {
    let response = router
      .clone()
      .oneshot(
        Request::builder()
          .uri(format!("https://{host}/protected"))
          .version(axum::http::Version::HTTP_2)
          .header("cookie", request_cookie)
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let body = std::str::from_utf8(&bytes).unwrap();
    if expected == "upstream" {
      assert_eq!(body, "upstream");
    } else {
      assert!(body.contains("pow-bootstrap"));
    }
  }
  server.abort();
}

#[tokio::test]
async fn malformed_signed_randomx_work_is_rejected_before_native_hashing() {
  let cfg = common::config();
  let service = make_service(&cfg).unwrap();
  for (blob, seed, difficulty, offset, expected) in [
    ("00".repeat(64), "ab".into(), 1000, 39, "dimensions"),
    ("00".repeat(257), "ab".repeat(32), 1000, 39, "dimensions"),
    ("00".repeat(64), "ab".repeat(32), 0, 39, "difficulty"),
    ("00".repeat(64), "ab".repeat(32), 1000, 62, "offset"),
  ] {
    let claims = ChallengeClaims::Randomx {
      v: 1,
      challenge_id: "work".into(),
      enforce: PowEnforcement::LocalVerify,
      host: "example.test".into(),
      blob,
      seed_hash: seed,
      target: "ffffffff".into(),
      difficulty,
      nonce_offset: offset,
      issued_at: now(),
      expires_at: now() + 60,
    };
    let token = crypto::sign_json(
      &claims,
      cfg.pow.signing_key.expose().as_bytes(),
      TokenPurpose::Challenge,
    )
    .unwrap();
    let error = service
      .verify_challenge(&token, "00000000", "example.test")
      .await
      .unwrap_err();
    assert!(error.to_string().contains(expected), "{error}");
  }
}

#[tokio::test]
async fn randomx_http_proofs_require_local_verification_even_with_signed_matching_enforcement() {
  let mut cfg = common::config();
  let pow = &mut Arc::get_mut(&mut cfg).unwrap().pow;
  pow.enforce = PowEnforcement::StratumPoolAccepted;
  let selection = pow.policy.rules.as_mut().unwrap()[0]
    .challenge
    .as_mut()
    .unwrap();
  selection.algorithm = Some(PowAlgorithm::RandomX);
  selection.difficulty = Some(1000);
  let service = make_service(&cfg).unwrap();
  let claims = ChallengeClaims::Randomx {
    v: 1,
    challenge_id: "work".into(),
    enforce: PowEnforcement::StratumPoolAccepted,
    host: "example.test".into(),
    blob: "00".repeat(64),
    seed_hash: "ab".repeat(32),
    target: "ffffffff".into(),
    difficulty: 1000,
    nonce_offset: 39,
    issued_at: now(),
    expires_at: now() + 60,
  };
  let token = crypto::sign_json(
    &claims,
    cfg.pow.signing_key.expose().as_bytes(),
    TokenPurpose::Challenge,
  )
  .unwrap();
  let error = service
    .verify_challenge(&token, "00000000", "example.test")
    .await
    .unwrap_err();
  assert!(
    error
      .to_string()
      .contains("disabled in stratum_pool_accepted mode"),
    "{error}"
  );
}

#[tokio::test]
async fn rolling_key_rotation_preserves_the_full_cross_instance_proof_flow() {
  let mut current = common::config();
  let config = Arc::get_mut(&mut current).unwrap();
  config.pow.signing_keys.active_id = Some("old-proof".into());
  config.pow.auth_signing_keys.active_id = Some("old-auth".into());
  config.pow.signing_keys.previous.insert(
    "next-proof".into(),
    rilgate_config::Secret("next-proof-secret".into()),
  );
  config.pow.auth_signing_keys.previous.insert(
    "next-auth".into(),
    rilgate_config::Secret("next-auth-secret".into()),
  );
  let old = make_service(&current).unwrap();
  let mut next = common::config();
  let config = Arc::get_mut(&mut next).unwrap();
  config.pow.signing_keys.active_id = Some("next-proof".into());
  config.pow.auth_signing_keys.active_id = Some("next-auth".into());
  config
    .pow
    .signing_keys
    .previous
    .insert("old-proof".into(), current.pow.signing_key.clone());
  config
    .pow
    .auth_signing_keys
    .previous
    .insert("old-auth".into(), current.pow.auth_signing_key.clone());
  config.pow.signing_key = rilgate_config::Secret("next-proof-secret".into());
  config.pow.auth_signing_key = rilgate_config::Secret("next-auth-secret".into());
  let new = make_service(&next).unwrap();
  let decision = old.evaluate_request(&HeaderMap::new(), "/protected");
  let presentation = old
    .issue_presentation("example.test".into(), "/protected".into(), &decision)
    .unwrap();
  let ChallengeResponse::Fast {
    challenge_token,
    challenge,
    difficulty,
  } = new
    .issue_challenge(&presentation, "example.test")
    .await
    .unwrap()
  else {
    panic!("expected fast");
  };
  let nonce = (0..100_000)
    .map(|n| n.to_string())
    .find(|nonce| {
      rilgate_pow_hash::sha256::verifier::verify_fast(&challenge, nonce, difficulty).unwrap()
    })
    .unwrap();
  let receipt = old
    .verify_challenge(&challenge_token, &nonce, "example.test")
    .await
    .unwrap();
  let auth = new
    .exchange_receipt(&receipt, "example.test", &unbound())
    .unwrap();
  assert!(
    old
      .verify_auth_token(&auth, "example.test", &unbound())
      .is_ok()
  );
  assert!(
    new
      .verify_auth_token(&auth, "other.test", &unbound())
      .is_err()
  );
  let mut retired = current;
  let config = Arc::get_mut(&mut retired).unwrap();
  config.pow.signing_keys.previous.clear();
  config.pow.auth_signing_keys.previous.clear();
  let retired = make_service(&retired).unwrap();
  assert!(
    retired
      .verify_auth_token(&auth, "example.test", &unbound())
      .is_err()
  );
}
