use anyhow::{Context, Result, ensure};
use hashwx_core::HashWx;
use sha2::{Digest, Sha256};

use crate::config;

/// A HashWX proof with validated work parameters, ready for the hash check.
pub(crate) struct ParsedProof {
  seed: [u8; 32],
  nonce: u64,
  target: u64,
}

impl ParsedProof {
  pub(crate) fn parse(
    challenge: &str,
    nonce: &str,
    difficulty: u64,
    nonces_per_seed: u32,
  ) -> Result<Self> {
    config::validate_difficulty(difficulty)?;
    config::validate_nonces_per_seed(nonces_per_seed)?;
    let challenge = challenge_bytes(challenge)?;
    let nonce = parse_nonce(nonce)?;
    Ok(Self {
      seed: seed(&challenge, nonce / u64::from(nonces_per_seed)),
      nonce,
      target: u64::MAX / difficulty,
    })
  }

  /// Runs the CPU-heavy hash and compares it with the difficulty target.
  pub(crate) fn check(&self) -> Result<()> {
    ensure!(
      HashWx::new(&self.seed).hash(self.nonce) < self.target,
      "HashWX proof does not meet the required difficulty"
    );
    Ok(())
  }
}

pub fn seed(challenge: &[u8; 32], batch: u64) -> [u8; 32] {
  let mut hash = Sha256::new();
  hash.update(challenge);
  hash.update(batch.to_le_bytes());
  hash.finalize().into()
}

fn challenge_bytes(challenge: &str) -> Result<[u8; 32]> {
  ensure!(
    challenge.len() == 64
      && challenge
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
    "HashWX challenge must be 32 bytes in lower-case hex"
  );
  let mut bytes = [0; 32];
  hex::decode_to_slice(challenge, &mut bytes).context("Invalid HashWX challenge")?;
  Ok(bytes)
}

fn parse_nonce(value: &str) -> Result<u64> {
  ensure!(
    !value.is_empty() && value.len() <= 20,
    "HashWX nonce must be a decimal u64"
  );
  let nonce = value
    .parse::<u64>()
    .context("HashWX nonce must be a decimal u64")?;
  ensure!(
    nonce.to_string() == value,
    "HashWX nonce must use canonical decimal encoding"
  );
  Ok(nonce)
}
