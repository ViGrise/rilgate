use super::{counters::CounterStore, honeypot::Honeypot};
use crate::open_graph::OpenGraphService;
use anyhow::Result;
use arc_swap::ArcSwap;
use rilgate_challenge::ChallengeService;
use rilgate_policy::{LookupPlan, policy::schema::Definition};
use std::sync::Arc;

/// Request-path services that depend on the policy definition.
pub struct PolicyServices {
  pub dns: Option<rilgate_dns::DnsLookups>,
  pub honeypot: Option<Arc<Honeypot>>,
  pub counters: std::collections::BTreeMap<String, Arc<CounterStore>>,
}

impl PolicyServices {
  pub fn new(definition: &Definition, plan: &LookupPlan) -> Result<Self> {
    Self::with_previous(definition, plan, None)
  }

  fn with_previous(
    definition: &Definition,
    plan: &LookupPlan,
    previous: Option<&Self>,
  ) -> Result<Self> {
    let dns = if plan.is_empty() {
      None
    } else {
      let settings = definition.dns.clone().unwrap_or_default();
      Some(rilgate_dns::DnsLookups::system(&settings)?)
    };
    let honeypot = definition
      .honeypot
      .as_ref()
      .map(|settings| Arc::new(Honeypot::new(settings)));
    let counters = definition
      .counters
      .iter()
      .flat_map(|counters| counters.iter())
      .map(|(name, settings)| {
        let store = previous
          .and_then(|services| services.counters.get(name))
          .filter(|counter| counter.settings == *settings)
          .cloned()
          .unwrap_or_else(|| Arc::new(CounterStore::new(settings)));
        (name.clone(), store)
      })
      .collect();
    Ok(Self {
      dns,
      honeypot,
      counters,
    })
  }

  /// Wraps the services in the swappable handle that `AppState` holds.
  pub fn into_shared(self) -> Arc<ArcSwap<Self>> {
    Arc::new(ArcSwap::from_pointee(self))
  }
}

/// Replaces the running policy together with everything compiled from it.
#[derive(Clone)]
pub struct PolicyHandle {
  pub(crate) pow: Arc<ChallengeService>,
  pub(crate) services: Arc<ArcSwap<PolicyServices>>,
  pub(crate) open_graph: Arc<OpenGraphService>,
}

impl PolicyHandle {
  /// Installs the definition. Every check runs first, so a failure leaves the running policy in place.
  pub fn replace(&self, definition: &Definition) -> Result<()> {
    let compiled = self.pow.compile(definition)?;
    let engine = compiled.engine();
    let previous = self.services.load_full();
    let services =
      PolicyServices::with_previous(definition, engine.lookup_plan(), Some(&previous))?;
    let condition = self
      .open_graph
      .compile_condition(engine.sets(), engine.geoip())?;
    self.open_graph.set_condition(condition);
    self.pow.install(compiled);
    self.services.store(Arc::new(services));
    Ok(())
  }

  pub fn challenge(&self) -> &Arc<ChallengeService> {
    &self.pow
  }

  /// Reports whether the open graph condition compares the host load.
  pub fn open_graph_uses_load(&self) -> bool {
    self.open_graph.uses_load()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use rilgate_policy::counters::{CounterKey, KeyPart};

  #[test]
  fn reload_preserves_equal_definitions_and_resets_changed_counters() {
    let mut definition: Definition =
      serde_json::from_value(serde_json::json!({"mode": "custom", "counters": {"pages": {"key": ["client_ip"], "window": "1m"}}}))
        .unwrap();
    let plan = LookupPlan::default();
    let key = || -> CounterKey { vec![KeyPart::Number(1)].into_boxed_slice() };
    let original = PolicyServices::new(&definition, &plan).unwrap();
    assert_eq!(original.counters["pages"].record(key()), 1);
    let reloaded = PolicyServices::with_previous(&definition, &plan, Some(&original)).unwrap();
    assert_eq!(reloaded.counters["pages"].record(key()), 2);
    definition
      .counters
      .as_mut()
      .unwrap()
      .get_mut("pages")
      .unwrap()
      .window *= 2;
    let changed = PolicyServices::with_previous(&definition, &plan, Some(&reloaded)).unwrap();
    assert_eq!(changed.counters["pages"].peek(key()), 0);
    assert_eq!(changed.counters["pages"].record(key()), 1);
    definition
      .counters
      .as_mut()
      .unwrap()
      .get_mut("pages")
      .unwrap()
      .count_when =
      Some(serde_json::from_value(serde_json::json!({"path": {"prefix": "/login"}})).unwrap());
    let changed = PolicyServices::with_previous(&definition, &plan, Some(&changed)).unwrap();
    assert_eq!(changed.counters["pages"].peek(key()), 0);
  }
}
