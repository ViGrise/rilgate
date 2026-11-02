#![no_main]
use libfuzzer_sys::fuzz_target;
use std::sync::{Mutex, OnceLock};
static RANDOMX: OnceLock<Mutex<randomx_core::RandomX>> = OnceLock::new();
#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
  a: Vec<u8>,
  b: Vec<u8>,
  c: Vec<u8>,
}
fuzz_target!(|input: Input| {
  let mut input = input;
  input.a.truncate(256);
  input.b.truncate(256);
  input.c.truncate(256);
  let mut hasher = RANDOMX
    .get_or_init(|| Mutex::new(randomx_core::RandomX::new(b"rilgate-fuzz-seed").unwrap()))
    .lock()
    .unwrap();
  let a = hasher.hash(&input.a);
  let again = hasher.hash(&input.a);
  assert_eq!(a.as_ref().ok(), again.as_ref().ok());
  let b = hasher.hash(&input.b);
  let c = hasher.hash(&input.c);
  match hasher.hash_pair([&input.a, &input.b]) {
    Ok(pair) => assert_eq!(pair, [*a.as_ref().unwrap(), *b.as_ref().unwrap()]),
    Err(_) => assert!(a.is_err() || b.is_err()),
  }
  match hasher.hash_triple([&input.a, &input.b, &input.c]) {
    Ok(triple) => assert_eq!(triple, [a.unwrap(), b.unwrap(), c.unwrap()]),
    Err(_) => assert!(a.is_err() || b.is_err() || c.is_err()),
  }
});
