mod common;
use axum::{
  body::{Body, to_bytes},
  http::{Request, StatusCode},
};
use rilgate_challenge::ChallengeService;
use rilgate_gateway::app;
use rilgate_policy::{ChallengePolicy, StepPolicy};
use rilgate_pow::{
  Algorithm, AlgorithmDescriptor, AlgorithmFuture, AlgorithmRegistry, PreparedWork, Proof,
  WorkRequest,
};
use rilgate_pow_hash::{FastAlgorithm, FastConfig, sha256::verifier::verify_fast};
use rilgate_pow_hashwx::{HashWxAlgorithm, proof::seed};
use rilgate_protocol::{PowAlgorithm, PowVerification};
use rilgate_test_utils::gateway::TestGateway;
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;

struct SecondAlgorithm(HashWxAlgorithm);
impl Algorithm for SecondAlgorithm {
  fn descriptor(&self) -> AlgorithmDescriptor {
    self.0.descriptor()
  }
  fn difficulty_for_weight(&self, _: i32) -> u64 {
    1
  }
  fn validate_difficulty(&self, difficulty: u64) -> anyhow::Result<()> {
    self.0.validate_difficulty(difficulty)
  }
  fn validate_verification(&self, verification: PowVerification) -> anyhow::Result<()> {
    self.0.validate_verification(verification)
  }
  fn challenge_ttl(&self) -> u64 {
    self.0.challenge_ttl()
  }
  fn issue(&self, request: WorkRequest) -> AlgorithmFuture<'_, PreparedWork> {
    self.0.issue(request)
  }
  fn verify<'a>(&'a self, work: &'a PreparedWork, proof: Proof<'a>) -> AlgorithmFuture<'a, ()> {
    self.0.verify(work, proof)
  }
}
async fn request(
  router: &TestGateway,
  path: &str,
  cookie: Option<&str>,
  body: Option<Value>,
) -> axum::response::Response {
  let mut builder = Request::builder()
    .uri(path)
    .header("host", "example.test")
    .header("accept", "application/json");
  if let Some(cookie) = cookie {
    builder = builder.header("cookie", cookie);
  }
  let body = if let Some(body) = body {
    builder = builder
      .method("POST")
      .header("content-type", "application/json");
    Body::from(body.to_string())
  } else {
    Body::empty()
  };
  router
    .clone()
    .oneshot(builder.body(body).unwrap())
    .await
    .unwrap()
}
async fn json_body(response: axum::response::Response) -> Value {
  serde_json::from_slice(&to_bytes(response.into_body(), 1_048_576).await.unwrap()).unwrap()
}

#[tokio::test]
async fn completed_step_survives_cookie_merge_and_gate_forwards_after_both_proofs() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let upstream = tokio::spawn(async move {
    axum::serve(
      listener,
      axum::Router::new().fallback(|| async { "forwarded" }),
    )
    .await
    .unwrap();
  });
  let mut config = common::config();
  let cfg = Arc::get_mut(&mut config).unwrap();
  cfg.upstream.url = format!("http://{address}");
  cfg.pow.policy.policy.rules.as_mut().unwrap()[0].challenge = Some(ChallengePolicy {
    steps: Some(vec![
      StepPolicy {
        hook: None,
        algorithm: Some(PowAlgorithm::Fast),
        difficulty: Some(1),
      },
      StepPolicy {
        hook: None,
        algorithm: Some(PowAlgorithm::Hashwx),
        difficulty: None,
      },
    ]),
    ..Default::default()
  });
  let mut registry = AlgorithmRegistry::default();
  registry
    .register(Arc::new(FastAlgorithm::new(&FastConfig {
      ttl: config.pow.challenge_ttl_seconds,
    })))
    .unwrap();
  registry
    .register(Arc::new(SecondAlgorithm(
      HashWxAlgorithm::new(&config.pow.hashwx).unwrap(),
    )))
    .unwrap();
  let service =
    Arc::new(ChallengeService::with_registry(Arc::new(config.pow.challenge()), registry).unwrap());
  let pool =
    Arc::new(rilgate_pool::StratumPoolProvider::new(config.pow.stratum_pool.clone()).unwrap());
  let router = TestGateway::start(app::build(config.clone(), service, pool).await.unwrap())
    .await
    .unwrap();
  let prefix = &config.web.base_path;
  let response = request(&router, "/protected", None, None).await;
  assert_eq!(response.status(), StatusCode::FORBIDDEN);
  let challenge = json_body(response).await;
  assert_eq!(challenge["challenge"]["step"], 1);
  let mut presentation = challenge["challenge"]["token"].as_str().unwrap().to_owned();
  let mut cookie = None;
  for index in 1..=2 {
    let work = json_body(
      request(
        &router,
        &format!("{prefix}/api/challenge"),
        cookie.as_deref(),
        Some(json!({"presentation_token": presentation})),
      )
      .await,
    )
    .await;
    let difficulty = work["difficulty"].as_u64().unwrap();
    let challenge = work["challenge"].as_str().unwrap();
    let nonce = if index == 1 {
      (0..100_000)
        .map(|nonce| nonce.to_string())
        .find(|nonce| verify_fast(challenge, nonce, difficulty).unwrap())
        .unwrap()
    } else {
      let challenge: [u8; 32] = hex::decode(challenge).unwrap().try_into().unwrap();
      let mut hasher = hashwx_core::HashWx::new(&seed(&challenge, 0));
      let nonces_per_seed = work["nonces_per_seed"].as_u64().unwrap();
      (0..100_000u64)
        .find(|nonce| {
          if nonce % nonces_per_seed == 0 {
            hasher = hashwx_core::HashWx::new(&seed(&challenge, nonce / nonces_per_seed));
          }
          hasher.hash(*nonce) < u64::MAX / difficulty
        })
        .unwrap()
        .to_string()
    };
    let response = request(&router, &format!("{prefix}/api/verify"), cookie.as_deref(), Some(json!({"challenge_token": work["challenge_token"], "nonce": nonce, "set_cookie": true, "presentation_token": presentation}))).await;
    assert_eq!(response.status(), StatusCode::OK);
    cookie = Some(
      response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned(),
    );
    let verified = json_body(response).await;
    if index == 1 {
      assert_eq!(verified["next"]["step"], 2);
      presentation = verified["next"]["presentation_token"]
        .as_str()
        .unwrap()
        .to_owned();
      let response = request(&router, "/protected", cookie.as_deref(), None).await;
      assert_eq!(response.status(), StatusCode::FORBIDDEN);
      let challenge = json_body(response).await;
      assert_eq!(challenge["challenge"]["step"], 2);
      assert_eq!(challenge["challenge"]["algorithm"], "hashwx");
      assert_eq!(
        verified["next"]["solver_module_url"],
        challenge["challenge"]["solver_module_url"]
      );
    } else {
      assert!(verified.get("next").is_none());
    }
  }
  let response = request(&router, "/protected", cookie.as_deref(), None).await;
  assert_eq!(response.status(), StatusCode::OK);
  assert_eq!(
    to_bytes(response.into_body(), 1024).await.unwrap(),
    "forwarded"
  );
  upstream.abort();
}
