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
