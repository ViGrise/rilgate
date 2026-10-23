use serde::{Deserialize, Serialize};
use std::str::FromStr;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PowEnforcement {
  LocalVerify,
  /// Decoding also accepts `p2pool_accepted`.
  #[serde(alias = "p2pool_accepted")]
  StratumPoolAccepted,
}

impl FromStr for PowEnforcement {
  type Err = String;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    match value {
      "local_verify" => Ok(Self::LocalVerify),
      "stratum_pool_accepted" => Ok(Self::StratumPoolAccepted),
      other => Err(format!(
        "Invalid PoW enforcement mode {other:?}; expected local_verify or stratum_pool_accepted"
      )),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::PowEnforcement;

  #[test]
  fn tokens_from_the_previous_name_decode_and_new_tokens_use_the_new_name() {
    let decoded: PowEnforcement = serde_json::from_str("\"p2pool_accepted\"").unwrap();
    assert_eq!(decoded, PowEnforcement::StratumPoolAccepted);
    assert_eq!(
      serde_json::to_string(&decoded).unwrap(),
      "\"stratum_pool_accepted\""
    );
  }
}
