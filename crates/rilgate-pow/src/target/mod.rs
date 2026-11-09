use std::cmp::Ordering;
/// Reports whether the 256-bit PoW result satisfies `difficulty`.
///
/// Monero compares the result as a little-endian integer against
/// `(2^256 - 1) / difficulty`.
pub fn hash_meets_difficulty(hash: &[u8], difficulty: u64) -> bool {
  if hash.len() != 32 || difficulty == 0 {
    return false;
  }

  let target = divide_256([u64::MAX; 4], difficulty);
  let value = little_endian_words(hash);

  for limb in (0..4).rev() {
    match value[limb].cmp(&target[limb]) {
      Ordering::Less => return true,
      Ordering::Greater => return false,
      Ordering::Equal => {}
    }
  }

  true
}

/// Share threshold of a Monero Stratum job target, in the 64-bit form that miners compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolTarget(u64);

impl PoolTarget {
  pub fn from_hex(target: &str) -> anyhow::Result<Self> {
    let invalid = || anyhow::anyhow!("Stratum pool target must be 4 or 8 hexadecimal bytes");
    let bytes = hex::decode(target).map_err(|_| invalid())?;
    let value = match bytes.len() {
      4 => u64::from(u32::from_le_bytes(bytes[..].try_into()?)),
      8 => u64::from_le_bytes(bytes[..].try_into()?),
      _ => return Err(invalid()),
    };
    anyhow::ensure!(value != 0, "Stratum pool target must be nonzero");
    // Compact targets expand like XMRig, which every pool must accept.
    let threshold = if bytes.len() == 4 {
      u64::MAX / (u64::from(u32::MAX) / value)
    } else {
      value
    };
    Ok(Self(threshold))
  }

  pub fn threshold(self) -> u64 {
    self.0
  }
}

/// Reports whether the high 64 bits of a 256-bit result are below the pool threshold.
pub fn hash_meets_pool_target(hash: &[u8], target: PoolTarget) -> bool {
  hash.len() == 32
    && u64::from_le_bytes(hash[24..32].try_into().expect("8 byte hash word")) < target.threshold()
}

fn little_endian_words(bytes: &[u8]) -> [u64; 4] {
  let mut words = [0u64; 4];

  for (limb, chunk) in words.iter_mut().zip(bytes.as_chunks::<8>().0) {
    *limb = u64::from_le_bytes(*chunk);
  }

  words
}

/// Divides a 256-bit little-endian value by a 64-bit divisor.
fn divide_256(value: [u64; 4], divisor: u64) -> [u64; 4] {
  let divisor = u128::from(divisor);
  let mut quotient = [0u64; 4];
  let mut remainder = 0u128;

  for bit in (0..256).rev() {
    let next = (remainder << 1) | u128::from((value[bit / 64] >> (bit % 64)) & 1);

    if next >= divisor {
      remainder = next - divisor;
      quotient[bit / 64] |= 1u64 << (bit % 64);
    } else {
      remainder = next;
    }
  }

  quotient
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn pool_targets_decode_compact_and_full_thresholds() {
    for (hex, expected) in [
      ("ffffffff", 18446744073709551615),
      ("b88d0600", 1844674407370955),
      ("c5a70000", 184465596081134),
      ("0000000000010000", 1099511627776),
    ] {
      assert_eq!(PoolTarget::from_hex(hex).unwrap().threshold(), expected);
    }
    for hex in ["00000000", "0000000000000000", "ffff", "zz"] {
      assert!(PoolTarget::from_hex(hex).is_err());
    }
  }

  #[test]
  fn pool_threshold_is_exclusive() {
    let target = PoolTarget::from_hex("b88d0600").unwrap();
    let mut hash = [0; 32];
    hash[24..].copy_from_slice(&(target.threshold() - 1).to_le_bytes());
    assert!(hash_meets_pool_target(&hash, target));
    hash[24..].copy_from_slice(&target.threshold().to_le_bytes());
    assert!(!hash_meets_pool_target(&hash, target));
  }

  #[test]
  fn zero_hash_meets_any_nonzero_difficulty() {
    assert!(hash_meets_difficulty(&[0_u8; 32], 1_000_000));
  }

  #[test]
  fn unit_difficulty_accepts_every_result() {
    assert!(hash_meets_difficulty(&[0xff_u8; 32], 1));
    assert!(!hash_meets_difficulty(&[0_u8; 32], 0));
  }

  #[test]
  fn target_is_maximum_divided_by_difficulty() {
    // Difficulty 2 puts the boundary exactly on the most significant bit.
    let target = divide_256([u64::MAX; 4], 2);
    assert_eq!(target, [u64::MAX, u64::MAX, u64::MAX, u64::MAX >> 1]);

    let mut at_boundary = [0xff_u8; 32];
    at_boundary[31] = 0x7f;
    assert!(hash_meets_difficulty(&at_boundary, 2));

    let above_boundary = [0xff_u8; 32];
    assert!(!hash_meets_difficulty(&above_boundary, 2));
  }
}
