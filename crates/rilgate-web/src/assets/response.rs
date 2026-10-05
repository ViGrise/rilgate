use super::{AssetMetadata, AssetSource};
use anyhow::Result;
use axum::{
  body::Body,
  http::{
    HeaderMap, StatusCode,
    header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_NONE_MATCH},
  },
  response::{IntoResponse, Response},
};
use tokio_util::io::ReaderStream;
impl AssetMetadata {
  pub fn response(&self, headers: &HeaderMap, head: bool) -> Result<Response> {
    let unchanged = headers
      .get(IF_NONE_MATCH)
      .and_then(|h| h.to_str().ok())
      .is_some_and(|value| {
        value.split(',').any(|tag| {
          tag.trim() == "*" || tag.trim().strip_prefix("W/").unwrap_or(tag.trim()) == self.etag
        })
      });
    let body = if head || unchanged {
      Body::empty()
    } else {
      match &self.source {
        AssetSource::Embedded(bytes) => Body::from(*bytes),
        AssetSource::Memory(bytes) => Body::from(bytes.clone()),
        AssetSource::File(file) => {
          Body::from_stream(ReaderStream::new(tokio::fs::File::from_std(file.reopen()?)))
        }
      }
    };
    let mut response = (
      if unchanged {
        StatusCode::NOT_MODIFIED
      } else {
        StatusCode::OK
      },
      body,
    )
      .into_response();
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
