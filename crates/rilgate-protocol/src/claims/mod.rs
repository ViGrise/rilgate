use serde::{Deserialize, Serialize};

use crate::{PowAlgorithm, PowEnforcement};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresentationClaims {
  pub v: u8,
  pub host: String,
  pub path: String,
  pub rule: String,
  pub weight: i32,
  pub algorithm: PowAlgorithm,
  pub difficulty: u64,
  pub enforce: PowEnforcement,
  pub issued_at: i64,
  pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "algorithm", rename_all = "snake_case")]
pub enum ChallengeClaims {
  Fast {
    v: u8,
    enforce: PowEnforcement,
    challenge_id: String,
    host: String,
    challenge: String,
    difficulty: u64,
    issued_at: i64,
    expires_at: i64,
  },

  Hashwx {
    v: u8,
    enforce: PowEnforcement,
    challenge_id: String,
    host: String,
    challenge: String,
    difficulty: u64,
    nonces_per_seed: u32,
    issued_at: i64,
    expires_at: i64,
  },

  Randomx {
    v: u8,
    challenge_id: String,
    enforce: PowEnforcement,
    host: String,
    blob: String,
    seed_hash: String,
    target: String,
    difficulty: u64,
    nonce_offset: usize,
    issued_at: i64,
    expires_at: i64,
  },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptClaims {
  pub host: String,
  pub v: u8,
  pub challenge_id: String,
  pub algorithm: PowAlgorithm,
  pub difficulty: u64,
  pub enforce: PowEnforcement,
  pub issued_at: i64,
  pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthClaims {
  pub host: String,
  pub v: u8,
  pub algorithm: PowAlgorithm,
  pub difficulty: u64,
  pub enforce: PowEnforcement,
  pub issued_at: i64,
  pub expires_at: i64,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub binding: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub networks: Option<Vec<NetworkBinding>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkBinding {
  #[serde(rename = "d")]
  pub digest: String,
  #[serde(rename = "e")]
  pub expires_at: i64,
}

impl ChallengeClaims {
  pub fn metadata(&self) -> ChallengeMetadata<'_> {
    match self {
      Self::Fast {
        v,
        enforce,
        challenge_id,
        host,
        difficulty,
        issued_at,
        expires_at,
        ..
      } => ChallengeMetadata {
        algorithm: PowAlgorithm::Fast,
        version: *v,
        enforce: *enforce,
        challenge_id,
        host,
        difficulty: *difficulty,
        issued_at: *issued_at,
        expires_at: *expires_at,
      },
      Self::Hashwx {
        v,
        enforce,
        challenge_id,
        host,
        difficulty,
        issued_at,
        expires_at,
        ..
      } => ChallengeMetadata {
        algorithm: PowAlgorithm::Hashwx,
        version: *v,
        enforce: *enforce,
        challenge_id,
        host,
        difficulty: *difficulty,
        issued_at: *issued_at,
        expires_at: *expires_at,
      },
      Self::Randomx {
        v,
        enforce,
        challenge_id,
        host,
        difficulty,
        issued_at,
        expires_at,
        ..
      } => ChallengeMetadata {
        algorithm: PowAlgorithm::RandomX,
        version: *v,
        enforce: *enforce,
        challenge_id,
        host,
        difficulty: *difficulty,
        issued_at: *issued_at,
        expires_at: *expires_at,
      },
    }
  }
}
pub struct ChallengeMetadata<'a> {
  pub algorithm: PowAlgorithm,
  pub version: u8,
  pub enforce: PowEnforcement,
  pub challenge_id: &'a str,
  pub host: &'a str,
  pub difficulty: u64,
  pub issued_at: i64,
  pub expires_at: i64,
}
