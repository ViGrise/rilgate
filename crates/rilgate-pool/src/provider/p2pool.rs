use super::WorkProvider;
use crate::P2PoolConfig;
use anyhow::Result;
use rilgate_core::{
  BoxFuture,
  error::{CapacityExceeded, PowBackendUnavailable},
};
use rilgate_protocol::StratumJob;
use rilgate_stratum::{StratumConfig, StratumSession};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc, oneshot};

/// A job request travelling to the task that owns a pooled session.
type JobRequest = oneshot::Sender<Result<StratumJob>>;

/// Handle to a session owned by a background task.
///
/// The session itself is owned by that task so it can keep draining job
/// notifications while idle, which is what keeps the cached job current. Dropping
/// the handle closes the request channel and ends the task.
#[derive(Clone)]
struct PooledSession {
  difficulty: u64,
  jobs: mpsc::Sender<JobRequest>,
}

impl PooledSession {
  /// Asks the owning task for its current job.
  ///
  /// Fails when the task has exited, which means the connection is gone and the
  /// caller must connect a replacement.
  async fn current_job(&self) -> Result<StratumJob> {
    let (reply, answered) = oneshot::channel();
    self
      .jobs
      .send(reply)
      .await
      .map_err(|_| anyhow::anyhow!("pooled P2Pool session is no longer running"))?;
    answered
      .await
      .map_err(|_| anyhow::anyhow!("pooled P2Pool session did not answer"))?
  }
}

pub struct P2PoolProvider {
  config: P2PoolConfig,
  slots: Arc<Semaphore>,
  job_slots: Arc<Semaphore>,
  idle: Mutex<Vec<PooledSession>>,
}

/// Upper bound on connection attempts for one `fetch`, so a pool that keeps
/// handing back dead sessions cannot spin.
const CONNECT_ATTEMPTS: usize = 2;

impl P2PoolProvider {
  pub fn new(config: P2PoolConfig) -> Result<Self> {
    config.validate()?;
    Ok(Self {
      job_slots: Arc::new(Semaphore::new(config.max_job_sessions)),
      slots: Arc::new(Semaphore::new(config.max_sessions)),
      idle: Mutex::new(Vec::new()),
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

  /// Reuses an idle session logged in at `difficulty`, if one exists.
  async fn take_idle(&self, difficulty: u64) -> Option<PooledSession> {
    let mut idle = self.idle.lock().await;
    idle.retain(|session| !session.jobs.is_closed());
    let index = idle
      .iter()
      .position(|session| session.difficulty == difficulty)?;
    Some(idle.swap_remove(index))
  }

  async fn remember(&self, session: PooledSession) {
    self.idle.lock().await.push(session);
  }

  /// Opens a session and hands ownership to a background task.
  ///
  /// Permits are taken here rather than per request, so a pooled session keeps
  /// its slot while it waits to be reused.
  async fn open(&self, difficulty: u64) -> Result<PooledSession> {
    let job_permit = self
      .job_slots
      .clone()
      .try_acquire_owned()
      .map_err(|_| CapacityExceeded)?;
    let slot = self.acquire_slot()?;

    let (jobs, requests) = mpsc::channel(1);
    let session = PooledSession { difficulty, jobs };

    tokio::spawn(run_session(
      self.config.stratum(),
      difficulty,
      requests,
      job_permit,
      slot,
      Duration::from_secs(self.config.job_session_idle_seconds),
    ));

    Ok(session)
  }

  async fn job(&self, difficulty: u64) -> Result<StratumJob> {
    if self.config.max_job_sessions == 0 {
      return self.open_ephemeral(difficulty).await;
    }

    let mut last_error = None;
    for _ in 0..CONNECT_ATTEMPTS {
      let session = match self.take_idle(difficulty).await {
        Some(session) => session,
        None => match self.open(difficulty).await {
          Ok(session) => session,
          Err(error) => return Err(error),
        },
      };

      match session.current_job().await {
        Ok(job) => {
          self.remember(session).await;
          return Ok(job);
        }
        Err(error) => {
          // Either the owning task has already exited, or it is still waiting on
          // a request nobody will collect. Dropping the handle at the end of this
          // arm covers both: a closed channel ends the task, which drops its
          // permits.
          tracing::warn!(?error, "Discarding unusable pooled P2Pool session");
          last_error = Some(error);
        }
      }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("unable to obtain a P2Pool job")))
  }

  /// Fetches one job within the shared session budget without retaining a connection.
  async fn open_ephemeral(&self, difficulty: u64) -> Result<StratumJob> {
    let _slot = self.acquire_slot()?;
    let session = StratumSession::connect(&self.config.stratum(), difficulty).await?;
    Ok(session.current_job)
  }
}

/// Owns one pooled session until it is idle for too long or its connection dies.
async fn run_session(
  config: StratumConfig,
  difficulty: u64,
  mut requests: mpsc::Receiver<JobRequest>,
  _job_permit: OwnedSemaphorePermit,
  _slot: OwnedSemaphorePermit,
  idle_timeout: Duration,
) {
  let mut session = match StratumSession::connect(&config, difficulty).await {
    Ok(session) => session,
    Err(error) => {
      tracing::warn!(?error, "Unable to open pooled P2Pool session");
      return;
    }
  };

  let mut idle_deadline = tokio::time::Instant::now() + idle_timeout;

  loop {
    tokio::select! {
      _ = tokio::time::sleep_until(idle_deadline) => {
        tracing::debug!("Closing idle pooled P2Pool session");
        return;
      }

      request = requests.recv() => {
        let Some(reply) = request else {
          // Every handle was dropped, so the provider is shutting down.
          return;
        };

        // Apply any job rotation that arrived while this session sat idle, so
        // the caller receives the job P2Pool currently considers valid. A zero
        // timeout polls read_event once, which drains only already-buffered
        // notifications and leaves the connection untouched.
        while let Ok(Ok(_)) =
          tokio::time::timeout(Duration::ZERO, session.read_event()).await
        {}

        let job = session.current_job.clone();
        if reply.send(Ok(job)).is_err() {
          // The caller gave up waiting.
          return;
        }

        idle_deadline = tokio::time::Instant::now() + idle_timeout;
      }

      event = session.read_event() => {
        if event.is_err() {
          tracing::warn!("Pooled P2Pool session connection lost");
          return;
        }
      }
    }
  }
}

impl WorkProvider for P2PoolProvider {
  type Work = StratumJob;
  fn fetch(&self, difficulty: u64) -> BoxFuture<'_, StratumJob> {
    Box::pin(async move {
      self.job(difficulty).await.map_err(|error| {
        tracing::warn!(?error, "P2Pool challenge unavailable");
        PowBackendUnavailable.into()
      })
    })
  }
}
