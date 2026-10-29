use crate::{MAX_BODY_BYTES, MAX_REDIRECTS, REQUEST_TIMEOUT};
use anyhow::{Context, Result, bail, ensure};
use bytes::Bytes;
use rilgate_policy::Validators;

/// The result of a conditional feed fetch.
#[derive(Debug)]
pub enum Fetched {
  Modified { body: Bytes, validators: Validators },
  NotModified,
}

/// Per-request limits and credentials for a feed fetch.
#[derive(Debug, Clone)]
pub struct FetchOptions<'a> {
  pub max_bytes: usize,
  pub basic_auth: Option<(&'a str, &'a str)>,
}

impl Default for FetchOptions<'_> {
  fn default() -> Self {
    Self {
      max_bytes: MAX_BODY_BYTES,
      basic_auth: None,
    }
  }
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
    self
      .fetch_with(url, validators, &FetchOptions::default())
      .await
  }

  pub async fn fetch_with(
    &self,
    url: &str,
    validators: &Validators,
    options: &FetchOptions<'_>,
  ) -> Result<Fetched> {
    let max_bytes = options.max_bytes;
    let mut request = self.client.get(url);
    if let Some((user, password)) = options.basic_auth {
      request = request.basic_auth(user, Some(password));
    }
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
      && length > max_bytes as u64
    {
      bail!("feed body exceeds {max_bytes} bytes");
    }
    let etag = response.headers().get(http::header::ETAG).cloned();
    let last_modified = response.headers().get(http::header::LAST_MODIFIED).cloned();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.context("feed read failed")? {
      ensure!(
        body.len() + chunk.len() <= max_bytes,
        "feed body exceeds {max_bytes} bytes"
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
