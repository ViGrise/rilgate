use std::fmt;
#[derive(Clone, Default, serde::Deserialize)]
#[serde(transparent)]
pub struct Secret(pub String);

impl Secret {
  pub fn expose(&self) -> &str {
    &self.0
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
