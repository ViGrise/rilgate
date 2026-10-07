use http::{HeaderMap, HeaderValue};
use rilgate_gateway::auth::cookies::cookie_value;

#[test]
fn cookie_lookup_preserves_header_order_and_value_boundaries() {
  for (raw, expected) in [
    ("other=value", None),
    ("auth_extra=other; auth=token", Some("token")),
    ("Auth=other; auth=token", Some("token")),
    (
      "malformed; auth=token=padding; other=value",
      Some("token=padding"),
    ),
    ("auth=first; auth=second", Some("first")),
    ("auth=; auth=second", Some("")),
    (" other=value;  auth=token  ; lang=en", Some("token")),
  ] {
    let mut headers = HeaderMap::new();
    headers.append("cookie", HeaderValue::from_bytes(b"\x80").unwrap());
    headers.append("cookie", raw.parse().unwrap());
    assert_eq!(cookie_value(&headers, "auth"), expected, "{raw}");
  }
}
