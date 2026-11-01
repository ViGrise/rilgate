use anyhow::{Result, bail, ensure};
use std::collections::BTreeSet;

/// Where a condition is compiled. Some conditions need data that only policy rules have.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Scope {
  #[default]
  Rule,
  CountWhen,
  Request,
}

#[derive(Default)]
pub(crate) struct Budget {
  nodes: usize,
  pub(super) bytes: usize,
  pub(super) honeypot: bool,
  pub(super) counters: Vec<String>,
  pub(crate) scope: Scope,
  pub(crate) dnsbl_zones: BTreeSet<String>,
  pub(crate) reverse_dns: bool,
}
impl Budget {
  pub(crate) fn for_policy(honeypot: bool, counters: Vec<String>) -> Self {
    Self {
      honeypot,
      counters,
      ..Default::default()
    }
  }

  pub(super) fn for_request() -> Self {
    Self {
      scope: Scope::Request,
      ..Default::default()
    }
  }

  pub(super) fn consume(&mut self, depth: usize, path: &str) -> Result<()> {
    self.nodes += 1;
    ensure!(depth <= 16, "{path}: condition depth must not exceed 16");
    ensure!(
      self.nodes <= 512,
      "{path}: policy must not exceed 512 condition nodes"
    );
    Ok(())
  }

  /// Rejects a condition outside policy rules. `in_count_when` allows it inside `count_when`.
  pub(crate) fn require_rule(&self, label: &str, name: &str, in_count_when: bool) -> Result<()> {
    match self.scope {
      Scope::Rule => {}
      Scope::CountWhen => ensure!(in_count_when, "{label}: unavailable in count_when"),
      Scope::Request => bail!("{label}: {name} is only available in policy rules"),
    }
    Ok(())
  }
}
