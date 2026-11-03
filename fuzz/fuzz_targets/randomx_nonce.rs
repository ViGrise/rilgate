#![no_main]
use libfuzzer_sys::fuzz_target;

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
  blob: Vec<u8>,
  offset: usize,
  nonce_hex: String,
}
fuzz_target!(|input: Input| {
  let mut blob = input.blob;
  blob.truncate(512);
  let before = blob.clone();
  if rilgate_pow_randomx::randomx::verification::write_nonce(
    &mut blob,
    input.offset,
    &input.nonce_hex,
  )
  .is_ok()
  {
    assert_eq!(blob.len(), before.len());
    let end = input.offset.checked_add(4).unwrap();
    let mut written = blob[input.offset..end].to_vec();
    written.reverse();
    assert_eq!(written, hex::decode(&input.nonce_hex).unwrap());
    for (index, (a, b)) in blob.iter().zip(&before).enumerate() {
      if !(input.offset..end).contains(&index) {
        assert_eq!(a, b);
      }
    }
  }
});
