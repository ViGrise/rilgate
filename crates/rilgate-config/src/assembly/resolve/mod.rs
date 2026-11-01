mod keys;
mod pow;
mod server;
mod web;

use crate::{Config, Configuration, schema::Document};
use anyhow::{Result, ensure};
use std::sync::Arc;

impl Document {
  pub fn resolve(&self) -> Result<Config> {
    self
      .config_sources
      .result(self.resolve_config(), &self.sources)
  }

  fn resolve_config(&self) -> Result<Config> {
    ensure!(
      self.pow.algorithms.randomx.serve.is_none(),
      "pow.algorithms.randomx.serve: use rilgate verifier serve; combined mode is not supported"
    );
    let (listen_addresses, operations_listen_addresses) = self.resolve_listen_addresses()?;
    ensure!(
      !self.server.upstream.url.is_empty(),
      "server.upstream.url: required; expected an HTTP or HTTPS upstream URL"
    );
    let keys = self.resolve_keys()?;
    let secret_generated = keys.secret_generated;
    let policy_document = rilgate_policy::PolicyDocument {
      signals: self.signals.clone(),
      policy: self.policy.clone(),
    };
    let needs_pool = self.requires_stratum_pool(&policy_document)?;
    let stratum_pool = self.resolve_stratum_pool(&policy_document, needs_pool)?;
    let server = self.resolve_server()?;
    let upstream = self.resolve_upstream()?;
    let open_graph = self.site.open_graph.resolve()?;
    let web = self.resolve_web();
    let pow = self.resolve_pow(keys, policy_document, stratum_pool, needs_pool)?;
    let config = Arc::new(Configuration {
      env: self.environment,
      app_port: listen_addresses[0].port(),
      listen_addresses,
      operations_listen_addresses,
      secret_generated,
      server,
      upstream,
      open_graph,
      web,
      pow,
    });
    config.validate()?;
    Ok(config)
  }

  pub fn display(&self, defaults: bool) -> Result<String> {
    let mut value = serde_yaml_ng::to_value(self)?;
    if !defaults {
      let defaults = Self::default();
      let mut baseline = serde_yaml_ng::to_value(&defaults)?;
      if self.site.open_graph.request_headers != defaults.site.open_graph.request_headers {
        // Header maps replace their defaults as a unit during deserialization.
        baseline["site"]["open_graph"]["request_headers"] = serde_yaml_ng::Value::Null;
      }
      prune(&mut value, &baseline);
    }
    Ok(serde_yaml_ng::to_string(&value)?)
  }
}
fn prune(value: &mut serde_yaml_ng::Value, default: &serde_yaml_ng::Value) {
  if let (Some(map), Some(default)) = (value.as_mapping_mut(), default.as_mapping()) {
    map.retain(|key, value| {
      if let Some(baseline) = default.get(key) {
        if value == baseline {
          return false;
        }
        prune(value, baseline);
      }
      true
    });
  }
}
