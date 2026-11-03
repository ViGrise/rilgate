#![no_main]
use libfuzzer_sys::fuzz_target;

#[derive(arbitrary::Arbitrary, Debug)]
enum Challenge {
  Bytes([u8; 32]),
  Text(String),
}

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
  challenge: Challenge,
  nonce: String,
  nonces_per_seed: u32,
  difficulty: u64,
}
fuzz_target!(|input: Input| {
  let challenge = match input.challenge {
    Challenge::Bytes(bytes) => hex::encode(bytes),
    Challenge::Text(text) => text,
  };
  let verified = rilgate_pow_hashwx::fuzzing::verify(
    &challenge,
    &input.nonce,
    input.difficulty,
    input.nonces_per_seed,
  );
  if verified.is_ok() {
    let nonce = input.nonce.parse::<u64>().unwrap();
    assert_eq!(nonce.to_string(), input.nonce);
  }
});
