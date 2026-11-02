#![no_main]
use libfuzzer_sys::fuzz_target;

#[derive(arbitrary::Arbitrary, Debug)]
struct Input {
  path: String,
  accept: Option<String>,
  content_type: Option<String>,
  requested_with: Option<String>,
}
fuzz_target!(|input: Input| {
  if let Ok(path) = rilgate_gateway::fuzzing::policy_path(&input.path) {
    assert_eq!(rilgate_gateway::fuzzing::policy_path(&path), Ok(path));
  }
  let _ = rilgate_gateway::fuzzing::wants_json(
    input.accept.as_deref(),
    input.content_type.as_deref(),
    input.requested_with.as_deref(),
    &input.path,
  );
  let mut headers = http::HeaderMap::new();
  if let Some(value) = input
    .accept
    .and_then(|value| http::HeaderValue::from_str(&value).ok())
  {
    headers.insert(http::header::COOKIE, value);
  }
  let _ = rilgate_gateway::auth::cookies::cookie_value(&headers, "rilgate_auth");
});
