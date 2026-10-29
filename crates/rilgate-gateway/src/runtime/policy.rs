use super::honeypot::Honeypot;
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
}

impl PolicyServices {
  pub fn new(definition: &Definition, plan: &LookupPlan) -> Result<Self> {
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
    Ok(Self { dns, honeypot })
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
    let services = PolicyServices::new(definition, engine.lookup_plan())?;
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
