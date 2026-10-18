use super::value::Time;
use rilgate_challenge::authorization::binding::AuthBinding;
use rilgate_crypto::{Secret, token::KeyRotation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Security {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub secret: Option<Secret>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub keys: Option<Keys>,
  pub auth: Auth,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Keys {
  pub challenge: Secret,
  pub auth: Secret,
  #[serde(default)]
  pub challenge_rotation: KeyRotation,
  #[serde(default)]
  pub auth_rotation: KeyRotation,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Auth {
  pub cookie: String,
  pub ttl: Time,
  pub bind: AuthBinding,
}
impl Default for Auth {
  fn default() -> Self {
    Self {
      cookie: "rilgate_auth".into(),
      ttl: Time::seconds(21_600),
      bind: AuthBinding::default(),
    }
  }
}
