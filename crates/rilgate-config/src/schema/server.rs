use super::value::{Size, Time};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Server {
  pub listen: Listen,
  pub operations: Operations,
  pub upstream: Upstream,
  pub max_connections: usize,
  pub max_requests: usize,
  pub h2_max_streams: u32,
  pub trusted_proxies: Vec<String>,
  #[serde(
    default,
    deserialize_with = "present",
    skip_serializing_if = "Option::is_none"
  )]
  pub client_ip_header: Option<String>,
  pub metrics_enabled: bool,
  pub readiness_timeout: Time,
}
impl Default for Server {
  fn default() -> Self {
    Self {
      listen: Listen::single(":8080"),
      operations: Operations::default(),
      upstream: Upstream::default(),
      max_connections: 1024,
      max_requests: 512,
      h2_max_streams: 64,
      trusted_proxies: vec![],
      client_ip_header: None,
      metrics_enabled: false,
      readiness_timeout: Time::seconds(2),
    }
  }
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Operations {
  pub listen: Listen,
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

/// One listen address or a list of them. A single address serializes as a plain string.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listen(pub Vec<String>);
impl Listen {
  pub fn single(address: impl Into<String>) -> Self {
    Self(vec![address.into()])
  }
}
impl Serialize for Listen {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    match self.0.as_slice() {
      [only] => only.serialize(serializer),
      many => many.serialize(serializer),
    }
  }
}
impl<'de> Deserialize<'de> for Listen {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Form {
      One(String),
      Many(Vec<String>),
    }
    Ok(match Form::deserialize(deserializer)? {
      Form::One(address) => Self(vec![address]),
      Form::Many(addresses) => Self(addresses),
    })
  }
}

fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
  deserializer: D,
) -> Result<Option<T>, D::Error> {
  T::deserialize(deserializer).map(Some)
}
