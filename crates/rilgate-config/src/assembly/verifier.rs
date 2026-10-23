use crate::{
  Environment, Finding, Severity,
  schema::{Document, pow::Serve},
};
use anyhow::{Context, Result, ensure};
use std::{net::SocketAddr, time::Duration};

#[derive(Debug, Clone)]
pub struct VerifierConfig {
  pub environment: Environment,
  pub serve: Serve,
  pub listen: SocketAddr,
  pub operations: Vec<SocketAddr>,
  pub metrics_enabled: bool,
  pub idle_timeout: Duration,
}
impl VerifierConfig {
  pub fn warnings(&self) -> Vec<Finding> {
    let mut findings = Vec::new();
    if matches!(self.environment, Environment::Production) {
      for (id, path, addresses) in [
        (
          "verifier-serve-public",
          "pow.algorithms.randomx.serve.listen",
          std::slice::from_ref(&self.listen),
        ),
        (
          "operations-listen-public",
          "server.operations.listen",
          self.operations.as_slice(),
        ),
      ] {
        for address in super::audit::public(addresses) {
          findings.push(Finding {
            id,
            severity: Severity::Warning,
            message: format!("{path}: {address} is outside loopback and private networks"),
          });
        }
      }
    }
    if self.metrics_enabled && self.operations.is_empty() {
      findings.push(Finding {
        id: "metrics-unreachable",
        severity: Severity::Warning,
        message: "server.metrics_enabled: server.operations.listen is empty".into(),
      });
    }
    findings
  }
}
impl Document {
  pub fn resolve_verifier(&self) -> Result<VerifierConfig> {
    let randomx = &self.pow.algorithms.randomx;
    let serve = randomx
      .serve
      .clone()
      .context("pow.algorithms.randomx.serve: required")?;
    ensure!(
      serve.workers == 1,
      "pow.algorithms.randomx.serve.workers: must be 1"
    );
    ensure!(
      (1..=4096).contains(&serve.queue),
      "pow.algorithms.randomx.serve.queue: expected 1 to 4096"
    );
    ensure!(
      (1..=65536).contains(&serve.max_connections),
      "pow.algorithms.randomx.serve.max_connections: expected 1 to 65536"
    );
    ensure!(
      serve.tls.cert.is_some() && serve.tls.key.is_some(),
      "pow.algorithms.randomx.serve.tls: cert and key are required"
    );
    ensure!(
      !matches!(self.environment, Environment::Production) || serve.tls.client_ca.is_some(),
      "pow.algorithms.randomx.serve.tls.client_ca: production requires client authentication [verifier-serve-unauthenticated]"
    );
    ensure!(
      randomx.cache.idle_timeout.0 <= Duration::from_secs(604800),
      "pow.algorithms.randomx.cache.idle_timeout: expected at most 7d"
    );
    let listen = super::listen_address(&serve.listen, "pow.algorithms.randomx.serve.listen")?;
    let operations = super::listen_addresses(
      &self.server.operations.listen,
      "server.operations.listen",
      0,
    )?;
    let gateway = super::parse_listen(&self.server.listen, "server.listen")?;
    ensure!(
      !operations.contains(&listen) && !gateway.contains(&listen),
      "pow.algorithms.randomx.serve.listen: overlaps server.listen or server.operations.listen"
    );
    Ok(VerifierConfig {
      environment: self.environment,
      serve,
      listen,
      operations,
      metrics_enabled: self.server.metrics_enabled,
      idle_timeout: randomx.cache.idle_timeout.0,
    })
  }
  pub fn display_verifier(&self, defaults: bool) -> Result<String> {
    let mut subset = Document {
      environment: self.environment,
      ..Default::default()
    };
    subset.pow.algorithms.randomx.cache = self.pow.algorithms.randomx.cache.clone();
    subset.pow.algorithms.randomx.serve = self.pow.algorithms.randomx.serve.clone();
    subset.server.operations = self.server.operations.clone();
    subset.server.metrics_enabled = self.server.metrics_enabled;
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&subset.display(defaults)?)?;
    let mut result = serde_yaml_ng::Mapping::new();
    for name in ["environment", "pow", "server"] {
      if let Some(value) = value.get(name) {
        result.insert(name.into(), value.clone());
      }
    }
    // Only verifier settings belong in the displayed command configuration.
    if defaults {
      let full = serde_yaml_ng::to_value(&subset)?;
      let mut rx = serde_yaml_ng::Mapping::new();
      for key in ["cache", "serve"] {
        rx.insert(
          key.into(),
          full["pow"]["algorithms"]["randomx"][key].clone(),
        );
      }
      let mut algorithms = serde_yaml_ng::Mapping::new();
      algorithms.insert("randomx".into(), rx.into());
      let mut pow = serde_yaml_ng::Mapping::new();
      pow.insert("algorithms".into(), algorithms.into());
      result.insert("pow".into(), pow.into());
      let mut server = serde_yaml_ng::Mapping::new();
      for key in ["operations", "metrics_enabled"] {
        server.insert(key.into(), full["server"][key].clone());
      }
      result.insert("server".into(), server.into());
    }
    Ok(serde_yaml_ng::to_string(&result)?)
  }
}
