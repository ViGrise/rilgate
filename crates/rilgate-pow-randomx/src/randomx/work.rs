use anyhow::{Context, Result, anyhow};
use uuid::Uuid;
pub(crate) fn bind_randomx_work(blob: &str, challenge_id: &Uuid) -> Result<String> {
  let mut input = hex::decode(blob).context("Invalid RandomX hashing blob")?;
  input.extend_from_slice(b"pow/v1\0");
  input.extend_from_slice(challenge_id.as_bytes());
  if input.len() > 256 {
    return Err(anyhow!(
      "RandomX hashing blob has no room for challenge binding"
    ));
  }
  Ok(hex::encode(input))
}
