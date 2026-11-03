use crate::{
  Environment, PowAlgorithm,
  schema::{Document, challenge_hooks as schema},
};
use anyhow::{Context as _, Result, ensure};
use rilgate_challenge_hooks::{
  HookSet, definition as resolved,
  template::{Context, Template, TemplateValue, Values},
};
use std::{collections::BTreeMap, time::Duration};

struct Compiler<'a> {
  params: &'a BTreeMap<String, TemplateValue>,
  secrets: &'a BTreeMap<String, rilgate_crypto::Secret>,
  development: bool,
}
impl Compiler<'_> {
  fn template(&self, value: &str, context: Context, path: &str) -> Result<Template> {
    let template = Template::parse(value, context).with_context(|| path.to_owned())?;
    template
      .validate_keys(self.params, self.secrets)
      .with_context(|| path.to_owned())?;
    Ok(template)
  }
  fn sample(&self) -> Values {
    let mut values = Values::default();
    for key in [
      "challenge_id",
      "host",
      "locale",
      "state",
      "callback",
      "client_ip",
      "user_agent",
      "result",
      "verification_id",
    ] {
      values
        .values
        .insert(key.into(), TemplateValue::String("example".into()));
    }
    for (key, value) in self.params {
      values.values.insert(format!("params.{key}"), value.clone());
    }
    for key in self.secrets.keys() {
      values.values.insert(
        format!("secrets.{key}"),
        TemplateValue::String("example".into()),
      );
    }
    values
  }
  fn url(&self, value: &str, context: Context, path: &str) -> Result<(Template, url::Url)> {
    let template = self.template(value, context, path)?;
    let parsed = template
      .render_url(&self.sample())
      .ok()
      .filter(|u| self.scheme(u) && u.host_str().is_some());
    ensure!(parsed.is_some(), "{path}: expected an absolute https URL");
    Ok((template, parsed.unwrap()))
  }
  fn scheme(&self, url: &url::Url) -> bool {
    url.scheme() == "https" || (self.development && url.scheme() == "http")
  }
  fn operand(&self, value: &serde_json::Value, path: &str) -> Result<resolved::Operand> {
    Ok(match value {
      serde_json::Value::String(s) => {
        resolved::Operand::Template(self.template(s, Context::Verify, path)?)
      }
      serde_json::Value::Number(n) => resolved::Operand::Literal(TemplateValue::Number(n.clone())),
      serde_json::Value::Bool(b) => resolved::Operand::Literal(TemplateValue::Bool(*b)),
      _ => anyhow::bail!("{path}: expected a string, number or boolean"),
    })
  }
  fn argument(&self, value: &serde_json::Value, path: &str) -> Result<resolved::Argument> {
    Ok(match value {
      serde_json::Value::String(s) => {
        resolved::Argument::Template(self.template(s, Context::Browser, path)?)
      }
      serde_json::Value::Array(v) => resolved::Argument::Array(
        v.iter()
          .map(|v| self.argument(v, path))
          .collect::<Result<_>>()?,
      ),
      serde_json::Value::Object(v) => resolved::Argument::Object(
        v.iter()
          .map(|(k, v)| Ok((k.clone(), self.argument(v, path)?)))
          .collect::<Result<_>>()?,
      ),
      v => resolved::Argument::Literal(v.clone()),
    })
  }
  fn csp_source(&self, value: &str, path: &str) -> Result<url::Url> {
    let source = url::Url::parse(value).ok().filter(|u| {
      self.scheme(u)
        && u.host_str().is_some_and(|host| {
          !host.contains('*') || (host.starts_with("*.") && !host[2..].contains('*'))
        })
        && u.query().is_none()
        && u.fragment().is_none()
        && u.username().is_empty()
        && u.password().is_none()
        && u.path().ends_with('/')
        && !value
          .chars()
          .any(|c| c.is_whitespace() || matches!(c, ';' | ','))
    });
    ensure!(
      source.is_some(),
      "{path}: expected an https source with an optional *. host prefix and a path that ends with /"
    );
    Ok(source.unwrap())
  }
  fn presentation(
    &self,
    presentation: &schema::Presentation,
    path: &str,
  ) -> Result<resolved::Presentation> {
    let path = format!("{path}.presentation");
    Ok(match presentation {
      schema::Presentation::Redirect {
        url,
        result_param,
        csp,
      } => {
        ensure!(csp.is_none(), "{path}.csp: only valid for mode: embed");
        ensure!(
          (1..=64).contains(&result_param.len())
            && result_param != "state"
            && result_param
              .bytes()
              .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c)),
          "{path}.result_param: expected 1 to 64 letters, digits, dots, hyphens or underscores, other than state"
        );
        resolved::Presentation::Redirect {
          url: self
            .url(url, Context::BrowserUrl, &format!("{path}.url"))?
            .0,
          result_param: result_param.clone(),
        }
      }
      schema::Presentation::Embed {
        widget,
        script,
        csp,
      } => {
        ensure!(
          widget.is_some() != script.is_some(),
          "{path}: set exactly one of widget or script"
        );
        let mut script_url = None;
        let widget = widget
          .as_ref()
          .map(|w| -> Result<_> {
            let p = format!("{path}.widget");
            let identifiers: Vec<_> = w.call.split('.').collect();
            ensure!(
              (1..=4).contains(&identifiers.len())
                && identifiers.iter().all(|s| {
                  let mut bytes = s.bytes();
                  bytes
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || b"_$".contains(&c))
                    && bytes.all(|c| c.is_ascii_alphanumeric() || b"_$".contains(&c))
                }),
              "{p}.call: expected 1 to 4 JavaScript identifiers joined by dots"
            );
            ensure!(
              w.onload.as_ref().is_none_or(|s| (1..=32).contains(&s.len())
                && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')),
              "{p}.onload: expected 1 to 32 letters, digits or underscores"
            );
            ensure!(
              w.args.len() <= 8 && w.args.iter().all(|v| depth(v) <= 4),
              "{p}.args: expected at most 8 entries and nesting depth 4"
            );
            let has_result = w.args.iter().any(has_result);
            ensure!(
              w.returns != schema::Returns::Callback || has_result,
              "{p}.args: returns: callback needs $result"
            );
            ensure!(
              w.returns != schema::Returns::Promise || !has_result,
              "{p}.args: returns: promise cannot use $result"
            );
            let args = w
              .args
              .iter()
              .map(|v| self.argument(v, &format!("{p}.args")))
              .collect::<Result<_>>()?;
            let (script, url) = self.url(&w.script, Context::Browser, &format!("{p}.script"))?;
            script_url = Some(url);
            Ok(resolved::Widget {
              script,
              onload: w.onload.clone(),
              call: w.call.clone(),
              args,
              returns: w.returns,
              integrity: w.integrity.clone(),
            })
          })
          .transpose()?;
        let script = script
          .as_ref()
          .map(|s| -> Result<_> {
            let (src, url) = self.url(&s.src, Context::Browser, &format!("{path}.script.src"))?;
            script_url = Some(url);
            Ok(resolved::ScriptAdapter {
              src,
              integrity: s.integrity.clone(),
            })
          })
          .transpose()?;
        let mut matches = false;
        for (directive, sources) in csp {
          let p = format!("{path}.csp.{directive}");
          ensure!(
            [
              "script-src",
              "style-src",
              "img-src",
              "font-src",
              "connect-src",
              "frame-src"
            ]
            .contains(&directive.as_str()),
            "{p}: unsupported directive"
          );
          ensure!(sources.len() <= 16, "{p}: expected at most 16 sources");
          for (i, source) in sources.iter().enumerate() {
            let source = self.csp_source(source, &format!("{p}[{i}]"))?;
            if directive == "script-src"
              && script_url
                .as_ref()
                .is_some_and(|url| source_matches(&source, url))
            {
              matches = true;
            }
          }
        }
        ensure!(
          matches,
          "{path}: the script URL must match a csp script-src source"
        );
        resolved::Presentation::Embed {
          widget,
          script,
          csp: csp.clone(),
        }
      }
    })
  }
}
impl Document {
  pub(super) fn resolve_challenge_hooks(&self) -> Result<HookSet> {
    let policy = rilgate_policy::PolicyDocument {
      signals: self.signals.clone(),
      policy: self.policy.clone(),
    };
    let hashwx_enabled = policy.challenges()?.iter().any(|(_, step)| {
      step.algorithm.unwrap_or(self.pow.default_algorithm) == PowAlgorithm::Hashwx
    });
    ensure!(
      self.challenge_hooks.len() <= 16,
      "challenge_hooks: at most 16 hooks are supported"
    );
    let mut hooks = BTreeMap::new();
    for (name, hook) in &self.challenge_hooks {
      let p = format!("challenge_hooks.{name}");
      ensure!(
        (1..=32).contains(&name.len())
          && name
            .bytes()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
          && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'),
        "{p}: expected a name of 1 to 32 lowercase letters, digits or hyphens that starts with a letter or digit"
      );
      for (group, keys) in [
        ("params", hook.params.keys().collect::<Vec<_>>()),
        ("secrets", hook.secrets.keys().collect()),
      ] {
        for key in keys {
          ensure!(
            (1..=32).contains(&key.len())
              && key
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'),
            "{p}.{group}.{key}: expected 1 to 32 lowercase letters, digits or underscores"
          );
        }
      }
      let grant_ttl = hook.grant_ttl.unwrap_or(self.security.auth.ttl).0;
      ensure!(
        grant_ttl.subsec_nanos() == 0
          && (Duration::from_secs(60)..=self.security.auth.ttl.0).contains(&grant_ttl),
        "{p}.grant_ttl: expected whole seconds from 1m to security.auth.ttl"
      );
      let params = hook
        .params
        .iter()
        .map(|(k, v)| Ok((k.clone(), serde_json::from_value(serde_json::to_value(v)?)?)))
        .collect::<Result<BTreeMap<_, TemplateValue>>>()?;
      let compiler = Compiler {
        params: &params,
        secrets: &hook.secrets,
        development: matches!(self.environment, Environment::Development),
      };
      let presentation = compiler.presentation(&hook.presentation, &p)?;
      let verify = &hook.verify;
      ensure!(
        (Duration::from_millis(100)..=Duration::from_secs(30)).contains(&verify.timeout.0),
        "{p}.verify.timeout: expected 100ms to 30s"
      );
      ensure!(
        (Duration::from_millis(100)..=verify.timeout.0).contains(&verify.attempt_timeout.0),
        "{p}.verify.attempt_timeout: expected 100ms to verify.timeout"
      );
      ensure!(
        (1..=5).contains(&verify.retry.attempts),
        "{p}.verify.retry.attempts: expected 1 to 5"
      );
      ensure!(
        (Duration::from_millis(10)..=Duration::from_secs(5)).contains(&verify.retry.backoff.0),
        "{p}.verify.retry.backoff: expected 10ms to 5s"
      );
      ensure!(
        verify.request.headers.len() <= 16,
        "{p}.verify.request.headers: expected at most 16 headers"
      );
      ensure!(
        verify.request.fields.len() <= 32,
        "{p}.verify.request.fields: expected at most 32 fields"
      );
      let url = compiler
        .url(
          &verify.request.url,
          Context::Verify,
          &format!("{p}.verify.request.url"),
        )?
        .0;
      let headers = verify
        .request
        .headers
        .iter()
        .map(|(name, value)| {
          let path = format!("{p}.verify.request.headers.{name}");
          ensure!(
            http::HeaderName::try_from(name.as_str()).is_ok(),
            "{path}: invalid header name"
          );
          Ok((
            name.clone(),
            compiler.template(value, Context::Verify, &path)?,
          ))
        })
        .collect::<Result<_>>()?;
      let fields = verify
        .request
        .fields
        .iter()
        .map(|(name, value)| {
          Ok((
            name.clone(),
            compiler.operand(value, &format!("{p}.verify.request.fields.{name}"))?,
          ))
        })
        .collect::<Result<_>>()?;
      ensure!(
        (1..=16).contains(&verify.accept.len()),
        "{p}.verify.accept: expected 1 to 16 conditions"
      );
      let accept = verify
        .accept
        .iter()
        .enumerate()
        .map(|(i, a)| {
          let path = format!("{p}.verify.accept[{i}]");
          ensure!(
            [
              a.equals.is_some(),
              a.one_of.is_some(),
              a.min.is_some(),
              a.max.is_some()
            ]
            .iter()
            .filter(|v| **v)
            .count()
              == 1,
            "{path}: set exactly one of equals, in, min or max"
          );
          ensure!(
            a.field.is_empty() || a.field.starts_with('/'),
            "{path}.field: expected a JSON Pointer"
          );
          let operator = if let Some(v) = &a.equals {
            resolved::Operator::Equals(compiler.operand(v, &path)?)
          } else if let Some(v) = &a.one_of {
            resolved::Operator::In(
              v.iter()
                .map(|v| compiler.operand(v, &path))
                .collect::<Result<_>>()?,
            )
          } else {
            let v = a.min.as_ref().or(a.max.as_ref()).unwrap();
            ensure!(
              v.is_number() || v.is_string(),
              "{path}: min and max need a number"
            );
            let operand = compiler.operand(v, &path)?;
            ensure!(
              v.is_number()
                || matches!(&operand,resolved::Operand::Template(t) if t.is_placeholder()),
              "{path}: min and max need a number"
            );
            if a.min.is_some() {
              resolved::Operator::Min(operand)
            } else {
              resolved::Operator::Max(operand)
            }
          };
          Ok(resolved::Accept {
            field: a.field.clone(),
            operator,
          })
        })
        .collect::<Result<_>>()?;
      let unavailable = &hook.unavailable;
      ensure!(
        (unavailable.action == schema::UnavailableAction::Fallback)
          == unavailable.fallback.is_some(),
        "{p}.unavailable.fallback: required only for action: fallback"
      );
      let fallback=unavailable.fallback.as_ref().map(|f| -> Result<_> {
        ensure!(f.algorithm==PowAlgorithm::Fast || (f.algorithm==PowAlgorithm::Hashwx && hashwx_enabled),"{p}.unavailable.fallback.algorithm: expected an enabled fast or hashwx algorithm");
        let valid=match f.algorithm { PowAlgorithm::Fast => (1..=64).contains(&f.difficulty), PowAlgorithm::Hashwx => (1..=rilgate_pow_hashwx::MAX_DIFFICULTY).contains(&f.difficulty), _=>false };
        ensure!(valid,"{p}.unavailable.fallback.difficulty: fast requires 1..64; hashwx requires 1..2^40; randomx requires at least 1000; refresh requires 1..{} seconds (pow.algorithms.refresh.max)",self.pow.algorithms.refresh.max);
        Ok(resolved::FallbackStep{algorithm:if f.algorithm==PowAlgorithm::Fast {"fast"} else {"hashwx"}.into(),difficulty:f.difficulty})
      }).transpose()?;
      ensure!(
        (1..=100).contains(&unavailable.after),
        "{p}.unavailable.after: expected 1 to 100"
      );
      ensure!(
        (Duration::from_secs(1)..=Duration::from_secs(3600)).contains(&unavailable.recheck.0),
        "{p}.unavailable.recheck: expected 1s to 1h"
      );
      hooks.insert(
        name.clone(),
        resolved::HookDefinition {
          params,
          secrets: hook.secrets.clone(),
          grant_ttl,
          presentation,
          verify: resolved::Verify {
            request: resolved::VerifyRequest {
              method: verify.request.method,
              url,
              encoding: verify.request.encoding,
              headers,
              fields,
            },
            accept,
            timeout: verify.timeout.0,
            attempt_timeout: verify.attempt_timeout.0,
            retry: resolved::Retry {
              attempts: verify.retry.attempts,
              backoff: verify.retry.backoff.0,
              on_timeout: verify.retry.on_timeout,
            },
          },
          unavailable: resolved::Unavailable {
            action: unavailable.action,
            fallback,
            grant: unavailable.grant,
            after: unavailable.after,
            recheck: unavailable.recheck.0,
          },
        },
      );
    }
    Ok(HookSet { hooks })
  }
}
fn source_matches(source: &url::Url, url: &url::Url) -> bool {
  let host = source.host_str().unwrap();
  let target = url.host_str().unwrap();
  source.scheme() == url.scheme()
    && source.port_or_known_default() == url.port_or_known_default()
    && (host == target
      || host
        .strip_prefix("*.")
        .is_some_and(|suffix| target.ends_with(&format!(".{suffix}"))))
    && if source.path().ends_with('/') {
      url.path().starts_with(source.path())
    } else {
      source.path() == url.path()
    }
}
fn has_result(value: &serde_json::Value) -> bool {
  match value {
    serde_json::Value::String(s) => s == "$result",
    serde_json::Value::Array(a) => a.iter().any(has_result),
    serde_json::Value::Object(o) => o.values().any(has_result),
    _ => false,
  }
}
fn depth(value: &serde_json::Value) -> usize {
  match value {
    serde_json::Value::Array(a) => 1 + a.iter().map(depth).max().unwrap_or(0),
    serde_json::Value::Object(o) => 1 + o.values().map(depth).max().unwrap_or(0),
    _ => 0,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  fn error(presentation: &str, unavailable: &str, policy: &str) -> String {
    let document: Document = serde_yaml_ng::from_str(&format!("environment: development\nserver:\n  upstream: {{url: 'http://127.0.0.1:3000'}}\nchallenge_hooks:\n  captcha:\n    presentation: {presentation}\n    verify:\n      request: {{url: 'https://example.com/verify'}}\n      accept: [{{field: /success, equals: true}}]\n    unavailable: {unavailable}\npolicy:\n  rules:\n    - name: protected\n      when: {{path: {{prefix: /}}}}\n      action: challenge\n      challenge: {policy}\n")).unwrap();
    format!("{:#}", document.resolve().unwrap_err())
  }
  #[test]
  fn browser_arguments_reject_secret_templates() {
    assert!(error("{mode: embed, widget: {script: 'https://example.com/widget.js', call: widget.render, args: ['$result', '{secrets.x}']}, csp: {script-src: ['https://example.com']}}","{}","{hook: captcha}").contains("challenge_hooks.captcha.presentation.widget.args: placeholder {secrets.x} is not allowed here"));
  }
  #[test]
  fn script_must_match_its_csp_sources() {
    assert!(error("{mode: embed, script: {src: 'https://example.com/widget.js'}, csp: {script-src: ['https://other.example.com']}}","{}","{hook: captcha}").contains("challenge_hooks.captcha.presentation: the script URL must match a csp script-src source"));
  }
  #[test]
  fn promise_widget_forbids_result_callback() {
    assert!(error("{mode: embed, widget: {script: 'https://example.com/widget.js', call: widget.render, args: ['$result'], returns: promise}, csp: {script-src: ['https://example.com']}}","{}","{hook: captcha}").contains("challenge_hooks.captcha.presentation.widget.args: returns: promise cannot use $result"));
  }
  #[test]
  fn deny_action_forbids_fallback() {
    assert!(
      error(
        "{mode: redirect, url: 'https://example.com/start'}",
        "{fallback: {algorithm: fast, difficulty: 1}}",
        "{hook: captcha}"
      )
      .contains("challenge_hooks.captcha.unavailable.fallback: required only for action: fallback")
    );
  }
  #[test]
  fn policy_must_reference_a_configured_hook() {
    assert!(
      error(
        "{mode: redirect, url: 'https://example.com/start'}",
        "{}",
        "{hook: missing}"
      )
      .contains("challenge: unknown challenge hook missing")
    );
  }
}
