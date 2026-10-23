use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};

pub fn seed(challenge: &[u8; 32], batch: u64) -> [u8; 32] {
  let mut hash = Sha256::new();
  hash.update(challenge);
  hash.update(batch.to_le_bytes());
  hash.finalize().into()
}

pub(crate) fn challenge_bytes(challenge: &str) -> Result<[u8; 32]> {
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

pub(crate) fn nonce(value: &str) -> Result<u64> {
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
