use http::HeaderValue;

/// The `Cache-Control` value of every response that clients must not store.
pub(crate) const NO_STORE: HeaderValue = HeaderValue::from_static("no-store");
