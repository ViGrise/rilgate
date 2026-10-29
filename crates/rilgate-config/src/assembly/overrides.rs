use std::path::PathBuf;

/// Values from the command line and process environment that take precedence
/// over the YAML document.
#[derive(Debug, Default, Clone)]
pub struct Overrides {
  pub config: Option<PathBuf>,
  /// Replaces the port of every listen address.
  pub port: Option<u16>,
  pub settings: Vec<Setting>,
}

/// A value assigned to one YAML path, such as `server.upstream.url`.
///
/// Values are typed by the schema exactly like `${VARIABLE}` references, so a
/// duration accepts `5s` and a list replaces the whole sequence.
#[derive(Debug, Clone)]
pub struct Setting {
  pub path: String,
  /// The flag or variable that supplied the value, for error messages.
  pub source: String,
  pub value: SettingValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingValue {
  Scalar(String),
  List(Vec<String>),
}
