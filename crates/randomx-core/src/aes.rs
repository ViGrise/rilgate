use aes::hazmat::Block;
#[cfg(not(target_arch = "wasm32"))]
use aes::hazmat::{cipher_round, equiv_inv_cipher_round};
#[cfg(any(target_arch = "wasm32", test))]
mod lookup;
#[cfg(target_arch = "wasm32")]
use lookup::{cipher_round, equiv_inv_cipher_round};

/// Builds a 16-byte AES block from the reference `AES_*` macro arguments.
///
/// The reference expands those macros into
/// `rx_set_int_vec_i128(k0, k1, k2, k3)`, whose first parameter is the highest
/// word, so the block ends up holding them in reverse order.
fn block(words: [u32; 4]) -> Block {
  let [first, second, third, fourth] = words;

  let mut bytes = [0u8; 16];
  for (index, word) in [fourth, third, second, first].into_iter().enumerate() {
    bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
  }

  Block::from(bytes)
}

fn bytes_block(bytes: &[u8]) -> Block {
  let array: [u8; 16] = bytes.try_into().expect("16 byte AES block");
  Block::from(array)
}

fn copy_block(block: &Block, out: &mut [u8]) {
  out.copy_from_slice(block.as_ref());
}

pub fn fill_aes_1r(seed: &mut [u8; 64], out: &mut [u8]) {
  assert_eq!(out.len() % 64, 0);

  let keys = [
    block([0xb4f44917, 0xdbb5552b, 0x62716609, 0x6daca553]),
    block([0x0da1dc4e, 0x1725d378, 0x846a710d, 0x6d7caf07]),
    block([0x3e20e345, 0xf4c0794f, 0x9f947ec6, 0x3f1262f1]),
    block([0x49169154, 0x16314c88, 0xb1ba317c, 0x6aef8135]),
  ];

  let mut state = [
    bytes_block(&seed[0..16]),
    bytes_block(&seed[16..32]),
    bytes_block(&seed[32..48]),
    bytes_block(&seed[48..64]),
  ];

  for chunk in out.as_chunks_mut::<64>().0 {
    equiv_inv_cipher_round(&mut state[0], &keys[0]);
    cipher_round(&mut state[1], &keys[1]);
    equiv_inv_cipher_round(&mut state[2], &keys[2]);
    cipher_round(&mut state[3], &keys[3]);

    copy_block(&state[0], &mut chunk[0..16]);
    copy_block(&state[1], &mut chunk[16..32]);
    copy_block(&state[2], &mut chunk[32..48]);
    copy_block(&state[3], &mut chunk[48..64]);
  }

  copy_block(&state[0], &mut seed[0..16]);
  copy_block(&state[1], &mut seed[16..32]);
  copy_block(&state[2], &mut seed[32..48]);
  copy_block(&state[3], &mut seed[48..64]);
}

pub fn fill_aes_4r(seed: &[u8; 64], out: &mut [u8]) {
  assert_eq!(out.len() % 64, 0);

  let keys = [
    block([0x99e5d23f, 0x2f546d2b, 0xd1833ddb, 0x6421aadd]),
    block([0xa5dfcde5, 0x06f79d53, 0xb6913f55, 0xb20e3450]),
    block([0x171c02bf, 0x0aa4679f, 0x515e7baf, 0x5c3ed904]),
    block([0xd8ded291, 0xcd673785, 0xe78f5d08, 0x85623763]),
    block([0x229effb4, 0x3d518b6d, 0xe3d6a7a6, 0xb5826f73]),
    block([0xb272b7d2, 0xe9024d4e, 0x9c10b3d9, 0xc7566bf3]),
    block([0xf63befa7, 0x2ba9660a, 0xf765a38b, 0xf273c9e7]),
    block([0xc0b0762d, 0x0c06d1fd, 0x915839de, 0x7a7cd609]),
  ];

  let mut state = [
    bytes_block(&seed[0..16]),
    bytes_block(&seed[16..32]),
    bytes_block(&seed[32..48]),
    bytes_block(&seed[48..64]),
  ];

  for chunk in out.as_chunks_mut::<64>().0 {
    for round in 0..4 {
      equiv_inv_cipher_round(&mut state[0], &keys[round]);
      cipher_round(&mut state[1], &keys[round]);
      equiv_inv_cipher_round(&mut state[2], &keys[round + 4]);
      cipher_round(&mut state[3], &keys[round + 4]);
    }

    copy_block(&state[0], &mut chunk[0..16]);
    copy_block(&state[1], &mut chunk[16..32]);
    copy_block(&state[2], &mut chunk[32..48]);
    copy_block(&state[3], &mut chunk[48..64]);
  }
}

pub fn hash_aes_1r(input: &[u8]) -> [u8; 64] {
  assert_eq!(input.len() % 64, 0);

  let mut state = [
    block([0xd7983aad, 0xcc82db47, 0x9fa856de, 0x92b52c0d]),
    block([0xace78057, 0xf59e125a, 0x15c7b798, 0x338d996e]),
    block([0xe8a07ce4, 0x5079506b, 0xae62c7d0, 0x6a770017]),
    block([0x7e994948, 0x79a10005, 0x07ad828d, 0x630a240c]),
  ];

  for chunk in input.as_chunks::<64>().0 {
    let keys = [
      bytes_block(&chunk[0..16]),
      bytes_block(&chunk[16..32]),
      bytes_block(&chunk[32..48]),
      bytes_block(&chunk[48..64]),
    ];

    cipher_round(&mut state[0], &keys[0]);
    equiv_inv_cipher_round(&mut state[1], &keys[1]);
    cipher_round(&mut state[2], &keys[2]);
    equiv_inv_cipher_round(&mut state[3], &keys[3]);
  }

  let xkeys = [
    block([0x06890201, 0x90dc56bf, 0x8b24949f, 0xf6fa8389]),
    block([0xed18f99b, 0xee1043c6, 0x51f4e03c, 0x61b263d1]),
  ];

  for key in &xkeys {
    cipher_round(&mut state[0], key);
    equiv_inv_cipher_round(&mut state[1], key);
    cipher_round(&mut state[2], key);
    equiv_inv_cipher_round(&mut state[3], key);
  }

  let mut out = [0u8; 64];
  copy_block(&state[0], &mut out[0..16]);
  copy_block(&state[1], &mut out[16..32]);
  copy_block(&state[2], &mut out[32..48]);
  copy_block(&state[3], &mut out[48..64]);
  out
}
