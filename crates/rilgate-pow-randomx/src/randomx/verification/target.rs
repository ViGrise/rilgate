use anyhow::{Context, Result, anyhow};
/// Writes the gateway's numeric hexadecimal nonce into the hashing blob.
pub fn write_nonce(blob: &mut [u8], nonce_offset: usize, nonce_hex: &str) -> Result<()> {
  let nonce = hex::decode(nonce_hex).context("Nonce must be hexadecimal")?;

  if nonce.len() != 4 {
    return Err(anyhow!(
      "Nonce must contain exactly 4 bytes / 8 hex characters"
    ));
  }

  let end = nonce_offset
    .checked_add(4)
    .ok_or_else(|| anyhow!("Nonce offset overflow"))?;

  if end > blob.len() {
    return Err(anyhow!("Nonce offset is outside the hashing blob"));
  }

  // RandomX hashes the nonce bytes in little-endian order.
  let value = u32::from_be_bytes(nonce.try_into().expect("4 byte nonce"));
  blob[nonce_offset..end].copy_from_slice(&value.to_le_bytes());

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn nonce_is_written_little_endian_at_configured_offset() {
    let mut blob = vec![0_u8; 64];
    write_nonce(&mut blob, 39, "01020304").unwrap();
    assert_eq!(&blob[39..43], &[4, 3, 2, 1]);
  }
}
