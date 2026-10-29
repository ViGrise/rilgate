use crate::{observability::metrics::Metrics, proxy::client::ProxyService};
use anyhow::{Result, ensure};
use http::{HeaderMap, HeaderValue, Uri};
use moka::{Expiry, future::Cache};
use rilgate_config::OpenGraphConfig;
use rilgate_web::templates::context::OpenGraphTag;
use std::{
  sync::Arc,
  time::{Duration, Instant},
};
use tokio::sync::Semaphore;

mod html;

const MAX_CONCURRENT_FETCHES: usize = 16;
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BODY_BYTES: usize = 512 * 1024;
const MAX_REDIRECTS: usize = 3;
const MAX_TAGS: usize = 32;
const MAX_NAME_BYTES: usize = 64;
const MAX_CONTENT_BYTES: usize = 1024;
const MAX_HEADER_VALUE_BYTES: usize = 1024;

#[derive(Clone, Hash, PartialEq, Eq)]
struct CacheKey {
  host: String,
  path: String,
  query: Option<String>,
}
#[derive(Clone)]
struct Entry {
  tags: Vec<OpenGraphTag>,
  failed: bool,
  ttl: Duration,
}
struct EntryExpiry;
impl Expiry<CacheKey, Entry> for EntryExpiry {
  fn expire_after_create(&self, _: &CacheKey, entry: &Entry, _: Instant) -> Option<Duration> {
    Some(entry.ttl)
  }
}

pub struct OpenGraphService {
  config: OpenGraphConfig,
  condition: Option<rilgate_policy::conditions::CompiledCondition>,
  client: reqwest::Client,
  cache: Cache<CacheKey, Entry>,
  slots: Semaphore,
}
impl OpenGraphService {
  pub fn new(
    config: OpenGraphConfig,
    sets: &rilgate_policy::IpSets,
    geoip: &rilgate_policy::GeoDatabases,
  ) -> Result<Self> {
    config.validate()?;
    let client = reqwest::Client::builder()
      .no_proxy()
      .referer(false)
      .redirect(reqwest::redirect::Policy::custom(|attempt| {
        let previous = attempt.previous();
        if previous.len() > MAX_REDIRECTS {
          return attempt.error("redirect limit exceeded");
        }
        if previous
          .first()
          .is_some_and(|origin| origin.origin() != attempt.url().origin())
        {
          return attempt.error("redirect origin changed");
        }
        attempt.follow()
      }))
      .timeout(FETCH_TIMEOUT)
      .build()?;
    Ok(Self {
      cache: Cache::builder()
        .max_capacity(config.max_entries)
        .expire_after(EntryExpiry)
        .build(),
      condition: config.compile_condition(sets, geoip)?,
      config,
      client,
      slots: Semaphore::new(MAX_CONCURRENT_FETCHES),
    })
  }

  pub async fn lookup(
    self: &Arc<Self>,
    proxy: &ProxyService,
    uri: &Uri,
    request: &rilgate_policy::conditions::Request<'_>,
    metrics: &Metrics,
  ) -> Vec<OpenGraphTag> {
    if !self.config.enabled
      || self
        .condition
        .as_ref()
        .is_some_and(|condition| !condition.matches(request))
    {
      return Vec::new();
    }
    let host = request.host;
    if !self.config.overrides.is_empty() {
      metrics.open_graph_lookup("override");
      return self
        .config
        .overrides
        .iter()
        .map(|(name, content)| {
          let name = name.to_ascii_lowercase();
          let attribute = if name.contains(':') {
            "property"
          } else {
            "name"
          };
          OpenGraphTag {
            attribute,
            name,
            content: content.clone(),
          }
        })
        .collect();
    }
    let Ok(mut url) = proxy.request_url(uri) else {
      metrics.open_graph_lookup("failed");
      return Vec::new();
    };
    if !self.config.include_query {
      url.set_query(None);
    }
    let key = CacheKey {
      host: host.into(),
      path: url.path().into(),
      query: url.query().map(str::to_owned),
    };
    if let Some(entry) = self.cache.get(&key).await {
      metrics.open_graph_lookup("hit");
      return entry.tags;
    }
    let mut headers = HeaderMap::new();
    headers.insert(
      http::header::ACCEPT,
      HeaderValue::from_static("text/html,application/xhtml+xml"),
    );
    for (name, value) in &self.config.request_headers {
      let value = value.replace("{host}", host);
      if value.len() <= MAX_HEADER_VALUE_BYTES
        && let Ok(value) = HeaderValue::from_str(&value)
      {
        headers.insert(
          http::header::HeaderName::from_bytes(name.as_bytes()).expect("Validated header name"),
          value,
        );
      }
    }
    if proxy.cfg.preserve_host
      && !host.is_empty()
      && let Ok(value) = HeaderValue::from_str(host)
    {
      headers.insert(http::header::HOST, value);
    }
    let service = self.clone();
    // The cache initializer owns its fetch so response deadlines leave it running.
    let pending = tokio::spawn(async move {
      service
        .cache
        .try_get_with(key, async {
          let _slot = service.slots.try_acquire().map_err(|_| ())?;
          let result = tokio::time::timeout(FETCH_TIMEOUT, service.fetch(url, headers)).await;
          let tags = result.ok().and_then(Result::ok);
          let failed = tags.is_none();
          Ok::<_, ()>(Entry {
            tags: tags.unwrap_or_default(),
            failed,
            ttl: if failed {
              service.config.negative_ttl
            } else {
              service.config.ttl
            },
          })
        })
        .await
    });
    match tokio::time::timeout(self.config.timeout, pending).await {
      Ok(Ok(Ok(entry))) => {
        metrics.open_graph_lookup(if entry.failed { "failed" } else { "fetched" });
        entry.tags
      }
      Ok(Ok(Err(_))) => {
        metrics.open_graph_lookup("busy");
        Vec::new()
      }
      Ok(Err(_)) => {
        metrics.open_graph_lookup("failed");
        Vec::new()
      }
      Err(_) => {
        metrics.open_graph_lookup("timeout");
        Vec::new()
      }
    }
  }

  async fn fetch(&self, url: url::Url, headers: HeaderMap) -> Result<Vec<OpenGraphTag>> {
    let mut response = self.client.get(url).headers(headers).send().await?;
    ensure!(
      response.status() == http::StatusCode::OK,
      "upstream status is not 200"
    );
    let media_type = response
      .headers()
      .get(http::header::CONTENT_TYPE)
      .and_then(|value| value.to_str().ok())
      .and_then(|value| value.split(';').next())
      .unwrap_or_default()
      .trim();
    ensure!(
      media_type.eq_ignore_ascii_case("text/html")
        || media_type.eq_ignore_ascii_case("application/xhtml+xml"),
      "upstream media type is not HTML"
    );
    // Requests carry no accept-encoding, so an encoded body is not readable markup.
    ensure!(
      response
        .headers()
        .get_all(http::header::CONTENT_ENCODING)
        .iter()
        .all(|value| value.as_bytes().eq_ignore_ascii_case(b"identity")),
      "upstream content encoding is not identity"
    );
    let mut tags = Vec::new();
    let mut body = Vec::new();
    let mut scanner = html::HeadScanner::default();
    while body.len() < MAX_BODY_BYTES && tags.len() < MAX_TAGS {
      let Some(chunk) = response.chunk().await? else {
        break;
      };
      body.extend_from_slice(&chunk[..chunk.len().min(MAX_BODY_BYTES - body.len())]);
      let head_ended = scanner.scan(&body, &mut |meta| {
        if tags.len() < MAX_TAGS
          && let Some(tag) = self.tag(meta)
        {
          tags.push(tag);
        }
      });
      if head_ended {
        break;
      }
    }
    Ok(tags)
  }

  fn tag(&self, meta: html::RawMeta) -> Option<OpenGraphTag> {
    let (attribute, name) = match meta.property {
      Some(name) => ("property", name),
      None => ("name", meta.name?),
    };
    let name = html_escape::decode_html_entities(&name).to_ascii_lowercase();
    let valid_name = name.len() <= MAX_NAME_BYTES
      && name.bytes().all(|byte| {
        byte.is_ascii_lowercase()
          || byte.is_ascii_digit()
          || matches!(byte, b':' | b'_' | b'.' | b'-')
      });
    if !valid_name || !self.config.tags.matches(&name) {
      return None;
    }
    let mut content: String = html_escape::decode_html_entities(&meta.content?)
      .chars()
      .filter_map(|character| match character {
        '\t' | '\n' | '\r' => Some(' '),
        character if character.is_control() => None,
        character => Some(character),
      })
      .collect();
    let mut boundary = content.len().min(MAX_CONTENT_BYTES);
    while !content.is_char_boundary(boundary) {
      boundary -= 1;
    }
    content.truncate(boundary);
    Some(OpenGraphTag {
      attribute,
      name,
      content,
    })
  }
}
