use http::StatusCode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResponseConfig {
  pub format: ResponseFormat,
  pub json_paths: Vec<String>,
  #[serde(with = "status_code")]
  pub challenge_status: StatusCode,
  #[serde(with = "status_code")]
  pub deny_status: StatusCode,
}

impl Default for ResponseConfig {
  fn default() -> Self {
    Self {
      format: ResponseFormat::default(),
      json_paths: Vec::new(),
      challenge_status: StatusCode::OK,
      deny_status: StatusCode::FORBIDDEN,
    }
  }
}

mod status_code {
  use super::*;

  pub fn serialize<S: serde::Serializer>(
    status: &StatusCode,
    serializer: S,
  ) -> Result<S::Ok, S::Error> {
    serializer.serialize_u16(status.as_u16())
  }

  pub fn deserialize<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
  ) -> Result<StatusCode, D::Error> {
    StatusCode::from_u16(u16::deserialize(deserializer)?).map_err(serde::de::Error::custom)
  }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseFormat {
  #[default]
  Auto,
  Html,
  Json,
}

impl ResponseConfig {
  pub fn validate(&self) -> anyhow::Result<()> {
    anyhow::ensure!(
      [200, 401, 403, 429, 503].contains(&self.challenge_status.as_u16()),
      "site.responses.challenge_status: expected one of 200, 401, 403, 429, 503"
    );
    anyhow::ensure!(
      [403, 404, 410, 429].contains(&self.deny_status.as_u16()),
      "site.responses.deny_status: expected one of 403, 404, 410, 429"
    );
    anyhow::ensure!(
      self.json_paths.len() <= 64,
      "site.responses.json_paths: at most 64 path prefixes are supported"
    );
    for (index, path) in self.json_paths.iter().enumerate() {
      anyhow::ensure!(
        path.starts_with('/')
          && path.len() <= 2048
          && !path
            .chars()
            .any(|c| c.is_control() || matches!(c, '?' | '#' | '\\' | '%'))
          && !path.split('/').any(|part| matches!(part, "." | "..")),
        "site.responses.json_paths[{index}]: expected an absolute path prefix without escapes, query, fragment or dot segments"
      );
    }
    anyhow::ensure!(
      !matches!(self.format, ResponseFormat::Html) || self.json_paths.is_empty(),
      "site.responses.json_paths: incompatible with format: html"
    );
    Ok(())
  }
}
