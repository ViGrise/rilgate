use super::{honeypot::Honeypot, request_rate::RequestRate};
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
  pub request_rate: Option<Arc<RequestRate>>,
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
    let request_rate = definition.request_rate.as_ref().map(|settings| {
      previous
        .and_then(|services| services.request_rate.as_ref())
        .filter(|counter| counter.settings == *settings)
        .cloned()
        .unwrap_or_else(|| Arc::new(RequestRate::new(settings)))
    });
    Ok(Self {
      dns,
      honeypot,
      request_rate,
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
      .compile_condition(engine.ip_sets(), engine.geoip())?;
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
  use rilgate_policy::policy::schema::RequestRateDefinition;

  #[test]
  fn reload_keeps_counts_until_the_window_or_capacity_changes() {
    let mut definition = Definition {
      request_rate: Some(RequestRateDefinition::default()),
      ..Default::default()
    };
    let plan = LookupPlan::default();
    let original = PolicyServices::new(&definition, &plan).unwrap();
    let ip = "192.0.2.1".parse().unwrap();
    assert_eq!(original.request_rate.as_ref().unwrap().record(ip), 1);
    let reloaded = PolicyServices::with_previous(&definition, &plan, Some(&original)).unwrap();
    assert_eq!(reloaded.request_rate.as_ref().unwrap().record(ip), 2);
    definition.request_rate.as_mut().unwrap().window *= 2;
    let changed = PolicyServices::with_previous(&definition, &plan, Some(&reloaded)).unwrap();
    assert_eq!(changed.request_rate.as_ref().unwrap().record(ip), 1);
    definition.request_rate.as_mut().unwrap().max_entries *= 2;
    let changed = PolicyServices::with_previous(&definition, &plan, Some(&changed)).unwrap();
    assert_eq!(changed.request_rate.as_ref().unwrap().record(ip), 1);
  }
}
