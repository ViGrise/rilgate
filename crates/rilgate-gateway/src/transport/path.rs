/// The longest request path that the gateway accepts. A presentation token carries the path,
/// so this bound keeps every token and proof API request within their size limits.
pub const MAX_REQUEST_PATH_BYTES: usize = 8 * 1024;

/// The request path that policy conditions match: percent-encoded unreserved
/// characters and `%2F` decoded, and runs of `/` collapsed to one.
pub(crate) fn policy_path(raw: &str) -> Result<String, &'static str> {
  let decoded = decode_unreserved(raw);
  // A decoded byte can complete a new escape, as in `%4%44`. A path that changes again on a
  // second decode has two readings, so policy and an upstream that decodes again could differ.
  if decoded
    .split('/')
    .any(|segment| matches!(segment.split(';').next(), Some("." | "..")))
    || decoded.contains('\\')
    || decoded
      .as_bytes()
      .windows(3)
      .any(|escape| escape.eq_ignore_ascii_case(b"%5c"))
    || decode_unreserved(&decoded) != decoded
  {
    return Err("Request path must not require URL normalization");
  }
  Ok(decoded)
}

fn decode_unreserved(raw: &str) -> String {
  let bytes = raw.as_bytes();
  let mut decoded = Vec::with_capacity(bytes.len());
  let mut index = 0;
  while index < bytes.len() {
    let mut byte = bytes[index];
    if byte == b'%'
      && let Some(value) = bytes
        .get(index + 1..index + 3)
        .and_then(|digits| std::str::from_utf8(digits).ok())
        .and_then(|digits| u8::from_str_radix(digits, 16).ok())
        .filter(|value| {
          value.is_ascii_alphanumeric() || matches!(value, b'-' | b'.' | b'_' | b'~' | b'/')
        })
    {
      byte = value;
      index += 2;
    }
    index += 1;
    if byte != b'/' || decoded.last() != Some(&b'/') {
      decoded.push(byte);
    }
  }
  // Only ASCII bytes are rewritten, so the result stays valid UTF-8.
  String::from_utf8(decoded).expect("ASCII rewrites keep UTF-8 valid")
}

#[cfg(test)]
mod tests {
  use super::policy_path;

  #[test]
  fn policy_path_decodes_unreserved_and_collapses_slashes() {
    assert_eq!(policy_path("//admin//panel").unwrap(), "/admin/panel");
    assert_eq!(policy_path("/%61dmin%2Fpanel").unwrap(), "/admin/panel");
    assert_eq!(policy_path("/a%20b").unwrap(), "/a%20b");
    assert_eq!(policy_path("/%2541").unwrap(), "/%2541");
    for rejected in [
      "/public/..%5cadmin",
      "/public/..%5Cadmin",
      "/public/..%5%63admin",
      "/public/..\\admin",
      "/x/..;/admin",
      "/a/%2e%2e/b",
      "/a/.;x/b",
      "/%4%44min",
      "/a}.%44F/}.%4%44",
    ] {
      assert_eq!(
        policy_path(rejected),
        Err("Request path must not require URL normalization"),
        "{rejected}"
      );
    }
  }
}
