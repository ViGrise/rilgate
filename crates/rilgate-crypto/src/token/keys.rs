use crate::Secret;
use anyhow::Result;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct KeyRotation {
  pub active_id: Option<String>,
  pub previous: BTreeMap<String, Secret>,
}

impl KeyRotation {
  pub fn validate(&self) -> Result<()> {
    anyhow::ensure!(
      self.previous.len() <= 8,
      "At most eight verification keys are supported"
    );
    for id in self.active_id.iter().chain(self.previous.keys()) {
      anyhow::ensure!(
        !id.is_empty()
          && id.len() <= 32
          && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "Key IDs must contain 1 to 32 ASCII letters, digits, underscores or hyphens"
      );
    }
    anyhow::ensure!(
      self.active_id.as_deref() != Some("legacy"),
      "legacy is reserved for unversioned verification keys"
    );
    if let Some(id) = &self.active_id {
      anyhow::ensure!(
        !self.previous.contains_key(id),
        "Active key ID cannot appear in verification keys"
      );
    }
    for key in self.previous.values() {
      anyhow::ensure!(
        !key.expose().is_empty(),
        "Verification keys cannot be empty"
      );
    }
    Ok(())
  }
}
