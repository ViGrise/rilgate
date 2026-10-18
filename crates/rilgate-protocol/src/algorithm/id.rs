use serde::{Deserialize, Serialize};
use std::str::FromStr;
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PowAlgorithm {
  #[serde(rename = "fast")]
  Fast,

  #[serde(rename = "randomx")]
  RandomX,
}

impl FromStr for PowAlgorithm {
  type Err = String;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    match value {
      "fast" => Ok(Self::Fast),
      "randomx" => Ok(Self::RandomX),
      other => Err(format!(
        "Invalid PoW algorithm {other:?}; expected fast or randomx"
      )),
    }
  }
}
