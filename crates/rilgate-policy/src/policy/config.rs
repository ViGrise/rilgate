use rilgate_protocol::PowAlgorithm;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ChallengePolicy {
  #[serde(
    default,
    deserialize_with = "super::schema::present",
    skip_serializing_if = "Option::is_none"
  )]
  pub algorithm: Option<PowAlgorithm>,
  #[serde(
    default,
    deserialize_with = "super::schema::present",
    skip_serializing_if = "Option::is_none"
  )]
  pub difficulty: Option<u64>,
}
