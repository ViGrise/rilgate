#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
  let bytes = &bytes[..bytes.len().min(200 * 1024)];
  let count = rilgate_stratum::fuzzing::read_json_lines(bytes);
  assert!(count <= bytes.iter().filter(|&&byte| byte == b'\n').count());
});
