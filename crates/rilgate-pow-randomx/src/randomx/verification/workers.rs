use std::{
  sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
  },
  thread,
};

#[derive(Default)]
struct Stats {
  active: AtomicBool,
  completed: AtomicU64,
  rejected: AtomicU64,
}

use anyhow::{Context, Result, anyhow};
use randomx_core::RandomX;
use tokio::sync::{mpsc, oneshot};

use rilgate_core::error::CapacityExceeded;

struct HashTask {
  seed: Vec<u8>,
  blob: Vec<u8>,
  reply: oneshot::Sender<Result<[u8; 32]>>,
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

impl Default for RandomXVerifier {
  fn default() -> Self {
    Self::new()
  }
}

impl RandomXVerifier {
  pub fn new() -> Self {
    Self::with_capacity(8).expect("Failed to spawn RandomX verifier thread")
  }

  pub fn with_capacity(capacity: usize) -> Result<Self> {
    let mut active: Option<(Vec<u8>, RandomX)> = None;
    Self::spawn(capacity, move |seed, blob| {
      if !active.as_ref().is_some_and(|(key, _)| key == seed) {
        // Seed changes must not keep two 256 MiB caches resident.
        active = None;
        active = Some((
          seed.to_vec(),
          RandomX::new(seed).context("RandomX cache initialization failed")?,
        ));
      }
      active
        .as_mut()
        .expect("RandomX cache initialized")
        .1
        .hash(blob)
        .context("RandomX hashing failed")
    })
  }

  fn spawn(
    capacity: usize,
    mut hash: impl FnMut(&[u8], &[u8]) -> Result<[u8; 32]> + Send + 'static,
  ) -> Result<Self> {
    if !(1..=1024).contains(&capacity) {
      return Err(anyhow!(
        "RandomX verifier queue capacity must be between 1 and 1024"
      ));
    }
    let (tx, mut rx) = mpsc::channel::<HashTask>(capacity);
    let stats = Arc::new(Stats::default());
    let worker_stats = stats.clone();
    thread::Builder::new()
      .name("randomx-verifier".into())
      .spawn(move || {
        while let Some(task) = rx.blocking_recv() {
          // Dropped requests no longer need CPU work or a cache for their seed.
          if task.reply.is_closed() {
            continue;
          }
          worker_stats.active.store(true, Ordering::Relaxed);
          let result = hash(&task.seed, &task.blob);
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

  pub async fn hash(&self, seed: Vec<u8>, blob: Vec<u8>) -> Result<[u8; 32]> {
    if seed.is_empty() || seed.len() > 256 || blob.is_empty() || blob.len() > 256 {
      return Err(anyhow!(
        "RandomX seed and input must contain between 1 and 256 bytes"
      ));
    }
    let (reply, rx) = oneshot::channel();

    self
      .tx
      .try_send(HashTask { seed, blob, reply })
      .map_err(|error| match error {
        mpsc::error::TrySendError::Full(_) => {
          self.stats.rejected.fetch_add(1, Ordering::Relaxed);
          anyhow::Error::new(CapacityExceeded)
        }
        mpsc::error::TrySendError::Closed(_) => anyhow!("RandomX verifier thread stopped"),
      })?;

    rx.await
      .map_err(|_| anyhow!("RandomX verifier response dropped"))?
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::randomx::verification::write_nonce;

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
    let verifier = RandomXVerifier::spawn(1, move |_, _| {
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
    assert!(error.is::<CapacityExceeded>());
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
  }
  /// Recomputes the official RandomX vector through the gateway verifier, so
  /// the cache reuse, nonce patching and native hashing all stay in step with
  /// the reference implementation.
  #[tokio::test]
  #[ignore = "allocates a 256 MiB Argon2d cache"]
  async fn verifier_recomputes_reference_vector() {
    const KEY: &str = "7797373ea4633194640bf8d8c3b66724d6aa7bd2dc20e009df2f8f1710abe8";
    const BLOB: &str = "1010e1eaf8cf067b37b5f0ee031ab23ed1755e090a3af4415830145\
                        853e2be3e1f6821fed84dae58d00e00da5214d6c1f2d0622e0abd51f9373\
                        d04e0b0f8e6d6514d90689721c4aac5a9bb0d";
    const EXPECTED: &str = "78af2a1864c42abce36d2e8983e13df99b2af0ce1362999af09fab004d4435a8";

    let verifier = RandomXVerifier::new();

    let seed = hex::decode(KEY).expect("seed hex");
    let mut blob = hex::decode(BLOB.replace([' ', '\n'], "")).expect("blob hex");

    write_nonce(&mut blob, 39, "000ed058").expect("nonce offset");

    let hash = verifier
      .hash(seed.clone(), blob.clone())
      .await
      .expect("hash");
    assert_eq!(hex::encode(hash), EXPECTED);

    // The same seed must reuse the cache and stay deterministic.
    let repeated = verifier.hash(seed, blob).await.expect("hash");
    assert_eq!(repeated, hash);
  }
}
