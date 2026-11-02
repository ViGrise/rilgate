#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
  if let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) {
    let _ = rilgate_stratum::fuzzing::parse_job(&value);
  }
});
