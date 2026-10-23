use rilgate_pow::{PreparedWork, Work};
use rilgate_protocol::{ChallengeResponse, PowEnforcement, StratumJob, claims::ChallengeClaims};
pub(super) fn claims(
  work: &PreparedWork,
  host: String,
  enforce: PowEnforcement,
) -> ChallengeClaims {
  match &work.work {
    Work::Fast {
      challenge,
      difficulty,
    } => ChallengeClaims::Fast {
      v: 1,
      enforce,
      challenge_id: work.challenge_id.clone(),
      host,
      challenge: challenge.clone(),
      difficulty: *difficulty,
      issued_at: work.issued_at,
      expires_at: work.expires_at,
    },
    Work::Hashwx {
      challenge,
      difficulty,
      nonces_per_seed,
    } => ChallengeClaims::Hashwx {
      v: 1,
      enforce,
      challenge_id: work.challenge_id.clone(),
      host,
      challenge: challenge.clone(),
      difficulty: *difficulty,
      nonces_per_seed: *nonces_per_seed,
      issued_at: work.issued_at,
      expires_at: work.expires_at,
    },
    Work::Randomx {
      job,
      difficulty,
      nonce_offset,
    } => ChallengeClaims::Randomx {
      v: 1,
      enforce,
      challenge_id: work.challenge_id.clone(),
      host,
      blob: job.blob.clone(),
      seed_hash: job.seed_hash.clone(),
      target: job.target.clone(),
      difficulty: *difficulty,
      nonce_offset: *nonce_offset,
      issued_at: work.issued_at,
      expires_at: work.expires_at,
    },
  }
}
pub(super) fn prepared(claims: &ChallengeClaims) -> PreparedWork {
  let metadata = claims.metadata();
  let work = match claims {
    ChallengeClaims::Fast {
      challenge,
      difficulty,
      ..
    } => Work::Fast {
      challenge: challenge.clone(),
      difficulty: *difficulty,
    },
    ChallengeClaims::Hashwx {
      challenge,
      difficulty,
      nonces_per_seed,
      ..
    } => Work::Hashwx {
      challenge: challenge.clone(),
      difficulty: *difficulty,
      nonces_per_seed: *nonces_per_seed,
    },
    ChallengeClaims::Randomx {
      blob,
      seed_hash,
      target,
      difficulty,
      nonce_offset,
      ..
    } => Work::Randomx {
      job: StratumJob {
        blob: blob.clone(),
        job_id: String::new(),
        target: target.clone(),
        seed_hash: seed_hash.clone(),
        height: None,
        algo: None,
      },
      difficulty: *difficulty,
      nonce_offset: *nonce_offset,
    },
  };
  PreparedWork {
    challenge_id: metadata.challenge_id.to_string(),
    issued_at: metadata.issued_at,
    expires_at: metadata.expires_at,
    work,
  }
}
pub(super) fn response(work: PreparedWork, challenge_token: String) -> ChallengeResponse {
  match work.work {
    Work::Fast {
      challenge,
      difficulty,
    } => ChallengeResponse::Fast {
      challenge_token,
      challenge,
      difficulty,
    },
    Work::Hashwx {
      challenge,
      difficulty,
      nonces_per_seed,
    } => ChallengeResponse::Hashwx {
      challenge_token,
      challenge,
      difficulty,
      nonces_per_seed,
      expires_at: work.expires_at,
    },
    Work::Randomx {
      job,
      difficulty,
      nonce_offset,
    } => ChallengeResponse::Randomx {
      challenge_token,
      job,
      difficulty,
      nonce_offset,
      expires_at: work.expires_at,
    },
  }
}
