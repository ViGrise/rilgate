use super::value::Time;
pub use rilgate_challenge_hooks::definition::{
  Encoding, FallbackGrant, Returns, UnavailableAction, VerifyMethod,
};
use rilgate_protocol::PowAlgorithm;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
  String(String),
  Integer(i64),
  Float(f64),
  Bool(bool),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookDefinition {
  #[serde(default)]
  pub params: BTreeMap<String, ParamValue>,
  #[serde(default)]
  pub secrets: BTreeMap<String, rilgate_crypto::Secret>,
  #[serde(default)]
  pub grant_ttl: Option<Time>,
  pub presentation: Presentation,
  pub verify: Verify,
  #[serde(default)]
  pub unavailable: Unavailable,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Presentation {
  Redirect {
    url: String,
    #[serde(default = "result")]
    result_param: String,
    #[serde(
      default,
      deserialize_with = "redirect_csp",
      skip_serializing_if = "Option::is_none"
    )]
    csp: Option<Value>,
  },
  Embed {
    #[serde(default)]
    widget: Option<Widget>,
    #[serde(default)]
    script: Option<ScriptAdapter>,
    #[serde(default)]
    csp: BTreeMap<String, Vec<String>>,
  },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Widget {
  pub script: String,
  #[serde(default)]
  pub onload: Option<String>,
  pub call: String,
  pub args: Vec<Value>,
  #[serde(default)]
  pub returns: Returns,
  #[serde(default)]
  pub integrity: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptAdapter {
  pub src: String,
  #[serde(default)]
  pub integrity: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verify {
  pub request: VerifyRequest,
  pub accept: Vec<Accept>,
  #[serde(default = "timeout")]
  pub timeout: Time,
  #[serde(default = "attempt_timeout")]
  pub attempt_timeout: Time,
  #[serde(default)]
  pub retry: Retry,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyRequest {
  #[serde(default)]
  pub method: VerifyMethod,
  pub url: String,
  #[serde(default)]
  pub encoding: Encoding,
  #[serde(default)]
  pub headers: BTreeMap<String, String>,
  #[serde(default)]
  pub fields: BTreeMap<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Retry {
  pub attempts: u32,
  pub backoff: Time,
  pub on_timeout: bool,
}
impl Default for Retry {
  fn default() -> Self {
    Self {
      attempts: 3,
      backoff: Time(Duration::from_millis(200)),
      on_timeout: false,
    }
  }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accept {
  pub field: String,
  pub equals: Option<Value>,
  #[serde(rename = "in")]
  pub one_of: Option<Vec<Value>>,
  pub min: Option<Value>,
  pub max: Option<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Unavailable {
  pub action: UnavailableAction,
  pub fallback: Option<FallbackStep>,
  pub grant: FallbackGrant,
  pub after: u32,
  pub recheck: Time,
}
impl Default for Unavailable {
  fn default() -> Self {
    Self {
      action: UnavailableAction::Deny,
      fallback: None,
      grant: FallbackGrant::WhileUnavailable,
      after: 3,
      recheck: Time::seconds(30),
    }
  }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackStep {
  pub algorithm: PowAlgorithm,
  pub difficulty: u64,
}
fn result() -> String {
  "result".into()
}
fn timeout() -> Time {
  Time::seconds(5)
}
fn attempt_timeout() -> Time {
  Time::seconds(2)
}

fn redirect_csp<'de, D: serde::Deserializer<'de>>(
  deserializer: D,
) -> Result<Option<Value>, D::Error> {
  Value::deserialize(deserializer).map(Some)
}
