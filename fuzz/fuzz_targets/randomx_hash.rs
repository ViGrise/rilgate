#![no_main]
use libfuzzer_sys::fuzz_target;
use std::sync::{Mutex, OnceLock};
static RANDOMX: OnceLock<Mutex<randomx_core::RandomX>> = OnceLock::new();

// Each light-mode hash is slow, so one run checks one batch against the single hashes of its
// inputs. The hasher stays alive across runs, so its batch VMs carry state from earlier inputs.
#[derive(arbitrary::Arbitrary, Debug)]
enum Batch {
  Pair([Vec<u8>; 2]),
  Triple([Vec<u8>; 3]),
}

fn capped(input: &[u8]) -> &[u8] {
  &input[..input.len().min(256)]
}

fuzz_target!(|batch: Batch| {
  let mut hasher = RANDOMX
    .get_or_init(|| Mutex::new(randomx_core::RandomX::new(b"rilgate-fuzz-seed").unwrap()))
    .lock()
    .unwrap();
  match &batch {
    Batch::Pair(inputs) => {
      let inputs = inputs.each_ref().map(|input| capped(input));
      let batched = hasher.hash_pair(inputs).unwrap();
      assert_eq!(batched, inputs.map(|input| hasher.hash(input).unwrap()));
    }
    Batch::Triple(inputs) => {
      let inputs = inputs.each_ref().map(|input| capped(input));
      let batched = hasher.hash_triple(inputs).unwrap();
      assert_eq!(batched, inputs.map(|input| hasher.hash(input).unwrap()));
    }
  }
});
