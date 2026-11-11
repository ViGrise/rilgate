use serde::{Deserialize, Serialize};

use crate::{PowAlgorithm, PowVerification};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresentationClaims {
  pub v: u8,
  pub host: String,
  pub path: String,
  #[serde(default, skip_serializing)]
  pub rule: String,
  #[serde(default, skip_serializing)]
  pub weight: i32,
  pub algorithm: PowAlgorithm,
  pub difficulty: u64,
  #[serde(
    rename = "enforce",
    with = "crate::algorithm::verification::token_format"
  )]
  pub verification: PowVerification,
  pub issued_at: i64,
  pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresentationClaimsV2 {
  pub v: u8,
  pub host: String,
  pub path: String,
  #[serde(default, skip_serializing)]
  pub rule: String,
  #[serde(default, skip_serializing)]
  pub weight: i32,
  pub challenge_id: String,
  pub steps: Vec<crate::ChallengeStep>,
  pub step: u8,
  pub issued_at: i64,
  pub expires_at: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnyPresentationClaims {
  V2(PresentationClaimsV2),
  V1(PresentationClaims),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "algorithm", rename_all = "snake_case")]
pub enum ChallengeClaims {
  Fast {
    v: u8,
    #[serde(
      rename = "enforce",
      with = "crate::algorithm::verification::token_format"
    )]
    verification: PowVerification,
    challenge_id: String,
    host: String,
    challenge: String,
    difficulty: u64,
    issued_at: i64,
    expires_at: i64,
  },

  Hashwx {
    v: u8,
    #[serde(
      rename = "enforce",
      with = "crate::algorithm::verification::token_format"
    )]
    verification: PowVerification,
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
    #[serde(
      rename = "enforce",
      with = "crate::algorithm::verification::token_format"
    )]
    verification: PowVerification,
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
  #[serde(
    rename = "enforce",
    with = "crate::algorithm::verification::token_format"
  )]
  pub verification: PowVerification,
  pub issued_at: i64,
  pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthClaims {
  pub host: String,
  pub v: u8,
  pub algorithm: PowAlgorithm,
  pub difficulty: u64,
  #[serde(
    rename = "enforce",
    with = "crate::algorithm::verification::token_format"
  )]
  pub verification: PowVerification,
  pub issued_at: i64,
  pub expires_at: i64,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub binding: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub networks: Option<Vec<NetworkBinding>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthClaimsV2 {
  pub host: String,
  pub v: u8,
  pub grants: Vec<crate::Grant>,
  pub issued_at: i64,
  pub expires_at: i64,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub binding: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub networks: Option<Vec<NetworkBinding>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnyAuthClaims {
  V2(AuthClaimsV2),
  V1(AuthClaims),
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
        verification,
        challenge_id,
        host,
        difficulty,
        issued_at,
        expires_at,
        ..
      } => ChallengeMetadata {
        algorithm: PowAlgorithm::Fast,
        version: *v,
        verification: *verification,
        challenge_id,
        host,
        difficulty: *difficulty,
        issued_at: *issued_at,
        expires_at: *expires_at,
      },
      Self::Hashwx {
        v,
        verification,
        challenge_id,
        host,
        difficulty,
        issued_at,
        expires_at,
        ..
      } => ChallengeMetadata {
        algorithm: PowAlgorithm::Hashwx,
        version: *v,
        verification: *verification,
        challenge_id,
        host,
        difficulty: *difficulty,
        issued_at: *issued_at,
        expires_at: *expires_at,
      },
      Self::Randomx {
        v,
        verification,
        challenge_id,
        host,
        difficulty,
        issued_at,
        expires_at,
        ..
      } => ChallengeMetadata {
        algorithm: PowAlgorithm::RandomX,
        version: *v,
        verification: *verification,
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
  pub verification: PowVerification,
  pub challenge_id: &'a str,
  pub host: &'a str,
  pub difficulty: u64,
  pub issued_at: i64,
  pub expires_at: i64,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn presentation_rollout_reads_legacy_metadata_and_omits_it_on_issue() {
    for payload in [
      serde_json::json!({"v":1,"host":"example.test","path":"/admin","rule":"internal-admin-rule","weight":42,"algorithm":"fast","difficulty":1,"enforce":"local_verify","issued_at":1,"expires_at":60}),
      serde_json::json!({"v":2,"host":"example.test","path":"/admin","rule":"internal-admin-rule","weight":42,"challenge_id":"challenge","steps":[{"pow":{"algorithm":"fast","difficulty":1,"enforce":"local_verify"}}],"step":0,"issued_at":1,"expires_at":60}),
    ] {
      let claims: AnyPresentationClaims = serde_json::from_value(payload).unwrap();
      let issued = serde_json::to_value(claims).unwrap();
      assert!(issued.get("rule").is_none());
      assert!(issued.get("weight").is_none());
      let _: AnyPresentationClaims = serde_json::from_value(issued).unwrap();
    }
  }
}
