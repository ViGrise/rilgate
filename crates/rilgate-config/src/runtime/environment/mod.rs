use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
  Development,
  #[default]
  Production,
}

impl FromStr for Environment {
  type Err = String;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    match value {
      "development" => Ok(Self::Development),
      "production" => Ok(Self::Production),
      other => Err(format!(
        "Invalid environment {other:?}; expected development or production"
      )),
    }
  }
}

impl std::fmt::Display for Environment {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.write_str(match self {
      Self::Development => "development",
      Self::Production => "production",
    })
  }
}
