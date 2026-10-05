use anyhow::{Result, anyhow};
pub(crate) use rilgate_core::time::unix_now;
pub(crate) fn validate_lifetime(
  version: u8,
  issued_at: i64,
  expires_at: i64,
  ttl: u64,
) -> Result<()> {
  let now = unix_now()?;
  if version != 1
    || issued_at > now
    || issued_at < 0
    || expires_at <= issued_at
    || expires_at.saturating_sub(issued_at) > ttl as i64
    || now >= expires_at
  {
    return Err(anyhow!("Token version or lifetime is invalid or expired"));
  }
  Ok(())
}
