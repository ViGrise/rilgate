use crate::{
  HookSet,
  definition::{Argument, Presentation},
  template::{TemplateValue, Values},
};
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
pub struct BrowserValues<'a> {
  pub challenge_id: &'a str,
  pub host: &'a str,
  pub locale: &'a str,
  pub state: &'a str,
  pub callback: &'a str,
}
impl HookSet {
  pub fn public(&self, name: &str, browser: &BrowserValues<'_>) -> Result<Value> {
    let hook = self
      .hooks
      .get(name)
      .ok_or_else(|| anyhow!("Unknown challenge hook"))?;
    let mut values = hook.values();
    for (key, value) in [
      ("challenge_id", browser.challenge_id),
      ("host", browser.host),
      ("locale", browser.locale),
      ("state", browser.state),
      ("callback", browser.callback),
    ] {
      values
        .values
        .insert(key.into(), TemplateValue::String(value.into()));
    }
    Ok(match &hook.presentation {
      Presentation::Redirect { url, .. } => {
        json!({"name": name, "mode": "redirect", "url": url.render_url(&values)?.as_str()})
      }
      Presentation::Embed {
        widget: Some(widget),
        ..
      } => {
        json!({"name": name,"mode":"embed","widget": {"script":widget.script.render_url(&values)?.as_str(), "onload":widget.onload, "call":widget.call, "args":widget.args.iter().map(|arg| render_argument(arg,&values)).collect::<Vec<_>>(), "returns":widget.returns, "integrity":widget.integrity}})
      }
      Presentation::Embed {
        script: Some(script),
        ..
      } => {
        json!({"name":name,"mode":"embed","script":{"src":script.src.render_url(&values)?.as_str(),"integrity":script.integrity},"params":hook.params,"challenge_id":browser.challenge_id})
      }
      _ => return Err(anyhow!("Invalid challenge hook presentation")),
    })
  }
}
fn render_argument(arg: &Argument, values: &Values) -> Value {
  match arg {
    Argument::Template(t) => serde_json::to_value(t.render(values)).expect("Scalar template value"),
    Argument::Literal(v) => v.clone(),
    Argument::Array(args) => {
      Value::Array(args.iter().map(|a| render_argument(a, values)).collect())
    }
    Argument::Object(args) => Value::Object(
      args
        .iter()
        .map(|(k, a)| (k.clone(), render_argument(a, values)))
        .collect(),
    ),
  }
}
