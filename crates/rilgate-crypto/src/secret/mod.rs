use std::fmt;
#[derive(Clone, Default, serde::Deserialize)]
#[serde(transparent)]
pub struct Secret(pub String);

impl Secret {
  pub fn expose(&self) -> &str {
    &self.0
  }

  /// Returns 32 random bytes from the operating system, hex encoded.
  pub fn generate() -> anyhow::Result<Self> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes)
      .map_err(|error| anyhow::anyhow!("random source failed: {error}"))?;
    Ok(Self(hex::encode(bytes)))
  }
}

impl fmt::Debug for Secret {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("[redacted]")
  }
}

impl serde::Serialize for Secret {
  fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str("[redacted]")
  }
}
