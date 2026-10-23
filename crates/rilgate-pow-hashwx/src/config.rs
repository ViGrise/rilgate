use anyhow::{Result, ensure};
use rilgate_pow::difficulty::DifficultyConfig;

pub const MAX_DIFFICULTY: u64 = 1 << 40;

#[derive(Debug, Clone)]
pub struct HashWxConfig {
  pub nonces_per_seed: u32,
  pub difficulty: DifficultyConfig,
  pub ttl: u64,
}

impl HashWxConfig {
  pub fn validate(&self) -> Result<()> {
    validate_nonces_per_seed(self.nonces_per_seed)?;
    ensure!(
      (1..=MAX_DIFFICULTY).contains(&self.difficulty.base)
        && (self.difficulty.base..=MAX_DIFFICULTY).contains(&self.difficulty.max)
        && self.difficulty.weight_step > 0,
      "difficulty requires 1 <= base <= max <= 2^40 and a positive score_step"
    );
    Ok(())
  }
}

pub(crate) fn validate_nonces_per_seed(value: u32) -> Result<()> {
  ensure!(
    (463..=1_048_576).contains(&value),
    "nonces_per_seed must be between 463 and 1048576"
  );
  Ok(())
}
