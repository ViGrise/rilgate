//! Reference vectors from the official RandomX test suite
//! (`src/tests/tests.cpp`).
//!
//! The Argon2d-backed tests allocate a 256 MiB cache and run the portable
//! interpreter, so they are ignored by default:
//!
//! ```text
//! cargo test-randomx
//! ```

use blake2::{Blake2b256, Digest};
use randomx_core::{
  RandomX, aes::fill_aes_1r, blake_generator::Blake2Generator, constants::DATASET_ITEM_SIZE,
  reciprocal::reciprocal, superscalar::generate_superscalar,
};

const KEY_000: &[u8] = b"test key 000";
const KEY_001: &[u8] = b"test key 001";

fn decode_hex(text: &str) -> Vec<u8> {
  text
    .as_bytes()
    .as_chunks::<2>()
    .0
    .iter()
    .map(|pair| {
      let digit = |c: u8| match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => panic!("invalid hex digit"),
      };
      (digit(pair[0]) << 4) | digit(pair[1])
    })
    .collect()
}

fn encode_hex(bytes: &[u8]) -> String {
  bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn aes_generator_1r() {
  let mut state = decode_hex("6c19536eb2de31b6c0065f7f116e86f960d8af0c57210a6584c3237b9d064dc7");
  let mut seed = [0u8; 64];
  seed[..32].copy_from_slice(&state);

  let mut buffer = [0u8; 64];
  fill_aes_1r(&mut seed, &mut buffer);
  state[..32].copy_from_slice(&buffer[..32]);

  assert_eq!(
    encode_hex(&state),
    "fa89397dd6ca422513aeadba3f124b5540324c4ad4b6db434394307a17c833ab"
  );
}

#[test]
fn randomx_reciprocal_values() {
  assert_eq!(reciprocal(3), 12297829382473034410);
  assert_eq!(reciprocal(13), 11351842506898185609);
  assert_eq!(reciprocal(33), 17887751829051686415);
  assert_eq!(reciprocal(65537), 18446462603027742720);
  assert_eq!(reciprocal(15000001), 10316166306300415204);
  assert_eq!(reciprocal(3845182035), 10302264209224146340);
  assert_eq!(reciprocal(0xffff_ffff), 9223372039002259456);
}

#[test]
fn superscalar_generator() {
  let references = [
    "d3a4a6623738756f77e6104469102f082eff2a3e60be7ad696285ef7dfc72a61",
    "f5e7e0bbc7e93c609003d6359208688070afb4a77165a552ff7be63b38dfbc86",
    "85ed8b11734de5b3e9836641413a8f36e99e89694f419c8cd25c3f3f16c40c5a",
    "5dd956292cf5d5704ad99e362d70098b2777b2a1730520be52f772ca48cd3bc0",
    "6f14018ca7d519e9b48d91af094c0f2d7e12e93af0228782671a8640092af9e5",
    "134be097c92e2c45a92f23208cacd89e4ce51f1009a0b900dbe83b38de11d791",
    "268f9392c20c6e31371a5131f82bd7713d3910075f2f0468baafaa1abd2f3187",
    "c668a05fd909714ed4a91e8d96d67b17e44329e88bc71e0672b529a3fc16be47",
    "99739351315840963011e4c5d8e90ad0bfed3facdcb713fe8f7138fbf01c4c94",
    "14ab53d61880471f66e80183968d97effd5492b406876060e595fcf9682f9295",
  ];

  let mut generator = Blake2Generator::new(KEY_000, 0);

  for reference in references {
    let program = generate_superscalar(&mut generator);

    let mut instructions = Vec::with_capacity(program.instructions.len());
    for instruction in &program.instructions {
      instructions.extend_from_slice(&instruction.to_bytes());
    }

    let digest: [u8; 32] = Blake2b256::digest(&instructions).into();

    assert_eq!(encode_hex(&digest), reference);
  }
}

/// Cache and dataset vectors. Allocates a 256 MiB Argon2d cache.
#[test]
#[ignore = "allocates a 256 MiB Argon2d cache"]
fn cache_and_dataset_vectors() {
  let hasher = RandomX::new(KEY_000).expect("cache init");
  let cache = hasher.cache();

  assert_eq!(cache.cache_word(0), 0x191e_0e1d_23c0_2186);
  assert_eq!(cache.cache_word(1568413), 0xf1b6_2fe6_210b_f8b1);
  assert_eq!(cache.cache_word(33554431), 0x1f47_f056_d05c_d99b);

  for (item_number, expected) in [
    (0_u64, 0x6805_88a8_5ae2_22db_u64),
    (10_000_000, 0x7943_a1f6_186f_fb72),
    (20_000_000, 0x9035_244d_7180_95e1),
    (30_000_000, 0x145a_5091_f785_3099),
  ] {
    let item = cache.dataset_item(item_number);
    assert_eq!(item.len(), DATASET_ITEM_SIZE / 8);
    assert_eq!(item[0], expected, "dataset item {item_number}");
  }
}

/// Full RandomX v1 hash vectors for the string key "test key 000".
#[test]
#[ignore = "allocates a 256 MiB Argon2d cache"]
fn hash_vectors_key_000() {
  let mut hasher = RandomX::new(KEY_000).expect("cache init");

  for (input, reference) in [
    (
      "This is a test",
      "639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f",
    ),
    (
      "Lorem ipsum dolor sit amet",
      "300a0adb47603dedb42228ccb2b211104f4da45af709cd7547cd049e9489c969",
    ),
    (
      "sed do eiusmod tempor incididunt ut labore et dolore magna aliqua",
      "c36d4ed4191e617309867ed66a443be4075014e2b061bcdaf9ce7b721d2b77a8",
    ),
  ] {
    let hash = hasher.hash(input.as_bytes()).expect("hash");
    assert_eq!(encode_hex(&hash), reference, "input {input:?}");
  }
  let paired = hasher
    .hash_pair([b"This is a test", b"Lorem ipsum dolor sit amet"])
    .expect("paired hashes");
  assert_eq!(
    encode_hex(&paired[0]),
    "639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f"
  );
  assert_eq!(
    encode_hex(&paired[1]),
    "300a0adb47603dedb42228ccb2b211104f4da45af709cd7547cd049e9489c969"
  );
  let triple = hasher
    .hash_triple([
      b"This is a test",
      b"Lorem ipsum dolor sit amet",
      b"sed do eiusmod tempor incididunt ut labore et dolore magna aliqua",
    ])
    .expect("triple hashes");
  assert_eq!(triple[0], paired[0]);
  assert_eq!(triple[1], paired[1]);
  assert_eq!(
    encode_hex(&triple[2]),
    "c36d4ed4191e617309867ed66a443be4075014e2b061bcdaf9ce7b721d2b77a8"
  );
  assert_eq!(hasher.hash(b"This is a test").expect("hash"), triple[0]);
}

/// Full RandomX v1 hash vectors for the string key "test key 001".
#[test]
#[ignore = "allocates a 256 MiB Argon2d cache"]
fn hash_vectors_key_001() {
  let mut hasher = RandomX::new(KEY_001).expect("cache init");

  let hash = hasher
    .hash(b"sed do eiusmod tempor incididunt ut labore et dolore magna aliqua")
    .expect("hash");
  assert_eq!(
    encode_hex(&hash),
    "e9ff4503201c0c2cca26d285c93ae883f9b1d30c9eb240b820756f2d5a7905fc"
  );

  let input = decode_hex(
    "0b0b98bea7e805e0010a2126d287a2a0cc833d312cb786385a7c2f9de69d2553\
     7f584a9bc9977b00000000666fd8753bf61a8631f12984e3fd44f4014eca62\
     9276817b56f32e9b68bd82f416",
  );

  let hash = hasher.hash(&input).expect("hash");
  assert_eq!(
    encode_hex(&hash),
    "c56414121acda1713c2f2a819d8ae38aed7c80c35c2a769298d34f03833cd5f1"
  );
}

/// Full RandomX v1 hash vector that exercises the ISUB_R register/immediate
/// edge case. The reference test derives the key from a 32-byte literal, so
/// its cache key is the first 31 bytes.
#[test]
#[ignore = "allocates a 256 MiB Argon2d cache"]
fn hash_vector_binary_key() {
  let key = decode_hex("7797373ea4633194640bf8d8c3b66724d6aa7bd2dc20e009df2f8f1710abe8");

  let mut hasher = RandomX::new(&key).expect("cache init");

  let input = decode_hex(
    "1010e1eaf8cf067b37b5f0ee031ab23ed1755e090a3af4415830145853e2be3e\
     1f6821fed84dae58d00e00da5214d6c1f2d0622e0abd51f9373d04e0b0f8e6d65\
     14d90689721c4aac5a9bb0d",
  );

  let hash = hasher.hash(&input).expect("hash");
  assert_eq!(
    encode_hex(&hash),
    "78af2a1864c42abce36d2e8983e13df99b2af0ce1362999af09fab004d4435a8"
  );
}
