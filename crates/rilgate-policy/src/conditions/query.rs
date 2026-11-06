use std::sync::OnceLock;

/// Decoded query parameters of one request. Parsing runs once, on the first condition that reads
/// a parameter, so requests pay nothing under policies without query conditions.
#[derive(Debug, Default)]
pub struct Query<'a> {
  raw: &'a str,
  parameters: OnceLock<Vec<(String, String)>>,
}

impl<'a> Query<'a> {
  /// Wraps the raw query string, without the leading `?`.
  pub fn new(raw: &'a str) -> Self {
    Self {
      raw,
      parameters: OnceLock::new(),
    }
  }

  /// Returns every value of the parameter, in request order.
  pub(super) fn values(&self, name: &str) -> impl Iterator<Item = &str> {
    self
      .parameters
      .get_or_init(|| {
        form_urlencoded::parse(self.raw.as_bytes())
          .into_owned()
          .collect()
      })
      .iter()
      .filter(move |(parameter, _)| parameter == name)
      .map(|(_, value)| value.as_str())
  }
}
