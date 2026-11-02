#![no_main]
use libfuzzer_sys::fuzz_target;

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
  challenge: String,
  nonce: String,
  difficulty: u64,
}
fuzz_target!(|input: Input| {
  if let Ok(true) = rilgate_pow_hash::sha256::verifier::verify_fast(
    &input.challenge,
    &input.nonce,
    input.difficulty,
  ) {
    let nonce = input.nonce.parse::<u64>().unwrap();
    assert_eq!(nonce.to_string(), input.nonce);
  }
});
