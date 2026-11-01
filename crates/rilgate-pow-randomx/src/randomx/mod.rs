pub mod verification;
mod work;
use anyhow::{Context, Result, anyhow};
use rilgate_core::{error::PowBackendUnavailable, time::unix_now};
use rilgate_pool::WorkProvider;
use rilgate_pow::{
  Algorithm, AlgorithmDescriptor, AlgorithmFuture, PowAlgorithm, PreparedWork, Proof, Work,
  WorkRequest, difficulty::DifficultyConfig,
};
use rilgate_protocol::{PowVerification, StratumJob};
use std::{
  sync::Arc,
  time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;
use verification::{HashError, LocalHasher, OrderedHasher, hash_meets_difficulty, write_nonce};
pub struct RandomXAlgorithm {
  difficulty: DifficultyConfig,
  ttl: u64,
  nonce_offset: usize,
  solver_module_path: String,
  browser: rilgate_pow::algorithm::BrowserSolverOptions,
  hasher: OrderedHasher,
  local_hasher: Option<Arc<LocalHasher>>,
  provider: Arc<dyn WorkProvider<Work = StratumJob>>,
}
#[derive(Debug, Clone)]
pub struct RandomXConfig {
  pub difficulty: DifficultyConfig,
  pub ttl: u64,
  pub nonce_offset: usize,
  pub solver_module_path: String,
  pub max_workers: u8,
  pub store_cache: bool,
  pub job_details: bool,
  pub queue_capacity: usize,
  pub cache_idle_timeout: Duration,
}
/// The hashers that verify proofs, tried in `order`.
#[derive(Debug, Clone)]
pub struct HasherOrder {
  pub order: Vec<verification::HasherKind>,
  pub remote: Option<RemoteHasherConfig>,
  pub deadline_policy: verification::DeadlinePolicy,
}
impl Default for HasherOrder {
  fn default() -> Self {
    Self {
      order: vec![verification::HasherKind::Local],
      remote: None,
      deadline_policy: Default::default(),
    }
  }
}
#[derive(Debug, Clone)]
pub struct RemoteHasherConfig {
  pub endpoint: String,
  pub server_name: String,
  pub timeout: Duration,
  pub ca: Option<std::path::PathBuf>,
  pub client_cert: Option<std::path::PathBuf>,
  pub client_key: Option<std::path::PathBuf>,
}
impl RandomXAlgorithm {
  pub fn with_order(
    config: &RandomXConfig,
    pool: Arc<dyn WorkProvider<Work = StratumJob>>,
    hashers: &HasherOrder,
  ) -> Result<RandomXAlgorithm> {
    use verification::{HasherKind, RandomXHasher, RemoteHasher};
    let mut ordered: Vec<Arc<dyn RandomXHasher>> = Vec::new();
    let mut local_hasher = None;
    for kind in &hashers.order {
      match kind {
        HasherKind::Local => {
          let local = Arc::new(LocalHasher::new(
            config.queue_capacity,
            config.cache_idle_timeout,
          ));
          local_hasher = Some(local.clone());
          ordered.push(local);
        }
        HasherKind::Remote => {
          let remote = hashers
            .remote
            .as_ref()
            .context("remote hasher configuration is missing")?;
          ordered.push(Arc::new(RemoteHasher::new(
            &remote.endpoint,
            &remote.server_name,
            remote.timeout,
            remote.ca.as_deref(),
            remote.client_cert.as_deref(),
            remote.client_key.as_deref(),
          )?));
        }
      }
    }
    let mut algorithm = RandomXAlgorithm::with_hasher(
      config,
      pool,
      OrderedHasher::new(ordered).with_deadline_policy(hashers.deadline_policy),
    );
    algorithm.local_hasher = local_hasher;
    Ok(algorithm)
  }

  pub fn with_hasher(
    config: &RandomXConfig,
    provider: Arc<dyn WorkProvider<Work = StratumJob>>,
    hasher: OrderedHasher,
  ) -> Self {
    Self {
      difficulty: config.difficulty.clone(),
      ttl: config.ttl,
      nonce_offset: config.nonce_offset,
      solver_module_path: config.solver_module_path.clone(),
      browser: rilgate_pow::algorithm::BrowserSolverOptions {
        max_workers: config.max_workers,
        store_cache: config.store_cache,
        job_details: config.job_details,
      },
      hasher,
      local_hasher: None,
      provider,
    }
  }
}
impl Algorithm for RandomXAlgorithm {
  fn hash_diagnostics(&self) -> Option<rilgate_pow::algorithm::HashDiagnostics> {
    Some(self.hasher.diagnostics())
  }

  fn remote_ready(&self, timeout: Duration) -> AlgorithmFuture<'_, bool> {
    Box::pin(async move { Ok(self.hasher.remote_ready(timeout).await) })
  }

  fn verifier_diagnostics(&self) -> Option<rilgate_pow::algorithm::VerifierDiagnostics> {
    Some(
      self
        .local_hasher
        .as_ref()
        .map_or_else(Default::default, |local| local.diagnostics()),
    )
  }

  fn descriptor(&self) -> AlgorithmDescriptor {
    AlgorithmDescriptor {
      id: PowAlgorithm::RandomX,
      version: 1,
      display_name: "RandomX",
      solver_module_path: self.solver_module_path.clone(),
      requires_consent: true,
      memory_bytes: 256 * 1024 * 1024,
      browser: Some(self.browser.clone()),
    }
  }
  fn difficulty_for_weight(&self, weight: i32) -> u64 {
    if weight <= 0 {
      return self.difficulty.base.max(1000);
    }
    let exponent = (weight / self.difficulty.weight_step.max(1)).clamp(0, 63) as u32;
    self
      .difficulty
      .base
      .saturating_mul(2_u64.saturating_pow(exponent))
      .min(self.difficulty.max)
      .max(1000)
  }
  fn validate_difficulty(&self, difficulty: u64) -> Result<()> {
    anyhow::ensure!(
      difficulty >= 1000,
      "RandomX pool difficulty must be at least 1000"
    );
    Ok(())
  }
  fn validate_verification(&self, verification: PowVerification) -> Result<()> {
    anyhow::ensure!(
      verification == PowVerification::Gateway,
      "RandomX HTTP challenge requires pow.algorithms.randomx.verify.mode: gateway"
    );
    Ok(())
  }
  fn challenge_ttl(&self) -> u64 {
    self.ttl
  }
  fn issue(&self, request: WorkRequest) -> AlgorithmFuture<'_, PreparedWork> {
    Box::pin(async move {
      self.validate_verification(request.verification)?;
      self.validate_difficulty(request.difficulty)?;
      let mut job = self
        .provider
        .fetch(request.difficulty)
        .await
        .map_err(|error| {
          if error.is::<rilgate_pool::provider::AllPoolsUnavailable>() {
            anyhow::Error::new(rilgate_pow::WorkUnavailable)
          } else {
            error
          }
        })?;
      if self
        .nonce_offset
        .checked_add(4)
        .is_none_or(|end| end > job.blob.len() / 2)
      {
        return Err(PowBackendUnavailable.into());
      }
      let id = Uuid::new_v4();
      // Local work is unique while the seed cache remains shared.
      job.blob = work::bind_randomx_work(&job.blob, &id).map_err(|error| {
        tracing::warn!(?error, "Stratum pool challenge unavailable");
        PowBackendUnavailable
      })?;
      let now = unix_now()?;
      Ok(PreparedWork {
        challenge_id: id.to_string(),
        issued_at: now,
        expires_at: now + self.ttl as i64,
        work: Work::Randomx {
          job,
          difficulty: request.difficulty,
          nonce_offset: self.nonce_offset,
        },
      })
    })
  }
  fn verify<'a>(&'a self, work: &'a PreparedWork, proof: Proof<'a>) -> AlgorithmFuture<'a, ()> {
    Box::pin(async move {
      let Work::Randomx {
        job,
        difficulty,
        nonce_offset,
      } = &work.work
      else {
        return Err(anyhow!("Expected RandomX work"));
      };
      self.validate_difficulty(*difficulty)?;
      let seed = hex::decode(&job.seed_hash).context("Invalid RandomX seed hash")?;
      let mut blob = hex::decode(&job.blob).context("Invalid RandomX hashing blob")?;
      anyhow::ensure!(
        seed.len() == 32 && !blob.is_empty() && blob.len() <= 256,
        "Invalid RandomX work dimensions"
      );
      write_nonce(&mut blob, *nonce_offset, proof.nonce)?;
      anyhow::ensure!(work.expires_at > 0, "Challenge expired");
      let deadline = UNIX_EPOCH
        .checked_add(Duration::from_secs(work.expires_at as u64))
        .context("Invalid challenge expiry")?;
      let started = Instant::now();
      let remaining = deadline
        .duration_since(SystemTime::now())
        .context("Challenge expired")?;
      let deadline = started + remaining;
      let hash = tokio::time::timeout_at(deadline.into(), self.hasher.hash(seed, blob, deadline))
        .await
        .map_err(|_| anyhow!("Challenge expired"))?;
      // Hashers can complete without yielding to the expiry timer.
      anyhow::ensure!(Instant::now() < deadline, "Challenge expired");
      let hash = hash.map_err(|error| match error {
        HashError::Unavailable(error) if error.is::<rilgate_core::error::CapacityExceeded>() => {
          error
        }
        HashError::Unavailable(error) => error.context(PowBackendUnavailable),
        HashError::Invalid(error) => error,
      })?;
      anyhow::ensure!(
        hash_meets_difficulty(&hash, *difficulty),
        "RandomX proof does not meet the required difficulty"
      );
      Ok(())
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  struct Provider;
  impl WorkProvider for Provider {
    type Work = StratumJob;
    fn fetch(&self, _: u64) -> rilgate_core::BoxFuture<'_, StratumJob> {
      Box::pin(async { anyhow::bail!("No work available") })
    }
  }

  #[test]
  fn ordered_constructor_builds_only_selected_hashers() {
    use verification::HasherKind;
    let config = RandomXConfig {
      difficulty: DifficultyConfig {
        base: 1000,
        weight_step: 10,
        max: 32000,
      },
      ttl: 60,
      solver_module_path: "/static/js/solvers/randomx/solver.js".into(),
      max_workers: 8,
      store_cache: true,
      job_details: true,
      nonce_offset: 39,
      queue_capacity: 8,
      cache_idle_timeout: Duration::from_secs(60),
    };
    for order in [
      vec![HasherKind::Local],
      vec![HasherKind::Remote],
      vec![HasherKind::Remote, HasherKind::Local],
      vec![HasherKind::Local, HasherKind::Remote],
    ] {
      let hashers = HasherOrder {
        order,
        remote: Some(RemoteHasherConfig {
          endpoint: "https://localhost:9443".into(),
          server_name: "localhost".into(),
          timeout: Duration::from_secs(2),
          ca: None,
          client_cert: None,
          client_key: None,
        }),
        deadline_policy: Default::default(),
      };
      let algorithm = RandomXAlgorithm::with_order(&config, Arc::new(Provider), &hashers).unwrap();
      assert_eq!(
        algorithm.local_hasher.is_some(),
        hashers.order.contains(&HasherKind::Local)
      );
    }
  }
}
