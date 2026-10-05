use serde::{Deserialize, Serialize};
use std::str::FromStr;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PowEnforcement {
  LocalVerify,
  P2poolAccepted,
}

impl FromStr for PowEnforcement {
  type Err = String;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    match value {
      "local_verify" => Ok(Self::LocalVerify),
      "p2pool_accepted" => Ok(Self::P2poolAccepted),
      other => Err(format!(
        "Invalid PoW enforcement mode {other:?}; expected local_verify or p2pool_accepted"
      )),
    }
  }
}
