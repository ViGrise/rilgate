use rilgate_test_utils::binding::unbound;
mod common;

use anyhow::{Result, ensure};
use rilgate_challenge::ChallengeService;
use rilgate_crypto::token::{self as signer, TokenPurpose};
use rilgate_pow::{
  Algorithm, AlgorithmDescriptor, AlgorithmFuture, AlgorithmRegistry, PowAlgorithm, PreparedWork,
  Proof, WorkRequest,
};
use rilgate_pow_hash::{FastAlgorithm, FastConfig, sha256::verifier::verify_fast};
use rilgate_protocol::{ChallengeResponse, claims::ChallengeClaims};
use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};

struct CountingAlgorithm {
  inner: FastAlgorithm,
  verified: Arc<AtomicUsize>,
}

impl Algorithm for CountingAlgorithm {
  fn descriptor(&self) -> AlgorithmDescriptor {
    self.inner.descriptor()
  }

  fn difficulty_for_weight(&self, _: i32) -> u64 {
    2
  }

  fn validate_difficulty(&self, difficulty: u64) -> Result<()> {
    ensure!(difficulty == 2, "Expected difficulty 2");
    Ok(())
  }

  fn challenge_ttl(&self) -> u64 {
    self.inner.challenge_ttl()
  }

  fn issue(&self, request: WorkRequest) -> AlgorithmFuture<'_, PreparedWork> {
    self.inner.issue(request)
  }

  fn verify<'a>(&'a self, work: &'a PreparedWork, proof: Proof<'a>) -> AlgorithmFuture<'a, ()> {
    self.verified.fetch_add(1, Ordering::SeqCst);
    self.inner.verify(work, proof)
  }
}

#[tokio::test]
async fn registered_backend_controls_policy_and_proof_while_common_checks_run_first() {
  let mut configuration = common::config();
  Arc::get_mut(&mut configuration)
    .unwrap()
    .pow
    .policy
    .policy
    .rules
    .as_mut()
    .unwrap()[0]
    .challenge
    .as_mut()
    .unwrap()
    .difficulty = None;
  let cfg = Arc::new(configuration.pow.clone());
  let verified = Arc::new(AtomicUsize::new(0));
  let mut registry = AlgorithmRegistry::default();
  registry
    .register(Arc::new(CountingAlgorithm {
      inner: FastAlgorithm::new(&FastConfig {
        ttl: cfg.challenge_ttl_seconds,
      }),
      verified: verified.clone(),
    }))
    .unwrap();
  let service = ChallengeService::with_registry(Arc::new(cfg.challenge()), registry).unwrap();
  let decision = service.evaluate_request(&http::HeaderMap::new(), "/protected");
  assert_eq!(decision.difficulty, Some(2));
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
    panic!("Expected fast challenge");
  };
  let nonce = (0..100_000)
    .map(|value| value.to_string())
    .find(|nonce| verify_fast(&challenge, nonce, difficulty).unwrap())
    .unwrap();

  assert!(
    service
      .verify_challenge(&challenge_token, &nonce, "other.test")
      .await
      .is_err()
  );
  let mut claims: ChallengeClaims = signer::verify_json(
    &challenge_token,
    cfg.signing_key.expose().as_bytes(),
    TokenPurpose::Challenge,
  )
  .unwrap();
  if let ChallengeClaims::Fast { v, .. } = &mut claims {
    *v = 2;
  }
  let unsupported = signer::sign_json(
    &claims,
    cfg.signing_key.expose().as_bytes(),
    TokenPurpose::Challenge,
  )
  .unwrap();
  assert!(
    service
      .verify_challenge(&unsupported, &nonce, "example.test")
      .await
      .is_err()
  );
  assert_eq!(verified.load(Ordering::SeqCst), 0);

  let receipt = service
    .verify_challenge(&challenge_token, &nonce, "example.test")
    .await
    .unwrap();
  let auth = service
    .exchange_receipt(&receipt, "example.test", &unbound())
    .unwrap();
  assert_eq!(
    service
      .verify_auth_token(&auth, "example.test", &unbound())
      .unwrap()
      .algorithm,
    PowAlgorithm::Fast
  );
  assert_eq!(verified.load(Ordering::SeqCst), 1);
  assert!(service.algorithm_descriptor(PowAlgorithm::RandomX).is_err());
}

#[test]
fn startup_uses_registered_backend_validation_and_rejects_missing_algorithms() {
  let cfg = Arc::new(common::config().pow.clone());
  let mut registry = AlgorithmRegistry::default();
  registry
    .register(Arc::new(CountingAlgorithm {
      inner: FastAlgorithm::new(&FastConfig {
        ttl: cfg.challenge_ttl_seconds,
      }),
      verified: Default::default(),
    }))
    .unwrap();
  assert!(ChallengeService::with_registry(Arc::new(cfg.challenge()), registry).is_err());

  let mut registry = AlgorithmRegistry::default();
  registry
    .register(Arc::new(FastAlgorithm::new(&FastConfig {
      ttl: cfg.challenge_ttl_seconds,
    })))
    .unwrap();
  assert!(
    registry
      .register(Arc::new(FastAlgorithm::new(&FastConfig {
        ttl: cfg.challenge_ttl_seconds
      })))
      .is_err()
  );
  assert!(registry.get(PowAlgorithm::Fast).is_ok());
  let mut configuration = (*cfg).clone();
  configuration.default_algorithm = PowAlgorithm::RandomX;
  assert!(ChallengeService::with_registry(Arc::new(configuration.challenge()), registry).is_err());
}

struct UnusedWorkProvider;

impl rilgate_pool::WorkProvider for UnusedWorkProvider {
  type Work = rilgate_protocol::StratumJob;

  fn fetch(&self, _: u64) -> rilgate_core::BoxFuture<'_, Self::Work> {
    Box::pin(async { unreachable!("Signed work does not fetch a pool job") })
  }
}

#[derive(Debug, Clone, Copy)]
enum VerifierOutcome {
  Unavailable,
  Overloaded,
  Invalid,
  TargetMiss,
  Verified,
  Pending,
}

struct FakeRandomXHasher {
  outcome: VerifierOutcome,
  calls: Arc<AtomicUsize>,
}

impl rilgate_pow_randomx::RandomXHasher for FakeRandomXHasher {
  fn kind(&self) -> rilgate_pow_randomx::HasherKind {
    rilgate_pow_randomx::HasherKind::Local
  }

  fn hash(
    &self,
    _: Vec<u8>,
    _: Vec<u8>,
    _: std::time::Instant,
  ) -> futures_util::future::BoxFuture<'_, Result<[u8; 32], rilgate_pow_randomx::HashError>> {
    use rilgate_pow_randomx::HashError;
    self.calls.fetch_add(1, Ordering::SeqCst);
    Box::pin(async move {
      match self.outcome {
        VerifierOutcome::Unavailable => {
          Err(HashError::Unavailable(anyhow::anyhow!("Verifier stopped")))
        }
        VerifierOutcome::Overloaded => Err(HashError::Unavailable(
          rilgate_core::error::CapacityExceeded.into(),
        )),
        VerifierOutcome::Invalid => Err(HashError::Invalid(anyhow::anyhow!("Invalid input"))),
        VerifierOutcome::TargetMiss => Ok([255; 32]),
        VerifierOutcome::Verified => Ok([0; 32]),
        VerifierOutcome::Pending => std::future::pending().await,
      }
    })
  }
}

#[tokio::test]
async fn randomx_verify_handler_maps_verifier_outcomes_to_http_status() {
  use http::{HeaderMap, StatusCode};
  use rilgate_gateway::{
    proof::{dto::VerifyRequest, handlers},
    proxy::client::ProxyService,
    runtime::state::AppState,
    transport::{errors::ApiError, rate_limit::RateLimiter},
  };
  use rilgate_pow_randomx::{OrderedHasher, RandomXAlgorithm, RandomXConfig};
  use rilgate_protocol::PowVerification;

  for (outcome, expected) in [
    (
      VerifierOutcome::Unavailable,
      StatusCode::SERVICE_UNAVAILABLE,
    ),
    (VerifierOutcome::Overloaded, StatusCode::SERVICE_UNAVAILABLE),
    (VerifierOutcome::Invalid, StatusCode::UNAUTHORIZED),
    (VerifierOutcome::TargetMiss, StatusCode::UNAUTHORIZED),
    (VerifierOutcome::Verified, StatusCode::OK),
    (VerifierOutcome::Pending, StatusCode::UNAUTHORIZED),
  ] {
    let config = common::config();
    let calls = Arc::new(AtomicUsize::new(0));
    let cfg = &config.pow;
    let mut registry = AlgorithmRegistry::default();
    registry
      .register(Arc::new(FastAlgorithm::new(&FastConfig {
        ttl: cfg.challenge_ttl_seconds,
      })))
      .unwrap();
    registry
      .register(Arc::new(RandomXAlgorithm::with_hasher(
        &RandomXConfig {
          difficulty: cfg.difficulty.clone(),
          ttl: cfg.randomx_challenge_ttl_seconds,
          solver_module_path: "/static/js/solvers/randomx/solver.js".into(),
          max_workers: 8,
          store_cache: true,
          job_details: true,
          nonce_offset: cfg.nonce_offset,
          queue_capacity: cfg.verifier_queue_capacity,
          cache_idle_timeout: cfg.randomx_cache_idle_timeout,
        },
        Arc::new(UnusedWorkProvider),
        OrderedHasher::new(vec![Arc::new(FakeRandomXHasher {
          outcome,
          calls: calls.clone(),
        })]),
      )))
      .unwrap();
    let service =
      Arc::new(ChallengeService::with_registry(Arc::new(cfg.challenge()), registry).unwrap());
    let pool = Arc::new(rilgate_pool::StratumPoolProvider::new(cfg.stratum_pool.clone()).unwrap());
    let web = rilgate_web::WebManager::new(config.web.clone()).unwrap();
    let state = AppState {
      randomx_fallback_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
      connections: Arc::new(tokio::sync::Semaphore::new(config.server.max_connections)),
      requests: Arc::new(tokio::sync::Semaphore::new(config.server.max_requests)),
      probes: Arc::new(tokio::sync::Semaphore::new(1)),
      metrics: Arc::new(Default::default()),
      cfg: config.clone(),
      pow: service.clone(),
      pool,
      open_graph: Arc::new(
        rilgate_gateway::open_graph::OpenGraphService::new(
          config.open_graph.clone(),
          &service.sets(),
          &service.geoip(),
        )
        .unwrap(),
      ),
      policy_services: rilgate_gateway::runtime::policy::PolicyServices {
        dns: None,
        honeypot: None,
        counters: Default::default(),
      }
      .into_shared(),
      proxy: Arc::new(ProxyService::new(config.upstream.clone()).unwrap()),
      limiter: Arc::new(RateLimiter::new(config.pow.rate_limit.clone()).unwrap()),
      web_runtime: Arc::new(rilgate_web::runtime::watcher::runtime(&web)),
      web,
      shutdown: tokio_util::sync::CancellationToken::new(),
    };
    let now = rilgate_core::time::unix_now().unwrap();
    let claims = ChallengeClaims::Randomx {
      v: 1,
      challenge_id: "work".into(),
      verification: PowVerification::Gateway,
      host: "example.test".into(),
      blob: "00".repeat(64),
      seed_hash: "ab".repeat(32),
      target: "ffffffff".into(),
      difficulty: 1000,
      nonce_offset: 39,
      issued_at: now,
      expires_at: now
        + if matches!(outcome, VerifierOutcome::Pending) {
          1
        } else {
          60
        },
    };
    let token = signer::sign_json(
      &claims,
      cfg.signing_key.expose().as_bytes(),
      TokenPurpose::Challenge,
    )
    .unwrap();
    let uri = format!("https://example.test{}/api/verify", config.web.base_path)
      .parse()
      .unwrap();
    let request: VerifyRequest = serde_json::from_value(serde_json::json!({
      "challenge_token": token,
      "nonce": "00000000",
    }))
    .unwrap();
    let response = handlers::verify(&state, &HeaderMap::new(), &uri, request, &unbound())
      .await
      .unwrap_or_else(ApiError::into_response);
    assert_eq!(response.status(), expected, "outcome {outcome:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "outcome {outcome:?}");
  }
}
