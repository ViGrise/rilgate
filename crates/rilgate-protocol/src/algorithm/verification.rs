use super::PowAlgorithm;
use serde::{Deserialize, Serialize};

/// How Rilgate accepts a proof of work for one algorithm.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PowVerification {
  /// Rilgate checks the proof itself.
  #[default]
  Gateway,
  /// A Stratum pool must accept the share.
  PoolShare,
}

/// The verification mode of each algorithm. Only RandomX has a configurable mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VerificationModes {
  pub randomx: PowVerification,
}

impl VerificationModes {
  pub fn of(self, algorithm: PowAlgorithm) -> PowVerification {
    match algorithm {
      PowAlgorithm::RandomX => self.randomx,
      PowAlgorithm::Fast | PowAlgorithm::Hashwx | PowAlgorithm::Refresh => PowVerification::Gateway,
    }
  }
}

/// Signed tokens keep the original value names so tokens issued before the rename stay valid.
pub mod token_format {
  use super::PowVerification;
  use serde::{Deserialize, Deserializer, Serializer, de::Error};

  pub fn serialize<S: Serializer>(
    value: &PowVerification,
    serializer: S,
  ) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(match value {
      PowVerification::Gateway => "local_verify",
      PowVerification::PoolShare => "stratum_pool_accepted",
    })
  }

  pub fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
  ) -> Result<PowVerification, D::Error> {
    match String::deserialize(deserializer)?.as_str() {
      "local_verify" => Ok(PowVerification::Gateway),
      "stratum_pool_accepted" | "p2pool_accepted" => Ok(PowVerification::PoolShare),
      other => Err(D::Error::unknown_variant(
        other,
        &["local_verify", "stratum_pool_accepted", "p2pool_accepted"],
      )),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::PowVerification;
  use crate::{PowAlgorithm, claims::ReceiptClaims};

  #[test]
  fn receipt_tokens_preserve_the_signed_verification_format() {
    let claims = ReceiptClaims {
      host: "example.test".into(),
      v: 1,
      challenge_id: "challenge".into(),
      algorithm: PowAlgorithm::RandomX,
      difficulty: 1000,
      verification: PowVerification::PoolShare,
      issued_at: 1,
      expires_at: 2,
    };
    let json = serde_json::to_string(&claims).unwrap();
    assert!(json.contains("\"enforce\":\"stratum_pool_accepted\""));
    for (wire, mode) in [
      ("local_verify", PowVerification::Gateway),
      ("p2pool_accepted", PowVerification::PoolShare),
    ] {
      let decoded: ReceiptClaims =
        serde_json::from_str(&json.replace("stratum_pool_accepted", wire)).unwrap();
      assert_eq!(decoded.verification, mode);
    }
  }
}
