use http::{HeaderValue, Method};

/// A gateway-owned endpoint. Public listeners resolve endpoints below `site.base_path`;
/// operations listeners resolve them below `server.operations.base_path`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Endpoint {
  Reload,
  Live,
  Health,
  Ready,
  Metrics,
  Trap,
  ChallengePage,
  ClientConfig,
  Challenge,
  Verify,
  HookVerify,
  HookCallback,
  Exchange,
  Refresh,
  Session,
  WebSocket,
  Static,
}

impl Endpoint {
  /// Maps a path below `site.base_path` to its public endpoint.
  pub(crate) fn from_public_path(path: &str) -> Option<Self> {
    Some(match path {
      "/challenge" => Self::ChallengePage,
      "/api/client-config" => Self::ClientConfig,
      "/api/challenge" => Self::Challenge,
      "/api/verify" => Self::Verify,
      "/api/hook/verify" => Self::HookVerify,
      "/api/hook/callback" => Self::HookCallback,
      "/api/exchange" => Self::Exchange,
      "/api/refresh" => Self::Refresh,
      "/api/session" => Self::Session,
      "/api/ws" => Self::WebSocket,
      _ if path.starts_with("/trap/") => Self::Trap,
      _ if path.starts_with("/static/") => Self::Static,
      _ => return None,
    })
  }

  /// Maps a path below `server.operations.base_path` to its operations endpoint.
  pub(crate) fn from_operations_path(path: &str) -> Option<Self> {
    Some(match path {
      "/reload" => Self::Reload,
      "/live" => Self::Live,
      "/health" => Self::Health,
      "/ready" => Self::Ready,
      "/metrics" => Self::Metrics,
      _ => return None,
    })
  }

  /// Proof endpoints need a resolved client address and pass the proof rate limit.
  pub(crate) fn is_proof(self) -> bool {
    matches!(
      self,
      Self::ChallengePage
        | Self::Challenge
        | Self::Verify
        | Self::HookVerify
        | Self::HookCallback
        | Self::Exchange
        | Self::WebSocket
        | Self::Refresh
        | Self::Session
    )
  }

  fn accepts_post(self) -> bool {
    matches!(
      self,
      Self::Reload | Self::Challenge | Self::Verify | Self::Exchange | Self::HookVerify
    )
  }

  /// Checks the HTTP methods supported by the endpoint.
  pub(crate) fn accepts(self, method: &Method) -> bool {
    if self.accepts_post() {
      method == Method::POST
    } else {
      method == Method::GET
        || (method == Method::HEAD && !matches!(self, Self::WebSocket | Self::HookCallback))
    }
  }

  /// The supported HTTP methods for a 405 response.
  pub(crate) fn allow(self) -> HeaderValue {
    HeaderValue::from_static(if self.accepts_post() {
      "POST"
    } else if self == Self::HookCallback {
      "GET"
    } else {
      "GET, HEAD"
    })
  }
}
