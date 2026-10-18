pub mod verification;
mod work;
use anyhow::{Context, Result, anyhow};
use rilgate_core::{error::PowBackendUnavailable, time::unix_now};
use rilgate_pool::WorkProvider;
use rilgate_pow::{
  Algorithm, AlgorithmDescriptor, AlgorithmFuture, PowAlgorithm, PreparedWork, Proof, Work,
  WorkRequest, difficulty::DifficultyConfig,
};
use rilgate_protocol::{PowEnforcement, StratumJob};
use std::{
  sync::{Arc, Mutex},
  time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;
use verification::{RandomXVerifier, hash_meets_difficulty, write_nonce};
pub struct RandomXAlgorithm {
  difficulty: DifficultyConfig,
  ttl: u64,
  nonce_offset: usize,
  queue_capacity: usize,
  verifier: Mutex<Option<RandomXVerifier>>,
  provider: Arc<dyn WorkProvider<Work = StratumJob>>,
}
#[derive(Debug, Clone)]
pub struct RandomXConfig {
  pub difficulty: DifficultyConfig,
  pub ttl: u64,
  pub nonce_offset: usize,
  pub queue_capacity: usize,
}
impl RandomXAlgorithm {
  pub fn new(config: &RandomXConfig, provider: Arc<dyn WorkProvider<Work = StratumJob>>) -> Self {
    Self {
      difficulty: config.difficulty.clone(),
      ttl: config.ttl,
      nonce_offset: config.nonce_offset,
      queue_capacity: config.queue_capacity,
      verifier: Mutex::new(None),
      provider,
    }
  }
  fn verifier(&self) -> Result<RandomXVerifier> {
    let mut verifier = self
      .verifier
      .lock()
      .unwrap_or_else(|error| error.into_inner());
    if verifier.is_none() {
      *verifier = Some(RandomXVerifier::with_capacity(self.queue_capacity)?);
    }
    Ok(
      verifier
        .as_ref()
        .expect("RandomX verifier initialized")
        .clone(),
    )
  }
}
impl Algorithm for RandomXAlgorithm {
  fn verifier_diagnostics(&self) -> Option<rilgate_pow::algorithm::VerifierDiagnostics> {
    let verifier = self
      .verifier
      .lock()
      .unwrap_or_else(|error| error.into_inner());
    Some(
      verifier
        .as_ref()
        .map_or_else(Default::default, RandomXVerifier::diagnostics),
    )
  }

  fn descriptor(&self) -> AlgorithmDescriptor {
    AlgorithmDescriptor {
      id: PowAlgorithm::RandomX,
      version: 1,
      display_name: "RandomX",
      solver_module_path: "/static/js/solvers/randomx/solver.js",
      requires_consent: true,
      memory_bytes: 256 * 1024 * 1024,
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
      "RandomX/P2Pool difficulty must be at least 1000"
    );
    Ok(())
  }
  fn validate_enforcement(&self, enforce: PowEnforcement) -> Result<()> {
    anyhow::ensure!(
      enforce == PowEnforcement::LocalVerify,
      "RandomX HTTP challenge is disabled in p2pool_accepted mode"
    );
    Ok(())
  }
  fn challenge_ttl(&self) -> u64 {
    self.ttl
  }
  fn issue(&self, request: WorkRequest) -> AlgorithmFuture<'_, PreparedWork> {
    Box::pin(async move {
      self.validate_enforcement(request.enforce)?;
      self.validate_difficulty(request.difficulty)?;
      let mut job = self.provider.fetch(request.difficulty).await?;
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
        tracing::warn!(?error, "P2Pool challenge unavailable");
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
      let remaining = deadline
        .duration_since(SystemTime::now())
        .context("Challenge expired")?;
      let hash = tokio::time::timeout(remaining, self.verifier()?.hash(seed, blob))
        .await
        .map_err(|_| anyhow!("Challenge expired"))??;
      anyhow::ensure!(
        hash_meets_difficulty(&hash, *difficulty),
        "RandomX proof does not meet the required difficulty"
      );
      Ok(())
    })
  }
}
