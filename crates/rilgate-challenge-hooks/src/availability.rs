use crate::{HookSet, Outcome};
use std::{
  collections::BTreeMap,
  sync::{
    Mutex,
    atomic::{AtomicU32, AtomicU64, Ordering},
  },
  time::Instant,
};
#[derive(Default)]
pub(crate) struct Availability {
  consecutive: AtomicU32,
  unavailable_until: Mutex<Option<Instant>>,
  pub verifications: [AtomicU64; 3],
  pub retries: AtomicU64,
  pub fallbacks: AtomicU64,
}
impl Availability {
  pub fn is_unavailable(&self) -> bool {
    self
      .unavailable_until
      .lock()
      .expect("Hook availability lock")
      .is_some_and(|t| Instant::now() < t)
  }
  pub fn record(&self, name: &str, outcome: Outcome, set: &HookSet) {
    let hook = &set.hooks[name];
    self.verifications[outcome as usize].fetch_add(1, Ordering::Relaxed);
    let mut until = self
      .unavailable_until
      .lock()
      .expect("Hook availability lock");
    match outcome {
      Outcome::Pass | Outcome::Fail => {
        self.consecutive.store(0, Ordering::Relaxed);
        if until.take().is_some() {
          tracing::info!(hook = name, "Challenge hook is available again");
        }
      }
      Outcome::Unavailable => {
        let consecutive = self.consecutive.load(Ordering::Relaxed).saturating_add(1);
        self.consecutive.store(consecutive, Ordering::Relaxed);
        if consecutive >= hook.unavailable.after {
          if until.is_none() {
            tracing::warn!(hook = name, "Challenge hook is unavailable");
          }
          *until = Some(Instant::now() + hook.unavailable.recheck);
        }
      }
    }
  }
}
#[derive(Debug)]
pub struct HookMetrics {
  pub verifications: [u64; 3],
  pub retries: u64,
  pub fallbacks: u64,
  pub available: bool,
}
pub(crate) fn snapshot(states: &BTreeMap<String, Availability>) -> BTreeMap<String, HookMetrics> {
  states
    .iter()
    .map(|(name, s)| {
      (
        name.clone(),
        HookMetrics {
          verifications: std::array::from_fn(|i| s.verifications[i].load(Ordering::Relaxed)),
          retries: s.retries.load(Ordering::Relaxed),
          fallbacks: s.fallbacks.load(Ordering::Relaxed),
          available: !s.is_unavailable(),
        },
      )
    })
    .collect()
}
