use crate::transport::errors::ApiError;
use anyhow::Result;
use rilgate_config::UpstreamConfig;
use std::{
  net::SocketAddr,
  sync::Arc,
  time::{Duration, Instant},
};
use tokio::sync::RwLock;

pub struct ProxyService {
  pub(crate) cfg: UpstreamConfig,
  upstream: url::Url,
  host: http::HeaderValue,
  base_path: String,
  resolved: RwLock<Option<(Instant, Arc<[SocketAddr]>)>>,
}
impl ProxyService {
  pub fn new(cfg: UpstreamConfig) -> Result<Self> {
    cfg.validate()?;
    let upstream = url::Url::parse(&cfg.url)?;
    let host = upstream[url::Position::BeforeHost..url::Position::AfterPort].parse()?;
    let base_path = upstream.path().trim_end_matches('/').to_owned();
    Ok(Self {
      upstream,
      host,
      base_path,
      cfg,
      resolved: RwLock::new(None),
    })
  }
  pub fn request_url(&self, uri: &http::Uri) -> Result<url::Url, ApiError> {
    let mut url = self.upstream.clone();
    let path = format!("{}{}", url.path().trim_end_matches('/'), uri.path());
    url.set_path(&path);
    // Policy matching and forwarding must use the same path representation.
    if url.path() != path {
      return Err(ApiError::InvalidRequest(
        "Request path must not require URL normalization".into(),
      ));
    }
    url.set_query(uri.query());
    Ok(url)
  }
  pub(crate) fn request_target(&self, uri: &http::Uri) -> Result<http::Uri, ApiError> {
    // Paths without dot segments or URL-encoded delimiters need no URL normalization.
    let simple_path = uri
      .path()
      .bytes()
      .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'~'));
    let simple_query = uri.query().is_none_or(|query| {
      query.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
          || matches!(
            byte,
            b'/'
              | b'-'
              | b'_'
              | b'~'
              | b'.'
              | b'%'
              | b'&'
              | b'='
              | b'+'
              | b':'
              | b';'
              | b','
              | b'@'
              | b'('
              | b')'
              | b'?'
          )
      })
    });
    if simple_path
      && simple_query
      && let Some(path) = uri.path_and_query()
    {
      if self.base_path.is_empty() {
        return Ok(path.clone().into());
      }
      return format!("{}{path}", self.base_path)
        .parse()
        .map_err(|_| ApiError::InvalidRequest("Invalid upstream URI".into()));
    }
    let url = self.request_url(uri)?;
    url[url::Position::BeforePath..url::Position::AfterQuery]
      .parse()
      .map_err(|_| ApiError::InvalidRequest("Invalid upstream URI".into()))
  }
  pub(crate) fn host(&self) -> http::HeaderValue {
    self.host.clone()
  }
  pub(crate) async fn peer(
    &self,
    index: usize,
    remaining: Duration,
  ) -> pingora::Result<(Box<pingora::upstreams::peer::HttpPeer>, usize)> {
    use pingora::{Error, ErrorType};
    let cached = self.resolved.read().await.clone();
    let addresses = match cached.filter(|(at, _)| at.elapsed() < Duration::from_secs(60)) {
      Some((_, addresses)) => addresses,
      None => {
        let mut resolved = self.resolved.write().await;
        if let Some((_, addresses)) = resolved
          .as_ref()
          .filter(|(at, _)| at.elapsed() < Duration::from_secs(60))
        {
          addresses.clone()
        } else {
          let host = self
            .upstream
            .host_str()
            .expect("validated upstream host")
            .trim_start_matches('[')
            .trim_end_matches(']');
          let port = self
            .upstream
            .port_or_known_default()
            .expect("HTTP upstream port");
          let addresses: Arc<[SocketAddr]> = tokio::time::timeout(
            remaining.min(Duration::from_secs(self.cfg.connect_timeout_seconds)),
            tokio::net::lookup_host((host, port)),
          )
          .await
          .map_err(|_| Error::explain(ErrorType::ConnectTimedout, "Upstream DNS timed out"))?
          .map_err(|error| Error::because(ErrorType::ConnectError, "Upstream DNS failed", error))?
          .take(16)
          .collect::<Vec<_>>()
          .into();
          if addresses.is_empty() {
            return Err(Error::explain(
              ErrorType::ConnectError,
              "Upstream DNS returned no addresses",
            ));
          }
          *resolved = Some((Instant::now(), addresses.clone()));
          addresses
        }
      }
    };
    let address = *addresses
      .get(index)
      .ok_or_else(|| Error::explain(ErrorType::ConnectError, "Upstream address list changed"))?;
    let tls = self.upstream.scheme() == "https";
    let sni = if tls {
      self
        .upstream
        .host_str()
        .unwrap()
        .trim_matches(['[', ']'])
        .to_owned()
    } else {
      String::new()
    };
    let mut peer = pingora::upstreams::peer::HttpPeer::new(address, tls, sni);
    peer.options.connection_timeout =
      Some(remaining.min(Duration::from_secs(self.cfg.connect_timeout_seconds)));
    peer.options.read_timeout = Some(Duration::from_secs(self.cfg.read_timeout_seconds));
    peer.options.write_timeout = Some(Duration::from_secs(self.cfg.read_timeout_seconds));
    peer.options.idle_timeout = Some(Duration::from_secs(90));
    Ok((Box::new(peer), addresses.len()))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn request_targets_match_url_validation_and_serialization() {
    for upstream in [
      "http://example.test",
      "http://example.test/base/",
      "http://example.test/a%20b/",
    ] {
      let proxy = ProxyService::new(UpstreamConfig {
        url: upstream.into(),
        preserve_host: true,
        max_body_bytes: 1024,
        connect_timeout_seconds: 2,
        read_timeout_seconds: 3,
      })
      .unwrap();
      let mut paths: Vec<String> = [
        "/",
        "/public",
        "/a//b",
        "/../protected",
        "/a/%2e%2e/b",
        "/a/.%2E/b",
        "/a\\b",
        "/file.json",
        "http://original.test/path?q=1",
        "example.test:443",
        "*",
      ]
      .into_iter()
      .map(str::to_owned)
      .collect();
      for byte in 0..=127u8 {
        paths.push(format!("/a{}b", char::from(byte)));
        paths.push(format!("/public?q=a{}b", char::from(byte)));
      }
      for path in paths {
        let Ok(uri) = path.parse::<http::Uri>() else {
          continue;
        };
        let expected = proxy.request_url(&uri);
        let actual = proxy.request_target(&uri);
        match (expected, actual) {
          (Ok(url), Ok(target)) => assert_eq!(
            target.to_string(),
            &url[url::Position::BeforePath..url::Position::AfterQuery],
            "{upstream} {path}"
          ),
          (Err(_), Err(_)) => {}
          other => panic!("{upstream} {path}: {other:?}"),
        }
      }
    }
  }
  #[tokio::test]
  async fn https_peers_verify_certificates_and_use_the_upstream_hostname() {
    let proxy = ProxyService::new(UpstreamConfig {
      url: "https://localhost:8443/base".into(),
      preserve_host: true,
      max_body_bytes: 1024,
      connect_timeout_seconds: 2,
      read_timeout_seconds: 3,
    })
    .unwrap();
    let (peer, _) = proxy.peer(0, Duration::from_secs(30)).await.unwrap();
    assert_eq!(peer.sni, "localhost");
    assert!(peer.options.verify_cert);
    assert!(peer.options.verify_hostname);
    assert_eq!(
      peer.options.connection_timeout,
      Some(Duration::from_secs(2))
    );
  }
}
