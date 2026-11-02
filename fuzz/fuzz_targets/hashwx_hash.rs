#![no_main]
use libfuzzer_sys::fuzz_target;

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
  seed: [u8; 32],
  nonces: Vec<u64>,
}
fuzz_target!(|input: Input| {
  let mut reused = hashwx_core::HashWx::new(&input.seed);
  for nonce in input.nonces.into_iter().take(8) {
    assert_eq!(
      reused.hash(nonce),
      hashwx_core::HashWx::new(&input.seed).hash(nonce)
    );
  }
});
