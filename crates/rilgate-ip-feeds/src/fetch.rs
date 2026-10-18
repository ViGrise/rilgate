use crate::{MAX_BODY_BYTES, MAX_REDIRECTS, REQUEST_TIMEOUT};
use anyhow::{Context, Result, bail, ensure};
use bytes::Bytes;

/// Conditional-request validators captured from a successful response.
#[derive(Debug, Clone, Default)]
pub struct Validators {
  pub etag: Option<http::HeaderValue>,
  pub last_modified: Option<http::HeaderValue>,
}

/// The result of a conditional feed fetch.
#[derive(Debug)]
pub enum Fetched {
  Modified { body: Bytes, validators: Validators },
  NotModified,
}

/// A bounded HTTP client for remote IP feeds.
pub struct FeedClient {
  client: reqwest::Client,
}

impl FeedClient {
  pub fn new() -> Result<Self> {
    let client = reqwest::Client::builder()
      .timeout(REQUEST_TIMEOUT)
      .user_agent(concat!("rilgate/", env!("CARGO_PKG_VERSION")))
      .redirect(redirect_policy())
      .build()?;
    Ok(Self { client })
  }

  pub async fn fetch(&self, url: &str, validators: &Validators) -> Result<Fetched> {
    let mut request = self.client.get(url);
    if let Some(etag) = &validators.etag {
      request = request.header(http::header::IF_NONE_MATCH, etag);
    }
    if let Some(last_modified) = &validators.last_modified {
      request = request.header(http::header::IF_MODIFIED_SINCE, last_modified);
    }
    let mut response = request.send().await.context("feed request failed")?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
      return Ok(Fetched::NotModified);
    }
    let status = response.status();
    ensure!(status.is_success(), "feed returned status {status}");
    if let Some(length) = response.content_length()
      && length > MAX_BODY_BYTES as u64
    {
      bail!("feed body exceeds {MAX_BODY_BYTES} bytes");
    }
    let etag = response.headers().get(http::header::ETAG).cloned();
    let last_modified = response.headers().get(http::header::LAST_MODIFIED).cloned();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.context("feed read failed")? {
      ensure!(
        body.len() + chunk.len() <= MAX_BODY_BYTES,
        "feed body exceeds {MAX_BODY_BYTES} bytes"
      );
      body.extend_from_slice(&chunk);
    }
    Ok(Fetched::Modified {
      body: Bytes::from(body),
      validators: Validators {
        etag,
        last_modified,
      },
    })
  }
}

fn redirect_policy() -> reqwest::redirect::Policy {
  reqwest::redirect::Policy::custom(|attempt| {
    if attempt.previous().len() > MAX_REDIRECTS {
      return attempt.error("too many redirects");
    }
    if let Some(first) = attempt.previous().first()
      && first.scheme() != attempt.url().scheme()
    {
      return attempt.error("redirect changes scheme");
    }
    attempt.follow()
  })
}
