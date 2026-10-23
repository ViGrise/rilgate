use std::{
  sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
  },
  thread,
  time::{Duration, Instant},
};

#[derive(Default)]
struct Stats {
  active: AtomicBool,
  completed: AtomicU64,
  rejected: AtomicU64,
  #[cfg(test)]
  released: AtomicU64,
}

use anyhow::{Context, Result, anyhow};
use randomx_core::RandomX;
use tokio::sync::{mpsc, oneshot};

use rilgate_core::error::CapacityExceeded;

use super::HashError;

struct HashTask {
  seed: Vec<u8>,
  blob: Vec<u8>,
  reply: oneshot::Sender<Result<[u8; 32], HashError>>,
}

/// Owns the Light mode RandomX cache and VM.
///
/// Cache construction needs roughly 256 MiB and several seconds of Argon2d, so
/// it is kept on a dedicated thread that is reused while the seed is stable
/// instead of running on a Tokio worker.
#[derive(Clone)]
pub struct RandomXVerifier {
  tx: mpsc::Sender<HashTask>,
  stats: Arc<Stats>,
}

impl RandomXVerifier {
  pub fn with_idle_timeout(capacity: usize, idle_timeout: Duration) -> Result<Self> {
    Self::spawn(
      capacity,
      idle_timeout,
      move |active: &mut Option<(Vec<u8>, RandomX)>, seed, blob| {
        if !active.as_ref().is_some_and(|(key, _)| key == seed) {
          // Seed changes must not keep two 256 MiB caches resident.
          *active = None;
          *active = Some((
            seed.to_vec(),
            RandomX::new(seed)
              .context("RandomX cache initialization failed")
              .map_err(HashError::Unavailable)?,
          ));
        }
        active
          .as_mut()
          .expect("RandomX cache initialized")
          .1
          .hash(blob)
          .context("RandomX hashing failed")
          .map_err(HashError::Unavailable)
      },
    )
  }

  fn spawn<C: Send + 'static>(
    capacity: usize,
    idle_timeout: Duration,
    mut hash: impl FnMut(&mut Option<C>, &[u8], &[u8]) -> Result<[u8; 32], HashError> + Send + 'static,
  ) -> Result<Self> {
    if !(1..=1024).contains(&capacity) {
      return Err(anyhow!(
        "RandomX verifier queue capacity must be between 1 and 1024"
      ));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
      .enable_time()
      .build()
      .context("Failed to create RandomX verifier runtime")?;
    let (tx, mut rx) = mpsc::channel::<HashTask>(capacity);
    let stats = Arc::new(Stats::default());
    let worker_stats = stats.clone();
    thread::Builder::new()
      .name("randomx-verifier".into())
      .spawn(move || {
        let mut cache = None;
        let mut last_hash = Instant::now();
        loop {
          let task = runtime.block_on(async {
            if !idle_timeout.is_zero() && cache.is_some() {
              let remaining = idle_timeout.saturating_sub(last_hash.elapsed());
              tokio::time::timeout(remaining, rx.recv()).await
            } else {
              Ok(rx.recv().await)
            }
          });
          let task = match task {
            Ok(Some(task)) => task,
            Ok(None) => break,
            Err(_) => {
              cache = None;
              #[cfg(test)]
              worker_stats.released.fetch_add(1, Ordering::Relaxed);
              continue;
            }
          };
          // Dropped requests no longer need CPU work or a cache for their seed.
          if task.reply.is_closed() {
            continue;
          }
          worker_stats.active.store(true, Ordering::Relaxed);
          let result = hash(&mut cache, &task.seed, &task.blob);
          last_hash = Instant::now();
          worker_stats.active.store(false, Ordering::Relaxed);
          worker_stats.completed.fetch_add(1, Ordering::Relaxed);
          let _ = task.reply.send(result);
        }
      })
      .context("Failed to spawn RandomX verifier thread")?;
    Ok(Self { tx, stats })
  }

  pub fn diagnostics(&self) -> rilgate_pow::algorithm::VerifierDiagnostics {
    rilgate_pow::algorithm::VerifierDiagnostics {
      queued: self.tx.max_capacity() - self.tx.capacity(),
      active: self.stats.active.load(Ordering::Relaxed),
      completed: self.stats.completed.load(Ordering::Relaxed),
      rejected: self.stats.rejected.load(Ordering::Relaxed),
    }
  }

  pub async fn hash(&self, seed: Vec<u8>, blob: Vec<u8>) -> Result<[u8; 32], HashError> {
    if seed.is_empty() || seed.len() > 256 || blob.is_empty() || blob.len() > 256 {
      return Err(HashError::Invalid(anyhow!(
        "RandomX seed and input must contain between 1 and 256 bytes"
      )));
    }
    let (reply, rx) = oneshot::channel();

    self
      .tx
      .try_send(HashTask { seed, blob, reply })
      .map_err(|error| {
        HashError::Unavailable(match error {
          mpsc::error::TrySendError::Full(_) => {
            self.stats.rejected.fetch_add(1, Ordering::Relaxed);
            anyhow::Error::new(CapacityExceeded)
          }
          mpsc::error::TrySendError::Closed(_) => anyhow!("RandomX verifier thread stopped"),
        })
      })?;

    rx.await
      .map_err(|_| HashError::Unavailable(anyhow!("RandomX verifier response dropped")))?
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::randomx::verification::write_nonce;

  #[tokio::test]
  async fn idle_cache_is_released_and_rebuilt() {
    let mut builds = 0;
    let verifier = RandomXVerifier::spawn(1, Duration::from_millis(20), move |cache, _, _| {
      if cache.is_none() {
        builds += 1;
        *cache = Some(builds);
      }
      Ok([cache.unwrap(); 32])
    })
    .unwrap();
    assert_eq!(verifier.hash(vec![1], vec![1]).await.unwrap(), [1; 32]);
    tokio::time::timeout(Duration::from_secs(2), async {
      while verifier.stats.released.load(Ordering::Relaxed) == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
      }
    })
    .await
    .unwrap();
    assert_eq!(verifier.hash(vec![1], vec![1]).await.unwrap(), [2; 32]);
  }

  #[tokio::test]
  async fn zero_idle_timeout_keeps_the_cache() {
    let verifier = RandomXVerifier::spawn(1, Duration::ZERO, |cache, _, _| {
      let uses = cache.get_or_insert(0);
      *uses += 1;
      Ok([*uses; 32])
    })
    .unwrap();
    assert_eq!(verifier.hash(vec![1], vec![1]).await.unwrap(), [1; 32]);
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(verifier.stats.released.load(Ordering::Relaxed), 0);
    assert_eq!(verifier.hash(vec![1], vec![1]).await.unwrap(), [2; 32]);
  }

  #[tokio::test]
  async fn dimensions_are_invalid_and_worker_failures_are_unavailable() {
    let verifier = RandomXVerifier::spawn::<()>(1, Duration::ZERO, |_, _, _| {
      Err(HashError::Unavailable(anyhow!(
        "RandomX cache initialization failed"
      )))
    })
    .unwrap();
    for (seed, blob) in [
      (vec![], vec![1]),
      (vec![1; 257], vec![1]),
      (vec![1], vec![]),
      (vec![1], vec![1; 257]),
    ] {
      assert!(matches!(
        verifier.hash(seed, blob).await,
        Err(HashError::Invalid(_))
      ));
    }
    assert!(matches!(
      verifier.hash(vec![1], vec![1]).await,
      Err(HashError::Unavailable(_))
    ));
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    let stopped = RandomXVerifier {
      tx,
      stats: Arc::new(Stats::default()),
    };
    assert!(matches!(
      stopped.hash(vec![1], vec![1]).await,
      Err(HashError::Unavailable(_))
    ));
  }

  #[tokio::test]
  async fn overload_is_rejected_and_canceled_work_is_skipped() {
    use std::sync::{
      Arc,
      atomic::{AtomicUsize, Ordering},
    };
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    let (started, wait_started) = oneshot::channel();
    let mut started = Some(started);
    let (release, wait) = std::sync::mpsc::channel();
    let verifier = RandomXVerifier::spawn::<()>(1, Duration::ZERO, move |_, _, _| {
      if calls.fetch_add(1, Ordering::SeqCst) == 0 {
        started.take().unwrap().send(()).unwrap();
        wait.recv().unwrap();
      }
      Ok([0; 32])
    })
    .unwrap();
    let first = {
      let verifier = verifier.clone();
      tokio::spawn(async move { verifier.hash(vec![1], vec![1]).await })
    };
    wait_started.await.unwrap();
    let (reply, response) = oneshot::channel();
    verifier
      .tx
      .try_send(HashTask {
        seed: vec![2],
        blob: vec![2],
        reply,
      })
      .unwrap();
    let error = verifier.hash(vec![3], vec![3]).await.unwrap_err();
    assert!(matches!(error, HashError::Unavailable(error) if error.is::<CapacityExceeded>()));
    drop(response);
    release.send(()).unwrap();
    first.await.unwrap().unwrap();
    let (reply, response) = oneshot::channel();
    verifier
      .tx
      .send(HashTask {
        seed: vec![4],
        blob: vec![4],
        reply,
      })
      .await
      .unwrap();
    response.await.unwrap().unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
    let diagnostics = verifier.diagnostics();
    assert_eq!(diagnostics.completed, 2);
    assert_eq!(diagnostics.rejected, 1);
    assert!(!diagnostics.active);
    assert_eq!(diagnostics.queued, 0);
  }
  /// Checks the reference hash across cache reuse and idle release.
  #[tokio::test]
  #[ignore = "allocates a 256 MiB Argon2d cache"]
  async fn verifier_recomputes_reference_vector() {
    const KEY: &str = "7797373ea4633194640bf8d8c3b66724d6aa7bd2dc20e009df2f8f1710abe8";
    const BLOB: &str = "1010e1eaf8cf067b37b5f0ee031ab23ed1755e090a3af4415830145\
                        853e2be3e1f6821fed84dae58d00e00da5214d6c1f2d0622e0abd51f9373\
                        d04e0b0f8e6d6514d90689721c4aac5a9bb0d";
    const EXPECTED: &str = "78af2a1864c42abce36d2e8983e13df99b2af0ce1362999af09fab004d4435a8";

    let verifier = RandomXVerifier::with_idle_timeout(8, Duration::from_millis(20)).unwrap();

    let seed = hex::decode(KEY).expect("seed hex");
    let mut blob = hex::decode(BLOB.replace([' ', '\n'], "")).expect("blob hex");

    write_nonce(&mut blob, 39, "000ed058").expect("nonce offset");

    let hash = verifier
      .hash(seed.clone(), blob.clone())
      .await
      .expect("hash");
    assert_eq!(hex::encode(hash), EXPECTED);

    assert_eq!(
      verifier.hash(seed.clone(), blob.clone()).await.unwrap(),
      hash
    );
    tokio::time::timeout(Duration::from_secs(2), async {
      while verifier.stats.released.load(Ordering::Relaxed) == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
      }
    })
    .await
    .expect("Cache released");
    let (rebuilt, queued) = tokio::join!(
      verifier.hash(seed.clone(), blob.clone()),
      verifier.hash(seed, blob),
    );
    assert_eq!(rebuilt.unwrap(), hash);
    assert_eq!(queued.unwrap(), hash);
  }
}
