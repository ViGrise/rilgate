use hashwx_core::HashWx;
use rilgate_core::time::unix_now;
use rilgate_pow::{
  Algorithm, PowAlgorithm, PreparedWork, Proof, Work, WorkRequest, difficulty::DifficultyConfig,
};
use rilgate_pow_hashwx::{HashWxAlgorithm, HashWxConfig, MAX_DIFFICULTY, proof::seed};
use rilgate_protocol::PowEnforcement;

fn config() -> HashWxConfig {
  HashWxConfig {
    nonces_per_seed: 463,
    difficulty: DifficultyConfig {
      base: 3,
      weight_step: 2,
      max: 10,
    },
    ttl: 60,
  }
}
fn work() -> PreparedWork {
  let now = unix_now().unwrap();
  PreparedWork {
    challenge_id: "test".into(),
    issued_at: now,
    expires_at: now + 60,
    work: Work::Hashwx {
      challenge: "ab".repeat(32),
      difficulty: 1000,
      nonces_per_seed: 463,
    },
  }
}

#[tokio::test]
async fn proof_binds_the_challenge_seed_batch_and_target() {
  let algorithm = HashWxAlgorithm::new(&config()).unwrap();
  algorithm
    .verify(&work(), Proof { nonce: "1159" })
    .await
    .unwrap();
  for changed in [
    Work::Hashwx {
      challenge: "ac".repeat(32),
      difficulty: 1000,
      nonces_per_seed: 463,
    },
    Work::Hashwx {
      challenge: "ab".repeat(32),
      difficulty: 1000,
      nonces_per_seed: 65536,
    },
    Work::Hashwx {
      challenge: "ab".repeat(32),
      difficulty: MAX_DIFFICULTY,
      nonces_per_seed: 463,
    },
  ] {
    let mut work = work();
    work.work = changed;
    assert!(
      algorithm
        .verify(&work, Proof { nonce: "1159" })
        .await
        .is_err()
    );
  }
}

#[tokio::test]
async fn malformed_unsigned_dimensions_and_nonce_encodings_are_rejected() {
  let algorithm = HashWxAlgorithm::new(&config()).unwrap();
  for nonce in ["", "01159", "+1159", "-1", "1159 ", "18446744073709551616"] {
    assert!(
      algorithm.verify(&work(), Proof { nonce }).await.is_err(),
      "{nonce}"
    );
  }
  for changed in [
    Work::Hashwx {
      challenge: "ab".repeat(32),
      difficulty: 1000,
      nonces_per_seed: 0,
    },
    Work::Hashwx {
      challenge: "ab".repeat(32),
      difficulty: 0,
      nonces_per_seed: 463,
    },
    Work::Hashwx {
      challenge: "ab".repeat(31),
      difficulty: 1000,
      nonces_per_seed: 463,
    },
    Work::Fast {
      challenge: "ab".repeat(32),
      difficulty: 1,
    },
  ] {
    let mut work = work();
    work.work = changed;
    assert!(
      algorithm
        .verify(&work, Proof { nonce: "1159" })
        .await
        .is_err()
    );
  }
}

#[test]
fn seed_encoding_retains_the_full_little_endian_batch_and_weight_mapping_saturates() {
  assert_eq!(
    hex::encode(seed(&[0xab; 32], 0x123456789abcdef0)),
    "cff2fd74fe11c48d9908ea19ae62f673e6540159570920de0277e488f5280b78"
  );
  let algorithm = HashWxAlgorithm::new(&config()).unwrap();
  for (weight, difficulty) in [
    (i32::MIN, 3),
    (0, 3),
    (1, 3),
    (2, 6),
    (4, 10),
    (i32::MAX, 10),
  ] {
    assert_eq!(algorithm.difficulty_for_weight(weight), difficulty);
  }
}

#[tokio::test]
async fn issuance_is_local_and_rejects_mining_enforcement() {
  let algorithm = HashWxAlgorithm::new(&config()).unwrap();
  let first = algorithm
    .issue(WorkRequest {
      difficulty: 1,
      enforce: PowEnforcement::LocalVerify,
    })
    .await
    .unwrap();
  let second = algorithm
    .issue(WorkRequest {
      difficulty: 1,
      enforce: PowEnforcement::LocalVerify,
    })
    .await
    .unwrap();
  let (
    Work::Hashwx {
      challenge: first_challenge,
      ..
    },
    Work::Hashwx {
      challenge: second_challenge,
      ..
    },
  ) = (&first.work, &second.work)
  else {
    panic!("Expected HashWX work")
  };
  assert_ne!(first_challenge, second_challenge);
  assert_eq!(hex::decode(first_challenge).unwrap().len(), 32);
  assert_eq!(first.algorithm(), PowAlgorithm::Hashwx);
  algorithm
    .verify(&first, Proof { nonce: "0" })
    .await
    .unwrap();
  assert!(
    algorithm
      .issue(WorkRequest {
        difficulty: 1,
        enforce: PowEnforcement::StratumPoolAccepted
      })
      .await
      .is_err()
  );
  let nonce = u64::MAX;
  let hash = HashWx::new(&seed(&[0xab; 32], nonce / 463)).hash(nonce);
  assert!(hash < u64::MAX);
  algorithm
    .verify(
      &work_with_easy_target(),
      Proof {
        nonce: "18446744073709551615",
      },
    )
    .await
    .unwrap();
}

fn work_with_easy_target() -> PreparedWork {
  let mut work = work();
  if let Work::Hashwx { difficulty, .. } = &mut work.work {
    *difficulty = 1;
  }
  work
}
