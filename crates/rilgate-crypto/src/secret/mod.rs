use std::fmt;
#[derive(Clone, Default)]
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
