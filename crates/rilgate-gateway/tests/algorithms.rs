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
