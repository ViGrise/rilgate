use http::HeaderMap;
use rilgate_challenge::authorization::binding::BindingInput;
use std::sync::OnceLock;

/// A binding input with no client attributes.
pub fn unbound() -> BindingInput<'static> {
  static EMPTY: OnceLock<HeaderMap> = OnceLock::new();
  BindingInput::unbound(EMPTY.get_or_init(HeaderMap::new))
}
