use anyhow::{Context, Result};
use axum::{
  body::{Body, HttpBody as _},
  http::{
    HeaderMap, HeaderName, Request, Response,
    header::{
      CONNECTION, CONTENT_LENGTH, HOST, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION, TE, TRAILER,
      TRANSFER_ENCODING, UPGRADE,
    },
  },
};
use futures_util::TryStreamExt;
use http_body_util::{BodyExt, Limited};
use reqwest::Client;
use std::sync::{
  Arc,
  atomic::{AtomicU8, Ordering},
};
use std::time::Duration;

use crate::transport::errors::ApiError;
use rilgate_config::UpstreamConfig;

#[derive(Clone)]
pub struct ProxyService {
  cfg: UpstreamConfig,
  client: Client,
  upstream: url::Url,
}

impl ProxyService {
  pub fn new(cfg: UpstreamConfig) -> Result<Self> {
    cfg.validate()?;
    let upstream = url::Url::parse(&cfg.url)?;
    let client = Client::builder()
      .connect_timeout(Duration::from_secs(cfg.connect_timeout_seconds))
      .read_timeout(Duration::from_secs(cfg.read_timeout_seconds))
      .redirect(reqwest::redirect::Policy::none())
      .build()
      .context("Failed to build upstream HTTP client")?;

    Ok(Self {
      cfg,
      client,
      upstream,
    })
  }

  pub(crate) fn request_url(&self, uri: &http::Uri) -> Result<url::Url, ApiError> {
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

  pub async fn forward(&self, request: Request<Body>) -> Result<Response<Body>, ApiError> {
    let url = self.request_url(request.uri())?;
    let (parts, body) = request.into_parts();

    let limit = self.cfg.max_body_bytes;
    if body.size_hint().lower() > limit as u64
      || parts
        .headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|length| length > limit as u64)
    {
      return Err(ApiError::PayloadTooLarge);
    }
    let body_length = body.size_hint().exact().or_else(|| {
      parts
        .headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    });
    let upload_failure = Arc::new(AtomicU8::new(0));
    let failure = upload_failure.clone();
    let body = if body.size_hint().exact() == Some(0) {
      reqwest::Body::from(Vec::new())
    } else {
      let stream = Limited::new(body, limit)
        .into_data_stream()
        .map_err(move |error| {
          let code = if error.is::<http_body_util::LengthLimitError>() {
            2
          } else {
            1
          };
          failure.store(code, Ordering::Relaxed);
          error
        });
      reqwest::Body::wrap_stream(stream)
    };

    let mut builder = self.client.request(parts.method.clone(), url);
    let request_connection_headers = connection_headers(&parts.headers);

    for (name, value) in &parts.headers {
      if is_hop_by_hop(name) || request_connection_headers.contains(name) || name == CONTENT_LENGTH
      {
        continue;
      }

      if name == HOST && !self.cfg.preserve_host {
        continue;
      }

      builder = builder.header(name, value);
    }

    builder = if let Some(length) = body_length {
      builder.header(CONTENT_LENGTH, length)
    } else {
      // Unknown-length bodies need explicit framing even on GET requests.
      builder.header(TRANSFER_ENCODING, "chunked")
    };
    let response = builder.body(body).send().await;
    // Upload errors describe the client request even when the upstream also closes its connection.
    match upload_failure.load(Ordering::Relaxed) {
      2 => return Err(ApiError::PayloadTooLarge),
      1 => {
        return Err(ApiError::InvalidRequest(
          "Unable to read request body".into(),
        ));
      }
      _ => {}
    }
    let response = response.map_err(|error| {
      tracing::warn!(error = %error, "Upstream request failed");
      if error.is_timeout() {
        ApiError::GatewayTimeout
      } else {
        ApiError::BadGateway
      }
    })?;

    let status = response.status();
    let headers = response.headers().clone();
    let response_connection_headers = connection_headers(&headers);

    let mut out = Response::builder().status(status);

    for (name, value) in &headers {
      if is_hop_by_hop(name) || response_connection_headers.contains(name) {
        continue;
      }
      out = out.header(name, value);
    }

    out
      .body(Body::from_stream(response.bytes_stream()))
      .map_err(|error| ApiError::Internal(error.into()))
  }
}

fn is_hop_by_hop(name: &HeaderName) -> bool {
  name == CONNECTION
    || name == TE
    || name == TRAILER
    || name == TRANSFER_ENCODING
    || name == UPGRADE
    || name == PROXY_AUTHENTICATE
    || name == PROXY_AUTHORIZATION
    || name.as_str() == "keep-alive"
    || name.as_str() == "proxy-connection"
}

fn connection_headers(headers: &HeaderMap) -> Vec<HeaderName> {
  headers
    .get_all(CONNECTION)
    .iter()
    .filter_map(|value| value.to_str().ok())
    .flat_map(|value| value.split(','))
    .filter_map(|name| HeaderName::from_bytes(name.trim().as_bytes()).ok())
    .collect()
}
