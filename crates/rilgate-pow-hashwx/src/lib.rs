mod config;
pub mod proof;

use anyhow::{Context, Result, anyhow, ensure};
pub use config::{HashWxConfig, MAX_DIFFICULTY};
use hashwx_core::HashWx;
use rilgate_core::time::unix_now;
use rilgate_pow::{
  Algorithm, AlgorithmDescriptor, AlgorithmFuture, PowAlgorithm, PreparedWork, Proof, Work,
  WorkRequest,
};
use rilgate_protocol::PowEnforcement;

pub struct HashWxAlgorithm {
  config: HashWxConfig,
}

impl HashWxAlgorithm {
  pub fn new(config: &HashWxConfig) -> Result<Self> {
    config.validate()?;
    Ok(Self {
      config: config.clone(),
    })
  }
}

impl Algorithm for HashWxAlgorithm {
  fn descriptor(&self) -> AlgorithmDescriptor {
    AlgorithmDescriptor {
      id: PowAlgorithm::Hashwx,
      version: 1,
      display_name: "HashWX",
      solver_module_path: "/static/js/solvers/hashwx/solver.js",
      requires_consent: false,
      memory_bytes: 16392,
    }
  }

  fn difficulty_for_weight(&self, weight: i32) -> u64 {
    let exponent = (weight.max(0) / self.config.difficulty.weight_step).min(63) as u32;
    self
      .config
      .difficulty
      .base
      .saturating_mul(2_u64.saturating_pow(exponent))
      .min(self.config.difficulty.max)
  }

  fn validate_difficulty(&self, difficulty: u64) -> Result<()> {
    ensure!(
      (1..=MAX_DIFFICULTY).contains(&difficulty),
      "HashWX difficulty must be between 1 and 2^40"
    );
    Ok(())
  }

  fn validate_enforcement(&self, enforce: PowEnforcement) -> Result<()> {
    ensure!(
      enforce == PowEnforcement::LocalVerify,
      "HashWX requires local_verify enforcement"
    );
    Ok(())
  }

  fn challenge_ttl(&self) -> u64 {
    self.config.ttl
  }

  fn issue(&self, request: WorkRequest) -> AlgorithmFuture<'_, PreparedWork> {
    Box::pin(async move {
      self.validate_enforcement(request.enforce)?;
      self.validate_difficulty(request.difficulty)?;
      let mut challenge = [0; 32];
      getrandom::fill(&mut challenge)
        .map_err(|error| anyhow!("HashWX random source failed: {error}"))?;
      let now = unix_now()?;
      Ok(PreparedWork {
        challenge_id: uuid::Uuid::new_v4().to_string(),
        issued_at: now,
        expires_at: now + self.config.ttl as i64,
        work: Work::Hashwx {
          challenge: hex::encode(challenge),
          difficulty: request.difficulty,
          nonces_per_seed: self.config.nonces_per_seed,
        },
      })
    })
  }

  fn verify<'a>(&'a self, work: &'a PreparedWork, proof: Proof<'a>) -> AlgorithmFuture<'a, ()> {
    Box::pin(async move {
      let Work::Hashwx {
        challenge,
        difficulty,
        nonces_per_seed,
      } = &work.work
      else {
        return Err(anyhow!("Expected HashWX work"));
      };
      self.validate_difficulty(*difficulty)?;
      config::validate_nonces_per_seed(*nonces_per_seed)?;
      let challenge = proof::challenge_bytes(challenge)?;
      let nonce = proof::nonce(proof.nonce)?;
      let seed = proof::seed(&challenge, nonce / u64::from(*nonces_per_seed));
      let target = u64::MAX / difficulty;
      let expires_at = work.expires_at;
      ensure!(unix_now()? < expires_at, "HashWX challenge expired");
      tokio::task::spawn_blocking(move || -> Result<()> {
        ensure!(unix_now()? < expires_at, "HashWX challenge expired");
        let hash = HashWx::new(&seed).hash(nonce);
        ensure!(
          hash < target,
          "HashWX proof does not meet the required difficulty"
        );
        ensure!(unix_now()? < expires_at, "HashWX challenge expired");
        Ok(())
      })
      .await
      .context("HashWX verification task failed")?
    })
  }
}
