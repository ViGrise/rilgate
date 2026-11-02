#![no_main]
use libfuzzer_sys::fuzz_target;
use rilgate_crypto::token::{TokenPurpose, sign_json, verify_json};
#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
  key: [u8; 32],
  purpose: u8,
  value: String,
  token: String,
  flip: usize,
}
fuzz_target!(|input: Input| {
  let purposes = [
    TokenPurpose::Presentation,
    TokenPurpose::Challenge,
    TokenPurpose::Receipt,
    TokenPurpose::Auth,
  ];
  let index = usize::from(input.purpose % 4);
  let purpose = purposes[index];
  let value = serde_json::from_str::<serde_json::Value>(&input.value)
    .unwrap_or(serde_json::Value::String(input.value));
  let payload = serde_json::to_string(&value).unwrap();
  // Real claims are well below 4 KiB of JSON.
  if payload.len() > 4096 {
    return;
  }
  let signed = sign_json(&value, &input.key, purpose).unwrap();
  if input.token != signed {
    assert!(verify_json::<serde_json::Value>(&input.token, &input.key, purpose).is_err());
  }
  // The token keeps the payload bytes. The comparison uses the raw payload text, because a JSON
  // number that is larger than an integer type parses into a float that can change its last bit.
  assert_eq!(
    verify_json::<Box<serde_json::value::RawValue>>(&signed, &input.key, purpose)
      .unwrap()
      .get(),
    payload
  );
  for (other, purpose) in purposes.into_iter().enumerate() {
    if other != index {
      assert!(verify_json::<serde_json::Value>(&signed, &input.key, purpose).is_err());
    }
  }
  let mut changed = signed.into_bytes();
  let flip = input.flip % changed.len();
  changed[flip] = if changed[flip] == b'A' { b'B' } else { b'A' };
  if let Ok(decoded) = verify_json::<Box<serde_json::value::RawValue>>(
    std::str::from_utf8(&changed).unwrap(),
    &input.key,
    purpose,
  ) {
    assert_eq!(decoded.get(), payload);
  }
});
