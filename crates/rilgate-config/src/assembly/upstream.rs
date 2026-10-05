use crate::{UpstreamConfig, loader::file::FileConfig, resolution::resolve};
pub(super) fn load(file: &FileConfig) -> UpstreamConfig {
  UpstreamConfig {
    url: resolve(
      file,
      "upstream.url",
      &[],
      None,
      "http://127.0.0.1:3000".to_string(),
    ),
    preserve_host: resolve(file, "upstream.preserve_host", &[], None, true),
    max_body_bytes: resolve(
      file,
      "upstream.max_body_bytes",
      &[],
      None,
      16 * 1024 * 1024_usize,
    ),
    connect_timeout_seconds: resolve(file, "upstream.connect_timeout_seconds", &[], None, 5_u64),
    read_timeout_seconds: resolve(file, "upstream.read_timeout_seconds", &[], None, 30_u64),
  }
}
