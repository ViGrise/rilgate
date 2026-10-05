use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
  io::{Read, Write},
  path::Path,
  sync::Arc,
};
use tempfile::NamedTempFile;
pub enum AssetSource {
  Embedded(&'static [u8]),
  Memory(Vec<u8>),
  File(Arc<NamedTempFile>),
}
pub struct AssetMetadata {
  pub source: AssetSource,
  pub content_type: String,
  pub size: u64,
  pub etag: String,
}
impl AssetMetadata {
  fn from_bytes(name: &str, bytes: &[u8], source: AssetSource) -> Self {
    Self {
      source,
      content_type: mime_guess::from_path(name)
        .first_or_octet_stream()
        .to_string(),
      size: bytes.len() as u64,
      etag: format!("\"{}\"", hex::encode(Sha256::digest(bytes))),
    }
  }
  pub fn embedded(name: &str, bytes: &'static [u8]) -> Self {
    Self::from_bytes(name, bytes, AssetSource::Embedded(bytes))
  }
  pub fn memory(name: &str, bytes: Vec<u8>) -> Self {
    Self::from_bytes(name, &bytes, AssetSource::Memory(bytes.clone()))
  }
  pub fn file(root: &Path, name: &str, path: &Path) -> Result<Self> {
    let mut input = crate::resources::loader::open_checked(root, path)?;
    let mut output =
      NamedTempFile::new().context("Creating immutable static asset backing file")?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut size = 0;
    loop {
      let count = input
        .read(&mut buffer)
        .with_context(|| format!("Reading static asset {name}"))?;
      if count == 0 {
        break;
      }
      hash.update(&buffer[..count]);
      output.write_all(&buffer[..count])?;
      size += count as u64;
    }
    output.flush()?;
    Ok(Self {
      source: AssetSource::File(Arc::new(output)),
      content_type: mime_guess::from_path(name)
        .first_or_octet_stream()
        .to_string(),
      size,
      etag: format!("\"{}\"", hex::encode(hash.finalize())),
    })
  }
  pub fn bytes(&self) -> Result<Vec<u8>> {
    match &self.source {
      AssetSource::Embedded(bytes) => Ok(bytes.to_vec()),
      AssetSource::Memory(bytes) => Ok(bytes.clone()),
      AssetSource::File(file) => {
        let mut bytes = Vec::new();
        file.reopen()?.read_to_end(&mut bytes)?;
        Ok(bytes)
      }
    }
  }
}
