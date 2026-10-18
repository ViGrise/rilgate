use super::value::{Size, Time};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Server {
  pub listen: String,
  pub upstream: Upstream,
  pub max_connections: usize,
  pub max_requests: usize,
  pub h2_max_streams: u32,
  pub trusted_proxies: Vec<String>,
  pub metrics_enabled: bool,
  pub readiness_timeout: Time,
}
impl Default for Server {
  fn default() -> Self {
    Self {
      listen: ":8080".into(),
      upstream: Upstream::default(),
      max_connections: 1024,
      max_requests: 512,
      h2_max_streams: 64,
      trusted_proxies: vec![],
      metrics_enabled: false,
      readiness_timeout: Time::seconds(2),
    }
  }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Upstream {
  pub url: String,
  pub preserve_host: bool,
  pub max_body_size: Size,
  pub timeout: Timeout,
}
impl Default for Upstream {
  fn default() -> Self {
    Self {
      url: String::new(),
      preserve_host: true,
      max_body_size: Size(16 * 1024 * 1024),
      timeout: Timeout::default(),
    }
  }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Timeout {
  pub connect: Time,
  pub read: Time,
}
impl Default for Timeout {
  fn default() -> Self {
    Self {
      connect: Time::seconds(5),
      read: Time::seconds(30),
    }
  }
}
