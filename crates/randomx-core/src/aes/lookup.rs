//! AES round tables for RandomX's public hashing state.

use aes::hazmat::Block;

const fn xtime(x: u8) -> u8 {
  (x << 1) ^ if x & 128 != 0 { 0x1b } else { 0 }
}
const fn multiply(mut a: u8, mut b: u8) -> u8 {
  let mut out = 0;
  while b != 0 {
    if b & 1 != 0 {
      out ^= a;
    }
    a = xtime(a);
    b >>= 1;
  }
  out
}
const fn sbox(x: u8) -> u8 {
  let mut inverse = 1;
  let mut base = x;
  let mut exponent = 254;
  while exponent != 0 {
    if exponent & 1 != 0 {
      inverse = multiply(inverse, base);
    }
    base = multiply(base, base);
    exponent >>= 1;
  }
  inverse
    ^ inverse.rotate_left(1)
    ^ inverse.rotate_left(2)
    ^ inverse.rotate_left(3)
    ^ inverse.rotate_left(4)
    ^ 0x63
}
const fn tables(inverse: bool) -> [[u32; 256]; 4] {
  let mut substitution = [0u8; 256];
  let mut i = 0;
  while i < 256 {
    let s = sbox(i as u8);
    if inverse {
      substitution[s as usize] = i as u8;
    } else {
      substitution[i] = s;
    }
    i += 1;
  }
  let mut out = [[0; 256]; 4];
  i = 0;
  while i < 256 {
    let s = substitution[i];
    let word = if inverse {
      u32::from_le_bytes([
        multiply(s, 14),
        multiply(s, 9),
        multiply(s, 13),
        multiply(s, 11),
      ])
    } else {
      u32::from_le_bytes([xtime(s), s, s, xtime(s) ^ s])
    };
    let mut j = 0;
    while j < 4 {
      out[j][i] = word.rotate_left(j as u32 * 8);
      j += 1;
    }
    i += 1;
  }
  out
}
static ENCRYPT: [[u32; 256]; 4] = tables(false);
static DECRYPT: [[u32; 256]; 4] = tables(true);

#[inline]
fn round(state: &mut Block, key: &Block, inverse: bool) {
  // Lookup indices depend only on public PoW state.
  let bytes: &[u8] = state.as_ref();
  let key: &[u8] = key.as_ref();
  let table = if inverse { &DECRYPT } else { &ENCRYPT };
  let mut out = [0; 16];
  for column in 0..4 {
    let step = if inverse { 3 } else { 1 };
    let word = table[0][bytes[column * 4] as usize]
      ^ table[1][bytes[((column + step) & 3) * 4 + 1] as usize]
      ^ table[2][bytes[((column + 2) & 3) * 4 + 2] as usize]
      ^ table[3][bytes[((column + 3 * step) & 3) * 4 + 3] as usize]
      ^ u32::from_le_bytes(key[column * 4..column * 4 + 4].try_into().unwrap());
    out[column * 4..column * 4 + 4].copy_from_slice(&word.to_le_bytes());
  }
  *state = Block::from(out);
}
#[inline]
pub(super) fn cipher_round(state: &mut Block, key: &Block) {
  round(state, key, false);
}
#[inline]
pub(super) fn equiv_inv_cipher_round(state: &mut Block, key: &Block) {
  round(state, key, true);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn table_rounds_match_portable_aes_for_public_states() {
    let mut state = 0x19a8_3359_ef2c_9861_u64;
    for _ in 0..4096 {
      let mut bytes = [0; 32];
      for chunk in bytes.as_chunks_mut::<8>().0 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        chunk.copy_from_slice(&state.to_le_bytes());
      }
      let original = Block::from(<[u8; 16]>::try_from(&bytes[..16]).unwrap());
      let key = Block::from(<[u8; 16]>::try_from(&bytes[16..]).unwrap());
      let mut actual = original;
      let mut expected = original;
      cipher_round(&mut actual, &key);
      aes::hazmat::cipher_round(&mut expected, &key);
      assert_eq!(actual, expected);
      actual = original;
      expected = original;
      equiv_inv_cipher_round(&mut actual, &key);
      aes::hazmat::equiv_inv_cipher_round(&mut expected, &key);
      assert_eq!(actual, expected);
    }
  }
}
