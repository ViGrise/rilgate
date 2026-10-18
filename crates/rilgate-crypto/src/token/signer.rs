use super::{KeyRotation, TokenPurpose};
use anyhow::{Context, Result, anyhow};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde::{Serialize, de::DeserializeOwned};
use sha2::Sha256;
use std::collections::BTreeMap;

type HmacSha256 = Hmac<Sha256>;

/// A keyed, purpose-bound HMAC state that can be shared across requests.
#[derive(Clone)]
pub struct TokenSigner {
  mac: HmacSha256,
  key_id: Option<String>,
  verification: BTreeMap<String, HmacSha256>,
}

impl TokenSigner {
  pub fn new(key: &[u8], purpose: TokenPurpose) -> Result<Self> {
    let mut mac = HmacSha256::new_from_slice(key).context("Invalid HMAC key")?;
    // The signature binds the token to its stage in the proof-of-work protocol.
    mac.update(purpose.domain());
    Ok(Self {
      mac,
      key_id: None,
      verification: BTreeMap::new(),
    })
  }

  pub fn rotating(key: &[u8], purpose: TokenPurpose, rotation: &KeyRotation) -> Result<Self> {
    rotation.validate()?;
    let mut signer = Self::new(key, purpose)?;
    signer.key_id = rotation.active_id.clone();
    for (id, key) in &rotation.previous {
      signer
        .verification
        .insert(id.clone(), Self::new(key.expose().as_bytes(), purpose)?.mac);
    }
    Ok(signer)
  }

  pub fn sign_json<T: Serialize>(&self, value: &T) -> Result<String> {
    let payload = serde_json::to_vec(value)?;
    let payload_len = base64::encoded_len(payload.len(), false)
      .ok_or_else(|| anyhow!("Token payload too large"))?;
    let mut token = String::with_capacity(payload_len + 44);
    if let Some(id) = &self.key_id {
      token.push_str(id);
      token.push('.');
    }
    URL_SAFE_NO_PAD.encode_string(&payload, &mut token);
    let mut mac = self.mac.clone();
    mac.update(token.as_bytes());
    let signature = mac.finalize().into_bytes();
    token.push('.');
    URL_SAFE_NO_PAD.encode_string(signature, &mut token);
    Ok(token)
  }

  pub fn verify_json<T: DeserializeOwned>(&self, token: &str) -> Result<T> {
    anyhow::ensure!(token.len() <= 16_384, "Signed token exceeds maximum size");
    let (signed, signature_b64) = token
      .rsplit_once('.')
      .ok_or_else(|| anyhow!("Malformed signed token"))?;
    let (payload_b64, selected) = if let Some((id, payload)) = signed.split_once('.') {
      let mac = if self.key_id.as_deref() == Some(id) {
        &self.mac
      } else {
        self
          .verification
          .get(id)
          .ok_or_else(|| anyhow!("Unknown token key ID"))?
      };
      (payload, mac)
    } else {
      let mac = if self.key_id.is_none() {
        &self.mac
      } else {
        self
          .verification
          .get("legacy")
          .ok_or_else(|| anyhow!("Unversioned tokens are disabled"))?
      };
      (signed, mac)
    };
    if signature_b64.len() != 43 {
      return Err(anyhow!("Malformed token signature"));
    }
    // Base64 decoding reserves space for a complete final three-byte group.
    let mut signature = [0u8; 33];
    let length = URL_SAFE_NO_PAD
      .decode_slice(signature_b64, &mut signature)
      .context("Malformed token signature")?;
    let mut mac = selected.clone();
    mac.update(signed.as_bytes());
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
