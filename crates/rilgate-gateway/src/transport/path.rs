/// The request path that policy conditions match: percent-encoded unreserved
/// characters and `%2F` decoded, and runs of `/` collapsed to one.
pub(crate) fn policy_path(raw: &str) -> Result<String, &'static str> {
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
  let decoded = String::from_utf8(decoded).expect("ASCII rewrites keep UTF-8 valid");
  if decoded
    .split('/')
    .any(|segment| matches!(segment.split(';').next(), Some("." | "..")))
  {
    return Err("Request path must not require URL normalization");
  }
  Ok(decoded)
}

#[cfg(test)]
mod tests {
  use super::policy_path;

  #[test]
  fn policy_path_decodes_unreserved_and_collapses_slashes() {
    assert_eq!(policy_path("//admin//panel").unwrap(), "/admin/panel");
    assert_eq!(policy_path("/%61dmin%2Fpanel").unwrap(), "/admin/panel");
    assert_eq!(policy_path("/a%20b").unwrap(), "/a%20b");
    for rejected in ["/x/..;/admin", "/a/%2e%2e/b", "/a/.;x/b"] {
      assert_eq!(
        policy_path(rejected),
        Err("Request path must not require URL normalization"),
        "{rejected}"
      );
    }
  }
}
