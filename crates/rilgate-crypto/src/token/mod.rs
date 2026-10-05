use anyhow::{Context, Result, anyhow};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::{Serialize, de::DeserializeOwned};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Copy)]
pub enum TokenPurpose {
  Presentation,
  Challenge,
  Receipt,
  Auth,
}

impl TokenPurpose {
  fn domain(self) -> &'static [u8] {
    match self {
      Self::Presentation => b"rilgate/v1/presentation\0",
      Self::Challenge => b"rilgate/v1/challenge\0",
      Self::Receipt => b"rilgate/v1/receipt\0",
      Self::Auth => b"rilgate/v1/auth\0",
    }
  }
}

pub fn sign_json<T: Serialize>(value: &T, key: &[u8], purpose: TokenPurpose) -> Result<String> {
  let payload = serde_json::to_vec(value)?;
  let payload_b64 = URL_SAFE_NO_PAD.encode(&payload);

  let mut mac = HmacSha256::new_from_slice(key).context("Invalid HMAC key")?;
  // The signature binds the token to its stage in the proof-of-work protocol.
  mac.update(purpose.domain());
  mac.update(payload_b64.as_bytes());
  let signature = mac.finalize().into_bytes();

  Ok(format!(
    "{}.{}",
    payload_b64,
    URL_SAFE_NO_PAD.encode(signature)
  ))
}

pub fn verify_json<T: DeserializeOwned>(
  token: &str,
  key: &[u8],
  purpose: TokenPurpose,
) -> Result<T> {
  let (payload_b64, signature_b64) = token
    .split_once('.')
    .ok_or_else(|| anyhow!("Malformed signed token"))?;

  let signature = URL_SAFE_NO_PAD
    .decode(signature_b64)
    .context("Malformed token signature")?;

  let mut mac = HmacSha256::new_from_slice(key).context("Invalid HMAC key")?;
  // The signature binds the token to its stage in the proof-of-work protocol.
  mac.update(purpose.domain());
  mac.update(payload_b64.as_bytes());
  mac
    .verify_slice(&signature)
    .map_err(|_| anyhow!("Invalid token signature"))?;

  let payload = URL_SAFE_NO_PAD
    .decode(payload_b64)
    .context("Malformed token payload")?;

  Ok(serde_json::from_slice(&payload)?)
}
