use crate::Secret;
use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

pub fn signing_keys(secret: &Secret) -> Result<(Secret, Secret)> {
  let hkdf = hkdf::Hkdf::<sha2::Sha256>::new(
    Some(b"rilgate/v1/key-derivation"),
    secret.expose().as_bytes(),
  );
  let derive = |context: &[u8]| -> Result<Secret> {
    let mut key = [0u8; 32];
    hkdf
      .expand(context, &mut key)
      .map_err(|_| anyhow::anyhow!("Invalid signing key derivation length"))?;
    Ok(Secret(URL_SAFE_NO_PAD.encode(key)))
  };
  Ok((
    derive(b"challenge-signing")?,
    derive(b"authentication-signing")?,
  ))
}
