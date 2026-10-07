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
impl AssetMetadata {
  pub fn response(&self, headers: &HeaderMap, head: bool) -> Result<Response<Body>> {
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
        AssetSource::Memory(bytes) => body(Bytes::copy_from_slice(bytes)),
        AssetSource::File(file) => StreamBody::new(
          ReaderStream::new(tokio::fs::File::from_std(file.reopen()?))
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
    out.insert(CACHE_CONTROL, "public, max-age=0, must-revalidate".parse()?);
    if !unchanged {
      out.insert(CONTENT_TYPE, self.content_type.parse()?);
      out.insert(CONTENT_LENGTH, self.size.to_string().parse()?);
    }
    Ok(response)
  }
}
