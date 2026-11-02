#![no_main]
use libfuzzer_sys::fuzz_target;

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
  challenge: String,
  nonce: String,
  nonces_per_seed: u32,
  difficulty: u64,
}
fuzz_target!(|input: Input| {
  let bytes = rilgate_pow_hashwx::fuzzing::challenge_bytes(&input.challenge);
  let nonce = rilgate_pow_hashwx::fuzzing::nonce(&input.nonce);
  if let Ok(nonce) = nonce {
    assert_eq!(nonce.to_string(), input.nonce);
  }
  if let (Ok(bytes), Ok(nonce)) = (bytes, nonce)
    && input.nonces_per_seed >= 1
    && input.difficulty >= 1
  {
    let seed = rilgate_pow_hashwx::proof::seed(&bytes, nonce / u64::from(input.nonces_per_seed));
    let _ = hashwx_core::HashWx::new(&seed).hash(nonce) < u64::MAX / input.difficulty;
  }
});
