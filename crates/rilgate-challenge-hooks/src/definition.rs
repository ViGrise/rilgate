use crate::template::{Template, TemplateValue, Values};
use rilgate_crypto::Secret;
use std::{collections::BTreeMap, time::Duration};
#[derive(Debug, Default)]
pub struct HookSet {
  pub hooks: BTreeMap<String, HookDefinition>,
}
#[derive(Debug)]
pub struct HookDefinition {
  pub params: BTreeMap<String, TemplateValue>,
  pub secrets: BTreeMap<String, Secret>,
  pub grant_ttl: Duration,
  pub presentation: Presentation,
  pub verify: Verify,
  pub unavailable: Unavailable,
}
impl HookDefinition {
  pub fn values(&self) -> Values {
    let mut values = Values::default();
    for (key, value) in &self.params {
      values.values.insert(format!("params.{key}"), value.clone());
    }
    for (key, value) in &self.secrets {
      values.values.insert(
        format!("secrets.{key}"),
        TemplateValue::String(value.expose().into()),
      );
    }
    values
  }
}
#[derive(Debug)]
pub enum Presentation {
  Redirect {
    url: Template,
    result_param: String,
  },
  Embed {
    widget: Option<Widget>,
    script: Option<ScriptAdapter>,
    csp: BTreeMap<String, Vec<String>>,
  },
}
#[derive(Debug)]
pub enum Argument {
  Template(Template),
  Array(Vec<Argument>),
  Object(BTreeMap<String, Argument>),
  Literal(serde_json::Value),
}
#[derive(Debug)]
pub struct Widget {
  pub script: Template,
  pub onload: Option<String>,
  pub call: String,
  pub args: Vec<Argument>,
  pub returns: Returns,
  pub integrity: Option<String>,
}
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Returns {
  #[default]
  Callback,
  Promise,
}
#[derive(Debug)]
pub struct ScriptAdapter {
  pub src: Template,
  pub integrity: Option<String>,
}
#[derive(Debug)]
pub struct Verify {
  pub request: VerifyRequest,
  pub accept: Vec<Accept>,
  pub timeout: Duration,
  pub attempt_timeout: Duration,
  pub retry: Retry,
}
#[derive(Debug)]
pub struct VerifyRequest {
  pub method: VerifyMethod,
  pub url: Template,
  pub encoding: Encoding,
  pub headers: BTreeMap<String, Template>,
  pub fields: BTreeMap<String, Operand>,
}
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyMethod {
  #[default]
  Post,
  Get,
}
impl<'de> serde::Deserialize<'de> for VerifyMethod {
  fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let method = <String as serde::Deserialize>::deserialize(deserializer)?;
    if method.eq_ignore_ascii_case("post") {
      Ok(Self::Post)
    } else if method.eq_ignore_ascii_case("get") {
      Ok(Self::Get)
    } else {
      Err(serde::de::Error::custom("expected post or get"))
    }
  }
}
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
  #[default]
  Form,
  Json,
}
#[derive(Debug)]
pub struct Retry {
  pub attempts: u32,
  pub backoff: Duration,
  pub on_timeout: bool,
}
#[derive(Debug)]
pub struct Accept {
  pub field: String,
  pub operator: Operator,
}
#[derive(Debug)]
pub enum Operator {
  Equals(Operand),
  In(Vec<Operand>),
  Min(Operand),
  Max(Operand),
}
#[derive(Debug)]
pub enum Operand {
  Template(Template),
  Literal(TemplateValue),
}
impl Operand {
  pub fn render(&self, values: &Values) -> TemplateValue {
    match self {
      Self::Template(t) => t.render(values),
      Self::Literal(v) => v.clone(),
    }
  }
}
#[derive(Debug)]
pub struct Unavailable {
  pub action: UnavailableAction,
  pub fallback: Option<FallbackStep>,
  pub grant: FallbackGrant,
  pub after: u32,
  pub recheck: Duration,
}
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableAction {
  #[default]
  Deny,
  Fallback,
}
#[derive(Debug)]
pub struct FallbackStep {
  pub algorithm: String,
  pub difficulty: u64,
}
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FallbackGrant {
  #[default]
  WhileUnavailable,
  UntilExpiry,
}

impl HookSet {
  /// Returns the CSP sources of the named embed hooks, merged by directive.
  pub fn csp<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> BTreeMap<String, Vec<String>> {
    let mut merged: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in names {
      if let Some(Presentation::Embed { csp, .. }) =
        self.hooks.get(name).map(|hook| &hook.presentation)
      {
        for (directive, sources) in csp {
          let entry = merged.entry(directive.clone()).or_default();
          for source in sources {
            if !entry.contains(source) {
              entry.push(source.clone());
            }
          }
        }
      }
    }
    merged
  }
}
