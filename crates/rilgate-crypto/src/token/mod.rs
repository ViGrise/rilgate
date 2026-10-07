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

/// A keyed, purpose-bound HMAC state that can be shared across requests.
#[derive(Clone)]
pub struct TokenSigner {
  mac: HmacSha256,
}

impl TokenSigner {
  pub fn new(key: &[u8], purpose: TokenPurpose) -> Result<Self> {
    let mut mac = HmacSha256::new_from_slice(key).context("Invalid HMAC key")?;
    // The signature binds the token to its stage in the proof-of-work protocol.
    mac.update(purpose.domain());
    Ok(Self { mac })
  }

  pub fn sign_json<T: Serialize>(&self, value: &T) -> Result<String> {
    let payload = serde_json::to_vec(value)?;
    let payload_len = base64::encoded_len(payload.len(), false)
      .ok_or_else(|| anyhow!("Token payload too large"))?;
    let mut token = String::with_capacity(payload_len + 44);
    URL_SAFE_NO_PAD.encode_string(&payload, &mut token);
    let mut mac = self.mac.clone();
    mac.update(token.as_bytes());
    let signature = mac.finalize().into_bytes();
    token.push('.');
    URL_SAFE_NO_PAD.encode_string(signature, &mut token);
    Ok(token)
  }

  pub fn verify_json<T: DeserializeOwned>(&self, token: &str) -> Result<T> {
    let (payload_b64, signature_b64) = token
      .split_once('.')
      .ok_or_else(|| anyhow!("Malformed signed token"))?;
    if signature_b64.len() != 43 {
      return Err(anyhow!("Malformed token signature"));
    }
    // Base64 decoding reserves space for a complete final three-byte group.
    let mut signature = [0u8; 33];
    let length = URL_SAFE_NO_PAD
      .decode_slice(signature_b64, &mut signature)
      .context("Malformed token signature")?;
    let mut mac = self.mac.clone();
    mac.update(payload_b64.as_bytes());
    mac
      .verify_slice(&signature[..length])
      .map_err(|_| anyhow!("Invalid token signature"))?;
    let payload = URL_SAFE_NO_PAD
      .decode(payload_b64)
      .context("Malformed token payload")?;
    Ok(serde_json::from_slice(&payload)?)
  }
}

pub fn sign_json<T: Serialize>(value: &T, key: &[u8], purpose: TokenPurpose) -> Result<String> {
  TokenSigner::new(key, purpose)?.sign_json(value)
}

pub fn verify_json<T: DeserializeOwned>(
  token: &str,
  key: &[u8],
  purpose: TokenPurpose,
) -> Result<T> {
  TokenSigner::new(key, purpose)?.verify_json(token)
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::{Value, json};

  fn reference(value: &Value, key: &[u8], purpose: TokenPurpose) -> String {
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).unwrap());
    let mut mac = HmacSha256::new_from_slice(key).unwrap();
    mac.update(purpose.domain());
    mac.update(payload.as_bytes());
    format!(
      "{payload}.{}",
      URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    )
  }

  #[test]
  fn prepared_signers_preserve_wire_format_and_isolate_keys_and_purposes() {
    let purposes = [
      TokenPurpose::Presentation,
      TokenPurpose::Challenge,
      TokenPurpose::Receipt,
      TokenPurpose::Auth,
    ];
    for (index, purpose) in purposes.iter().copied().enumerate() {
      for size in [0, 1, 2, 3, 63, 64, 65, 1024] {
        let key = vec![b'k'; size];
        let signer = TokenSigner::new(&key, purpose).unwrap();
        let value =
          json!({"host": "example.test", "path": "</script>\n\"", "padding": "x".repeat(size)});
        let token = signer.sign_json(&value).unwrap();
        assert_eq!(token, reference(&value, &key, purpose));
        assert_eq!(signer.verify_json::<Value>(&token).unwrap(), value);
        assert_eq!(signer.sign_json(&value).unwrap(), token);
        assert!(
          TokenSigner::new(b"other", purpose)
            .unwrap()
            .verify_json::<Value>(&token)
            .is_err()
        );
        for (other_index, other) in purposes.iter().copied().enumerate() {
          if index != other_index {
            assert!(
              TokenSigner::new(&key, other)
                .unwrap()
                .verify_json::<Value>(&token)
                .is_err()
            );
          }
        }
      }
    }
  }

  #[test]
  fn malformed_and_tampered_tokens_are_rejected() {
    let signer = TokenSigner::new(b"secret", TokenPurpose::Auth).unwrap();
    let token = signer.sign_json(&json!({"host":"example.test"})).unwrap();
    let (payload, signature) = token.split_once('.').unwrap();
    for malformed in [
      String::new(),
      "abc".into(),
      format!("{payload}."),
      format!("{payload}.{}", "A".repeat(42)),
      format!("{payload}.{signature}A"),
      format!("{payload}.{signature}.extra"),
      format!("{payload}.{}", "!".repeat(43)),
      format!("e30.{signature}"),
      format!("{payload}.{}", "A".repeat(43)),
    ] {
      assert!(
        signer.verify_json::<Value>(&malformed).is_err(),
        "{malformed}"
      );
    }
  }
}
