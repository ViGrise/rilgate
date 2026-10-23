use std::{
  sync::{Arc, Mutex},
  time::{Duration, Instant},
};

use anyhow::anyhow;
use futures_util::future::BoxFuture;

use super::RandomXVerifier;

#[derive(Debug)]
pub enum HashError {
  Unavailable(anyhow::Error),
  Invalid(anyhow::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HasherKind {
  Local,
  Remote,
}

/// Decides whether a remote hasher that cannot use the request deadline ends
/// the sequence with an invalid proof or passes the request to the next hasher.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeadlinePolicy {
  #[default]
  Fallback,
  Reject,
}

pub trait RandomXHasher: Send + Sync {
  fn kind(&self) -> HasherKind;
  fn ready(&self, _: Duration) -> BoxFuture<'_, bool> {
    Box::pin(async { false })
  }
  fn hash(
    &self,
    seed: Vec<u8>,
    input: Vec<u8>,
    deadline: Instant,
  ) -> BoxFuture<'_, Result<[u8; 32], HashError>>;
}

pub struct LocalHasher {
  capacity: usize,
  idle_timeout: Duration,
  verifier: Mutex<Option<RandomXVerifier>>,
}

impl LocalHasher {
  pub fn new(capacity: usize, idle_timeout: Duration) -> Self {
    Self {
      capacity,
      idle_timeout,
      verifier: Mutex::new(None),
    }
  }

  fn verifier(&self) -> anyhow::Result<RandomXVerifier> {
    let mut verifier = self
      .verifier
      .lock()
      .unwrap_or_else(|error| error.into_inner());
    if verifier.is_none() {
      *verifier = Some(RandomXVerifier::with_idle_timeout(
        self.capacity,
        self.idle_timeout,
      )?);
    }
    Ok(
      verifier
        .as_ref()
        .expect("RandomX verifier initialized")
        .clone(),
    )
  }

  pub fn diagnostics(&self) -> rilgate_pow::algorithm::VerifierDiagnostics {
    self
      .verifier
      .lock()
      .unwrap_or_else(|error| error.into_inner())
      .as_ref()
      .map_or_else(Default::default, RandomXVerifier::diagnostics)
  }
}

impl RandomXHasher for LocalHasher {
  fn kind(&self) -> HasherKind {
    HasherKind::Local
  }

  fn hash(
    &self,
    seed: Vec<u8>,
    input: Vec<u8>,
    deadline: Instant,
  ) -> BoxFuture<'_, Result<[u8; 32], HashError>> {
    Box::pin(async move {
      self
        .verifier()
        .map_err(HashError::Unavailable)?
        .hash_with_deadline(seed, input, Some(deadline))
        .await
    })
  }
}

pub struct OrderedHasher {
  hashers: Vec<Arc<dyn RandomXHasher>>,
  metrics: super::metrics::HashMetrics,
  deadline_policy: DeadlinePolicy,
}

impl OrderedHasher {
  pub fn new(hashers: Vec<Arc<dyn RandomXHasher>>) -> Self {
    Self {
      hashers,
      metrics: Default::default(),
      deadline_policy: Default::default(),
    }
  }

  pub fn with_deadline_policy(mut self, policy: DeadlinePolicy) -> Self {
    self.deadline_policy = policy;
    self
  }
  pub fn diagnostics(&self) -> rilgate_pow::algorithm::HashDiagnostics {
    self.metrics.snapshot()
  }

  pub async fn remote_ready(&self, timeout: Duration) -> bool {
    for hasher in &self.hashers {
      if hasher.kind() == HasherKind::Remote {
        return hasher.ready(timeout).await;
      }
    }
    false
  }

  pub async fn hash(
    &self,
    seed: Vec<u8>,
    input: Vec<u8>,
    deadline: Instant,
  ) -> Result<[u8; 32], HashError> {
    let mut unavailable = anyhow!("No RandomX hasher available");
    for (index, hasher) in self.hashers.iter().enumerate() {
      if Instant::now() >= deadline {
        break;
      }
      let duration = self.metrics.start(hasher.kind());
      let result = hasher.hash(seed.clone(), input.clone(), deadline).await;
      drop(duration);
      let result = match result {
        Err(HashError::Unavailable(error))
          if self.deadline_policy == DeadlinePolicy::Reject
            && matches!(
              error.downcast_ref::<super::remote::UnavailableReason>(),
              Some(super::remote::UnavailableReason::Deadline)
            ) =>
        {
          Err(HashError::Invalid(error))
        }
        result => result,
      };
      self.metrics.observe(hasher.kind(), &result);
      match result {
        Err(HashError::Unavailable(error)) => {
          if index + 1 < self.hashers.len() && Instant::now() < deadline {
            self.metrics.fallback(hasher.kind(), &error);
          }
          unavailable = error;
        }
        result => return result,
      }
    }
    Err(HashError::Unavailable(unavailable))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::atomic::{AtomicUsize, Ordering};
  use std::time::Duration;

  enum Outcome {
    Hash,
    Unavailable,
    DelayedUnavailable,
    Invalid,
    Deadline,
    Pending,
  }

  struct FakeHasher {
    outcome: Outcome,
    calls: Arc<AtomicUsize>,
  }

  impl RandomXHasher for FakeHasher {
    fn kind(&self) -> HasherKind {
      HasherKind::Local
    }

    fn hash(
      &self,
      _: Vec<u8>,
      _: Vec<u8>,
      _: Instant,
    ) -> BoxFuture<'_, Result<[u8; 32], HashError>> {
      self.calls.fetch_add(1, Ordering::SeqCst);
      Box::pin(async {
        match self.outcome {
          Outcome::Hash => Ok([7; 32]),
          Outcome::Unavailable => Err(HashError::Unavailable(anyhow!("unavailable"))),
          Outcome::DelayedUnavailable => {
            std::thread::sleep(Duration::from_millis(20));
            Err(HashError::Unavailable(anyhow!("unavailable")))
          }
          Outcome::Invalid => Err(HashError::Invalid(anyhow!("invalid"))),
          Outcome::Deadline => Err(HashError::Unavailable(anyhow!(
            super::super::remote::UnavailableReason::Deadline
          ))),
          Outcome::Pending => std::future::pending().await,
        }
      })
    }
  }

  fn ordered(outcomes: Vec<Outcome>) -> (OrderedHasher, Vec<Arc<AtomicUsize>>) {
    let calls: Vec<_> = outcomes
      .iter()
      .map(|_| Arc::new(AtomicUsize::new(0)))
      .collect();
    let hashers = outcomes
      .into_iter()
      .zip(&calls)
      .map(|(outcome, calls)| {
        Arc::new(FakeHasher {
          outcome,
          calls: calls.clone(),
        }) as Arc<dyn RandomXHasher>
      })
      .collect();
    (OrderedHasher::new(hashers), calls)
  }

  #[tokio::test]
  async fn deadline_policy_controls_the_next_hasher() {
    for policy in [DeadlinePolicy::Fallback, DeadlinePolicy::Reject] {
      let (hasher, calls) = ordered(vec![Outcome::Deadline, Outcome::Hash]);
      let hasher = hasher.with_deadline_policy(policy);
      let result = hasher
        .hash(
          vec![1; 32],
          vec![2; 76],
          Instant::now() + Duration::from_secs(1),
        )
        .await;
      let outcomes = hasher.diagnostics().outcomes[0];
      if policy == DeadlinePolicy::Fallback {
        assert_eq!(result.unwrap(), [7; 32]);
        assert_eq!(calls[1].load(Ordering::SeqCst), 1);
        assert_eq!(outcomes, [1, 1, 0]);
      } else {
        assert!(matches!(result, Err(HashError::Invalid(_))));
        assert_eq!(calls[1].load(Ordering::SeqCst), 0);
        assert_eq!(outcomes, [0, 0, 1]);
      }
    }
  }

  #[tokio::test]
  async fn unavailable_advances_to_the_next_hasher() {
    let (hasher, calls) = ordered(vec![Outcome::Unavailable, Outcome::Hash]);
    let hash = hasher
      .hash(vec![1], vec![2], Instant::now() + Duration::from_secs(1))
      .await
      .unwrap();
    assert_eq!(hash, [7; 32]);
    assert!(calls.iter().all(|count| count.load(Ordering::SeqCst) == 1));
  }

  #[tokio::test]
  async fn invalid_and_returned_hash_are_final() {
    for outcome in [Outcome::Invalid, Outcome::Hash] {
      let invalid = matches!(outcome, Outcome::Invalid);
      let (hasher, calls) = ordered(vec![outcome, Outcome::Hash]);
      let result = hasher
        .hash(vec![1], vec![2], Instant::now() + Duration::from_secs(1))
        .await;
      if invalid {
        assert!(matches!(result, Err(HashError::Invalid(_))));
      } else {
        assert_eq!(result.unwrap(), [7; 32]);
      }
      assert_eq!(calls[1].load(Ordering::SeqCst), 0);
    }
  }

  #[tokio::test]
  async fn exhausted_order_is_unavailable() {
    for outcomes in [vec![], vec![Outcome::Unavailable, Outcome::Unavailable]] {
      let (hasher, calls) = ordered(outcomes);
      assert!(matches!(
        hasher
          .hash(vec![1], vec![2], Instant::now() + Duration::from_secs(1))
          .await,
        Err(HashError::Unavailable(_))
      ));
      assert!(calls.iter().all(|count| count.load(Ordering::SeqCst) == 1));
    }
  }

  #[tokio::test]
  async fn expired_deadline_does_not_start_a_hasher() {
    let (hasher, calls) = ordered(vec![Outcome::Hash]);
    let result = hasher.hash(vec![1], vec![2], Instant::now()).await;
    assert!(matches!(result, Err(HashError::Unavailable(_))));
    assert_eq!(calls[0].load(Ordering::SeqCst), 0);
  }

  #[tokio::test]
  async fn elapsed_deadline_stops_fallback() {
    let (hasher, calls) = ordered(vec![Outcome::DelayedUnavailable, Outcome::Hash]);
    let deadline = Instant::now() + Duration::from_millis(10);
    let result = hasher.hash(vec![1], vec![2], deadline).await;
    assert!(matches!(result, Err(HashError::Unavailable(_))));
    assert_eq!(calls[0].load(Ordering::SeqCst), 1);
    assert_eq!(calls[1].load(Ordering::SeqCst), 0);
  }

  #[tokio::test]
  async fn outer_deadline_cancels_the_sequence() {
    let (hasher, calls) = ordered(vec![Outcome::Unavailable, Outcome::Pending, Outcome::Hash]);
    let deadline = Instant::now() + Duration::from_secs(60);
    let result = tokio::time::timeout(
      Duration::from_millis(20),
      hasher.hash(vec![1], vec![2], deadline),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(calls[0].load(Ordering::SeqCst), 1);
    assert_eq!(calls[1].load(Ordering::SeqCst), 1);
    assert_eq!(calls[2].load(Ordering::SeqCst), 0);
  }
}
