use super::WorkProvider;
use super::connection::Connections;
use crate::StratumPoolConfig;
use anyhow::Result;
use rilgate_core::{
  BoxFuture,
  error::{CapacityExceeded, PowBackendUnavailable},
};
use rilgate_protocol::StratumJob;
use rilgate_stratum::StratumSession;
use std::{
  collections::HashMap,
  sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
  },
  time::Duration,
};
use tokio::{
  sync::{OwnedSemaphorePermit, Semaphore, watch},
  time::Instant,
};

struct SessionUsage {
  last_requested: Instant,
  active: bool,
}

struct SessionLifetime(Arc<Mutex<SessionUsage>>);

impl Drop for SessionLifetime {
  fn drop(&mut self) {
    self
      .0
      .lock()
      .unwrap_or_else(|error| error.into_inner())
      .active = false;
  }
}

#[derive(Clone)]
struct PooledSession {
  jobs: watch::Receiver<Option<StratumJob>>,
  usage: Arc<Mutex<SessionUsage>>,
}

impl PooledSession {
  fn is_closed(&self) -> bool {
    self.jobs.has_changed().is_err()
      || !self
        .usage
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .active
  }

  fn touch(&self) -> bool {
    let mut usage = self.usage.lock().unwrap_or_else(|error| error.into_inner());
    if !usage.active {
      return false;
    }
    usage.last_requested = Instant::now();
    true
  }

  async fn current_job(mut self) -> Result<StratumJob> {
    loop {
      if self.is_closed() {
        return Err(anyhow::anyhow!(
          "pooled Stratum pool session is no longer running"
        ));
      }
      if let Some(job) = self.jobs.borrow_and_update().clone() {
        return Ok(job);
      }
      self
        .jobs
        .changed()
        .await
        .map_err(|_| anyhow::anyhow!("pooled Stratum pool session did not initialize"))?;
    }
  }
}

pub struct StratumPoolProvider {
  connections: Arc<Connections>,
  config: StratumPoolConfig,
  slots: Arc<Semaphore>,
  job_slots: Arc<Semaphore>,
  sessions: Mutex<HashMap<u64, PooledSession>>,
  started: Instant,
  failures: AtomicU64,
  capacity_failures: AtomicU64,
  last_warning_ms: AtomicU64,
}

const CONNECT_ATTEMPTS: usize = 2;

impl StratumPoolProvider {
  pub fn new(config: StratumPoolConfig) -> Result<Self> {
    config.validate()?;
    Ok(Self {
      job_slots: Arc::new(Semaphore::new(config.max_job_sessions)),
      slots: Arc::new(Semaphore::new(config.max_sessions)),
      sessions: Mutex::new(HashMap::new()),
      started: Instant::now(),
      failures: AtomicU64::new(0),
      capacity_failures: AtomicU64::new(0),
      last_warning_ms: AtomicU64::new(0),
      connections: Arc::new(Connections::new(config.clone())?),
      config,
    })
  }

  pub async fn connect(&self, difficulty: u64) -> Result<StratumSession> {
    self.connections.connect(difficulty).await
  }

  pub async fn readiness(&self, timeout: Duration) -> Vec<(String, bool)> {
    self.connections.readiness(timeout).await
  }

  pub fn connection_metrics(&self) -> String {
    self.connections.metrics()
  }

  pub fn active_sessions(&self) -> usize {
    self.config.max_sessions - self.slots.available_permits()
  }

  pub fn acquire_slot(&self) -> Result<OwnedSemaphorePermit> {
    self
      .slots
      .clone()
      .try_acquire_owned()
      .map_err(|_| CapacityExceeded.into())
  }

  /// Cumulative failed fetches, including session capacity rejections.
  pub fn failures(&self) -> (u64, u64) {
    (
      self.failures.load(Ordering::Relaxed),
      self.capacity_failures.load(Ordering::Relaxed),
    )
  }

  fn session(&self, difficulty: u64) -> Result<PooledSession> {
    let mut sessions = self
      .sessions
      .lock()
      .unwrap_or_else(|error| error.into_inner());
    sessions.retain(|_, session| !session.is_closed());
    if let Some(session) = sessions.get(&difficulty)
      && session.touch()
    {
      return Ok(session.clone());
    }
    let job_permit = self
      .job_slots
      .clone()
      .try_acquire_owned()
      .map_err(|_| CapacityExceeded)?;
    let slot = self.acquire_slot()?;
    let (jobs, receiver) = watch::channel(None);
    let usage = Arc::new(Mutex::new(SessionUsage {
      last_requested: Instant::now(),
      active: true,
    }));
    let session = PooledSession {
      jobs: receiver,
      usage: usage.clone(),
    };
    // Publishing the handle before connecting coalesces concurrent requests for one difficulty.
    tokio::spawn(run_session(
      self.connections.clone(),
      difficulty,
      jobs,
      SessionLifetime(usage),
      job_permit,
      slot,
      self.config.job_session_idle,
    ));
    sessions.insert(difficulty, session.clone());
    Ok(session)
  }

  async fn job(&self, difficulty: u64) -> Result<StratumJob> {
    if self.config.job_session_mode == crate::JobSessionMode::PerChallenge {
      return self.open_ephemeral(difficulty).await;
    }
    let mut last_error = None;
    for _ in 0..CONNECT_ATTEMPTS {
      match self.session(difficulty)?.current_job().await {
        Ok(job) => return Ok(job),
        Err(error) => last_error = Some(error),
      }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("unable to obtain a Stratum pool job")))
  }

  async fn open_ephemeral(&self, difficulty: u64) -> Result<StratumJob> {
    let _slot = self.acquire_slot()?;
    let session = self.connect(difficulty).await?;
    Ok(session.current_job)
  }

  fn record_failure(&self, error: &anyhow::Error) {
    self.failures.fetch_add(1, Ordering::Relaxed);
    if error.is::<CapacityExceeded>() {
      self.capacity_failures.fetch_add(1, Ordering::Relaxed);
    }
    let now = self.started.elapsed().as_millis() as u64 + 1;
    let last = self.last_warning_ms.load(Ordering::Relaxed);
    if (last == 0 || now.saturating_sub(last) >= 10_000)
      && self
        .last_warning_ms
        .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
    {
      let (failures, capacity_failures) = self.failures();
      tracing::warn!(
        ?error,
        failures,
        capacity_failures,
        "Stratum pool challenge unavailable"
      );
    }
  }
}

async fn run_session(
  connections: Arc<Connections>,
  difficulty: u64,
  jobs: watch::Sender<Option<StratumJob>>,
  lifetime: SessionLifetime,
  _job_permit: OwnedSemaphorePermit,
  _slot: OwnedSemaphorePermit,
  idle_timeout: Duration,
) {
  let connected = tokio::select! {
    result = connections.connect(difficulty) => result,
    _ = jobs.closed() => return,
  };
  let mut session = match connected {
    Ok(session) => session,
    Err(error) => {
      tracing::debug!(?error, "Unable to open pooled Stratum pool session");
      return;
    }
  };
  // A connection becomes idle only after its initial job is available.
  lifetime
    .0
    .lock()
    .unwrap_or_else(|error| error.into_inner())
    .last_requested = Instant::now();
  jobs.send_replace(Some(session.current_job.clone()));
  loop {
    let deadline = lifetime
      .0
      .lock()
      .unwrap_or_else(|error| error.into_inner())
      .last_requested
      + idle_timeout;
    tokio::select! {
      _ = jobs.closed() => return,
      _ = tokio::time::sleep_until(deadline) => {
        let mut usage = lifetime.0.lock().unwrap_or_else(|error| error.into_inner());
        if Instant::now() >= usage.last_requested + idle_timeout {
          usage.active = false;
          return;
        }
      }
      event = session.read_event() => {
        if event.is_err() {
          tracing::debug!("Pooled Stratum pool session connection lost");
          return;
        }
        jobs.send_if_modified(|current| {
          if current.as_ref() == Some(&session.current_job) {
            false
          } else {
            *current = Some(session.current_job.clone());
            true
          }
        });
      }
    }
  }
}

impl WorkProvider for StratumPoolProvider {
  type Work = StratumJob;
  fn fetch(&self, difficulty: u64) -> BoxFuture<'_, StratumJob> {
    Box::pin(async move {
      self.job(difficulty).await.map_err(|error| {
        self.record_failure(&error);
        PowBackendUnavailable.into()
      })
    })
  }
}
