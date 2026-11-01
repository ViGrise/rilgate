use http::{HeaderValue, Method};

/// A gateway-owned endpoint below `site.base_path`.
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
  Exchange,
  Refresh,
  Session,
  WebSocket,
  Static,
}

/// The listener that serves an endpoint. Other listeners answer 404.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Exposure {
  Public,
  Operations,
}

impl Endpoint {
  /// Maps a path below `site.base_path` to its endpoint.
  pub(crate) fn from_path(path: &str) -> Option<Self> {
    Some(match path {
      "/reload" => Self::Reload,
      "/api/live" => Self::Live,
      "/api/health" => Self::Health,
      "/api/ready" => Self::Ready,
      "/api/metrics" => Self::Metrics,
      "/challenge" => Self::ChallengePage,
      "/api/client-config" => Self::ClientConfig,
      "/api/challenge" => Self::Challenge,
      "/api/verify" => Self::Verify,
      "/api/exchange" => Self::Exchange,
      "/api/refresh" => Self::Refresh,
      "/api/session" => Self::Session,
      "/api/ws" => Self::WebSocket,
      _ if path.starts_with("/trap/") => Self::Trap,
      _ if path.starts_with("/static/") => Self::Static,
      _ => return None,
    })
  }

  pub(crate) fn exposure(self) -> Exposure {
    match self {
      Self::Reload | Self::Live | Self::Health | Self::Ready | Self::Metrics => {
        Exposure::Operations
      }
      _ => Exposure::Public,
    }
  }

  /// Proof endpoints need a resolved client address and pass the proof rate limit.
  pub(crate) fn is_proof(self) -> bool {
    matches!(
      self,
      Self::ChallengePage
        | Self::Challenge
        | Self::Verify
        | Self::Exchange
        | Self::WebSocket
        | Self::Refresh
        | Self::Session
    )
  }

  fn accepts_post(self) -> bool {
    matches!(
      self,
      Self::Reload | Self::Challenge | Self::Verify | Self::Exchange
    )
  }

  /// Reload, Challenge, Verify and Exchange accept POST. The others accept GET, and HEAD
  /// except WebSocket.
  pub(crate) fn accepts(self, method: &Method) -> bool {
    if self.accepts_post() {
      method == Method::POST
    } else {
      method == Method::GET || (method == Method::HEAD && self != Self::WebSocket)
    }
  }

  /// The `Allow` header value for a 405 answer: "POST" or "GET, HEAD".
  pub(crate) fn allow(self) -> HeaderValue {
    HeaderValue::from_static(if self.accepts_post() {
      "POST"
    } else {
      "GET, HEAD"
    })
  }
}
