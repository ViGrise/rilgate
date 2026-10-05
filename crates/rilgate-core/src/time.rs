use anyhow::{Context, Result};
use std::time::{SystemTime, UNIX_EPOCH};
pub fn unix_now() -> Result<i64> {
  Ok(
    SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .context("System clock is before UNIX epoch")?
      .as_secs() as i64,
  )
}
