use crate::schema::Document;
use anyhow::Result;
use rilgate_crypto::{Secret, token::KeyRotation};

pub(super) struct SigningKeys {
  pub signing_key: Secret,
  pub auth_signing_key: Secret,
  pub signing_keys: KeyRotation,
  pub auth_signing_keys: KeyRotation,
  pub secret_generated: bool,
}

impl Document {
  pub(super) fn resolve_keys(&self) -> Result<SigningKeys> {
    let mut secret_generated = false;
    let (signing_key, auth_signing_key, signing_keys, auth_signing_keys) =
      match (&self.security.secret, &self.security.keys) {
        (None, None) => {
          secret_generated = true;
          let secret = Secret::generate()?;
          let (challenge, auth) = rilgate_crypto::derivation::signing_keys(&secret)?;
          (challenge, auth, Default::default(), Default::default())
        }
        (Some(secret), None) => {
          crate::assembly::validate_secret(secret, "security.secret")?;
          let (challenge, auth) = rilgate_crypto::derivation::signing_keys(secret)?;
          (challenge, auth, Default::default(), Default::default())
        }
        (None, Some(keys)) => (
          keys.challenge.clone(),
          keys.auth.clone(),
          keys.challenge_rotation.clone(),
          keys.auth_rotation.clone(),
        ),
        (Some(_), Some(_)) => anyhow::bail!("security: secret and keys are mutually exclusive"),
      };
    Ok(SigningKeys {
      signing_key,
      auth_signing_key,
      signing_keys,
      auth_signing_keys,
      secret_generated,
    })
  }
}
