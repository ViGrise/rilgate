use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResponseConfig {
  pub format: ResponseFormat,
  pub json_paths: Vec<String>,
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
