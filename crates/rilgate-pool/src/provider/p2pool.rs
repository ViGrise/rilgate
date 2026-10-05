use super::WorkProvider;
use crate::P2PoolConfig;
use anyhow::Result;
use rilgate_core::{
  BoxFuture,
  error::{CapacityExceeded, PowBackendUnavailable},
};
use rilgate_protocol::StratumJob;
use rilgate_stratum as stratum;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
pub struct P2PoolProvider {
  config: P2PoolConfig,
  slots: Arc<Semaphore>,
}
impl P2PoolProvider {
  pub fn new(config: P2PoolConfig) -> Result<Self> {
    config.validate()?;
    Ok(Self {
      slots: Arc::new(Semaphore::new(config.max_sessions)),
      config,
    })
  }
  pub fn acquire_slot(&self) -> Result<OwnedSemaphorePermit> {
    self
      .slots
      .clone()
      .try_acquire_owned()
      .map_err(|_| CapacityExceeded.into())
  }
}
impl WorkProvider for P2PoolProvider {
  type Work = StratumJob;
  fn fetch(&self, difficulty: u64) -> BoxFuture<'_, StratumJob> {
    Box::pin(async move {
      let _slot = self.acquire_slot()?;
      stratum::fetch_job(&self.config.stratum(), difficulty)
        .await
        .map_err(|error| {
          tracing::warn!(?error, "P2Pool challenge unavailable");
          PowBackendUnavailable.into()
        })
    })
  }
}
