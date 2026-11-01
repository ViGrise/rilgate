mod common;
use rilgate_challenge::ChallengeService;
use rilgate_crypto::token::{self, TokenPurpose};
use rilgate_pow::{
  Algorithm, AlgorithmDescriptor, AlgorithmFuture, AlgorithmRegistry, PowAlgorithm, PreparedWork,
  Proof, WorkRequest,
};
use rilgate_pow_hashwx::{HashWxAlgorithm, proof::seed};
use rilgate_protocol::{ChallengeResponse, PowVerification, claims::ChallengeClaims};
use rilgate_test_utils::binding::unbound;
use std::sync::{
  Arc,
  atomic::{AtomicUsize, Ordering},
};

struct CountingHashWx {
  inner: HashWxAlgorithm,
  calls: Arc<AtomicUsize>,
}
impl Algorithm for CountingHashWx {
  fn descriptor(&self) -> AlgorithmDescriptor {
    self.inner.descriptor()
  }
  fn difficulty_for_weight(&self, weight: i32) -> u64 {
    self.inner.difficulty_for_weight(weight)
  }
  fn validate_difficulty(&self, difficulty: u64) -> anyhow::Result<()> {
    self.inner.validate_difficulty(difficulty)
  }
  fn validate_verification(&self, mode: PowVerification) -> anyhow::Result<()> {
    self.inner.validate_verification(mode)
  }
  fn challenge_ttl(&self) -> u64 {
    self.inner.challenge_ttl()
  }
  fn issue(&self, request: WorkRequest) -> AlgorithmFuture<'_, PreparedWork> {
    self.inner.issue(request)
  }
  fn verify<'a>(&'a self, work: &'a PreparedWork, proof: Proof<'a>) -> AlgorithmFuture<'a, ()> {
    self.calls.fetch_add(1, Ordering::SeqCst);
    self.inner.verify(work, proof)
  }
}

#[tokio::test]
async fn hashwx_lifecycle_binds_signed_work_and_checks_tokens_before_dispatch() {
  let mut config = common::config();
  let cfg = Arc::get_mut(&mut config).unwrap();
  cfg.pow.default_algorithm = PowAlgorithm::Hashwx;
  cfg.pow.hashwx.nonces_per_seed = 463;
  let selection = cfg.pow.policy.policy.rules.as_mut().unwrap()[0]
    .challenge
    .as_mut()
    .unwrap();
  selection.algorithm = Some(PowAlgorithm::Hashwx);
  selection.difficulty = Some(1000);
  let calls = Arc::new(AtomicUsize::new(0));
  assert!(
    ChallengeService::with_registry(
      Arc::new(config.pow.challenge()),
      AlgorithmRegistry::default()
    )
    .is_err()
  );
  let mut registry = AlgorithmRegistry::default();
  registry
    .register(Arc::new(CountingHashWx {
      inner: HashWxAlgorithm::new(&config.pow.hashwx).unwrap(),
      calls: calls.clone(),
    }))
    .unwrap();
  let service =
    ChallengeService::with_registry(Arc::new(config.pow.challenge()), registry).unwrap();
  let decision = service.evaluate_request(&http::HeaderMap::new(), "/protected");
  let presentation = service
    .issue_presentation("example.test".into(), "/protected".into(), &decision)
    .unwrap();
  let ChallengeResponse::Hashwx {
    challenge_token,
    challenge,
    difficulty,
    nonces_per_seed,
    expires_at,
    expires_in,
  } = service
    .issue_challenge(&presentation, "example.test")
    .await
    .unwrap()
  else {
    panic!("Expected HashWX work")
  };
  assert_eq!(nonces_per_seed, 463);
  assert!(expires_at > rilgate_core::time::unix_now().unwrap());
  assert!((1..=config.pow.hashwx.ttl).contains(&expires_in));
  let bytes: [u8; 32] = hex::decode(challenge).unwrap().try_into().unwrap();
  let mut hasher = hashwx_core::HashWx::new(&seed(&bytes, 0));
  let nonce = (0..100_000u64)
    .find(|nonce| {
      if nonce % u64::from(nonces_per_seed) == 0 {
        hasher = hashwx_core::HashWx::new(&seed(&bytes, nonce / u64::from(nonces_per_seed)));
      }
      hasher.hash(*nonce) < u64::MAX / difficulty
    })
    .unwrap()
    .to_string();
  let claims: ChallengeClaims = token::verify_json(
    &challenge_token,
    config.pow.signing_key.expose().as_bytes(),
    TokenPurpose::Challenge,
  )
  .unwrap();
  let mut bad_signature = challenge_token.clone().into_bytes();
  let signature_start = bad_signature
    .iter()
    .rposition(|byte| *byte == b'.')
    .unwrap()
    + 1;
  bad_signature[signature_start] = if bad_signature[signature_start] == b'A' {
    b'B'
  } else {
    b'A'
  };
  assert!(
    service
      .verify_challenge(
        &String::from_utf8(bad_signature).unwrap(),
        &nonce,
        "example.test"
      )
      .await
      .is_err()
  );
  assert!(
    service
      .verify_challenge(&challenge_token, &nonce, "other.test")
      .await
      .is_err()
  );
  for scenario in ["version", "expired", "verification", "difficulty"] {
    let mut modified = claims.clone();
    let ChallengeClaims::Hashwx {
      v,
      issued_at,
      expires_at,
      verification,
      difficulty,
      ..
    } = &mut modified
    else {
      unreachable!()
    };
    match scenario {
      "version" => *v = 2,
      "expired" => {
        *issued_at -= 120;
        *expires_at -= 120;
      }
      "verification" => *verification = PowVerification::PoolShare,
      "difficulty" => *difficulty = 0,
      _ => unreachable!(),
    }
    let signed = token::sign_json(
      &modified,
      config.pow.signing_key.expose().as_bytes(),
      TokenPurpose::Challenge,
    )
    .unwrap();
    assert!(
      service
        .verify_challenge(&signed, &nonce, "example.test")
        .await
        .is_err(),
      "{scenario}"
    );
  }
  assert_eq!(calls.load(Ordering::SeqCst), 0);
  let receipt = service
    .verify_challenge(&challenge_token, &nonce, "example.test")
    .await
    .unwrap();
  assert_eq!(calls.load(Ordering::SeqCst), 1);
  assert!(
    service
      .exchange_receipt(&receipt, "other.test", &unbound())
      .is_err()
  );
  let auth = service
    .exchange_receipt(&receipt, "example.test", &unbound())
    .unwrap();
  let verified = service
    .verify_auth_token(&auth, "example.test", &unbound())
    .unwrap();
  assert_eq!(verified.algorithm, PowAlgorithm::Hashwx);
  assert_eq!(verified.difficulty, difficulty);
  assert!(
    service
      .verify_auth_token(&auth, "other.test", &unbound())
      .is_err()
  );
}
