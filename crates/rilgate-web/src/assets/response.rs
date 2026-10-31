use super::{AssetMetadata, AssetSource};
use anyhow::Result;
use bytes::Bytes;
use futures_util::TryStreamExt;
use http::{
  HeaderMap, Response, StatusCode,
  header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_NONE_MATCH},
};
use http_body_util::{BodyExt, Full, StreamBody};
pub type Body = http_body_util::combinators::UnsyncBoxBody<Bytes, std::io::Error>;
pub fn body(bytes: impl Into<Bytes>) -> Body {
  Full::new(bytes.into())
    .map_err(|never| match never {})
    .boxed_unsync()
}
use tokio_util::io::ReaderStream;
/// Cache policy of an asset response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetCache {
  /// Caches store the asset and revalidate it on every use.
  Revalidate,
  /// The URL names the revision of the asset, so caches keep it for a year.
  Immutable,
}
impl AssetCache {
  fn header(self) -> &'static str {
    match self {
      Self::Revalidate => "public, max-age=0, must-revalidate",
      Self::Immutable => "public, max-age=31536000, immutable",
    }
  }
}
impl AssetMetadata {
  pub fn response(
    &self,
    headers: &HeaderMap,
    head: bool,
    cache: AssetCache,
  ) -> Result<Response<Body>> {
    let unchanged = headers
      .get(IF_NONE_MATCH)
      .and_then(|h| h.to_str().ok())
      .is_some_and(|value| {
        value.split(',').any(|tag| {
          tag.trim() == "*" || tag.trim().strip_prefix("W/").unwrap_or(tag.trim()) == self.etag
        })
      });
    let body = if head || unchanged {
      body(Bytes::new())
    } else {
      match &self.source {
        AssetSource::Embedded(bytes) => body(*bytes),
        AssetSource::Memory(bytes) => body(bytes.clone()),
        AssetSource::File(file) => StreamBody::new(
          // Larger chunks amortize blocking file reads while bounding memory per response.
          ReaderStream::with_capacity(
            tokio::fs::File::from_std(file.reopen()?),
            self.size.clamp(1, 64 * 1024) as usize,
          )
          .map_ok(http_body::Frame::data),
        )
        .boxed_unsync(),
      }
    };
    let mut response = Response::builder()
      .status(if unchanged {
        StatusCode::NOT_MODIFIED
      } else {
        StatusCode::OK
      })
      .body(body)?;
    let out = response.headers_mut();
    out.insert(ETAG, self.etag.parse()?);
    out.insert(CACHE_CONTROL, cache.header().parse()?);
    if !unchanged {
      out.insert(CONTENT_TYPE, self.content_type.parse()?);
      out.insert(CONTENT_LENGTH, self.size.to_string().parse()?);
    }
    Ok(response)
  }
}
