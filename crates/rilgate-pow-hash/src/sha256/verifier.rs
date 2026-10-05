use anyhow::{Result, anyhow};
use sha2::{Digest, Sha256};

pub fn verify_fast(challenge: &str, nonce: &str, difficulty: u64) -> Result<bool> {
  if difficulty == 0 || difficulty > 64 {
    return Err(anyhow!(
      "fast difficulty must be between 1 and 64 leading hexadecimal zeroes"
    ));
  }

  let parsed = nonce
    .parse::<u64>()
    .map_err(|_| anyhow!("fast nonce must be an unsigned decimal integer"))?;

  if parsed.to_string() != nonce {
    return Err(anyhow!("fast nonce must use canonical decimal encoding"));
  }

  let mut hasher = Sha256::new();
  hasher.update(challenge.as_bytes());
  hasher.update(nonce.as_bytes());
  let hash = hasher.finalize();

  Ok(has_leading_zero_nibbles(&hash, difficulty as usize))
}

fn has_leading_zero_nibbles(hash: &[u8], difficulty: usize) -> bool {
  let full_bytes = difficulty / 2;
  let half_nibble = difficulty % 2 == 1;

  if full_bytes > hash.len() {
    return false;
  }

  if hash[..full_bytes].iter().any(|byte| *byte != 0) {
    return false;
  }

  if half_nibble {
    let Some(byte) = hash.get(full_bytes) else {
      return false;
    };

    if byte & 0xF0 != 0 {
      return false;
    }
  }

  true
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn checks_nibbles() {
    assert!(has_leading_zero_nibbles(&[0x00, 0x0f], 3));
    assert!(!has_leading_zero_nibbles(&[0x00, 0x1f], 3));
    assert!(has_leading_zero_nibbles(&[0x00, 0xff], 2));
  }

  #[test]
  fn verifies_known_fast_solution() {
    // SHA-256("test-challenge153") starts with 00.
    assert!(verify_fast("test-challenge", "153", 2).unwrap());
    assert!(!verify_fast("test-challenge", "153", 3).unwrap());
  }
}
