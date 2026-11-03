#![expect(
  clippy::result_large_err,
  reason = "Hook outage errors include the next challenge step."
)]

pub mod auth;
pub mod observability;
pub mod open_graph;
pub mod proof;
pub mod proxy;
pub mod runtime;
pub mod transport;
pub mod web;
pub use rilgate_config as config;
pub use runtime::app;

#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub mod fuzzing {
  pub fn policy_path(raw: &str) -> Result<String, &'static str> {
    crate::transport::path::policy_path(raw)
  }
  pub fn wants_json(
    accept: Option<&str>,
    content_type: Option<&str>,
    requested_with: Option<&str>,
    path: &str,
  ) -> bool {
    let mut headers = http::HeaderMap::new();
    for (name, value) in [
      ("accept", accept),
      ("content-type", content_type),
      ("x-requested-with", requested_with),
    ] {
      if let Some(value) = value.and_then(|value| http::HeaderValue::from_str(value).ok()) {
        headers.insert(name, value);
      }
    }
    let config = rilgate_web::config::ResponseConfig {
      json_paths: vec!["/api".into()],
      ..Default::default()
    };
    crate::web::negotiation::wants_json(&config, &headers, path)
  }
}
