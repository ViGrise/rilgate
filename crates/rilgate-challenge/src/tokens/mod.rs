pub(crate) mod validation;

use crate::ChallengeConfig;
use anyhow::Result;
use rilgate_crypto::token::{TokenPurpose, TokenSigner};

pub(crate) struct TokenSigners {
  pub presentation: TokenSigner,
  pub challenge: TokenSigner,
  pub receipt: TokenSigner,
  pub auth: TokenSigner,
}

impl TokenSigners {
  pub fn new(cfg: &ChallengeConfig) -> Result<Self> {
    let key = cfg.signing_key.expose().as_bytes();
    Ok(Self {
      presentation: TokenSigner::new(key, TokenPurpose::Presentation)?,
      challenge: TokenSigner::new(key, TokenPurpose::Challenge)?,
      receipt: TokenSigner::new(key, TokenPurpose::Receipt)?,
      auth: TokenSigner::new(cfg.auth_signing_key.expose().as_bytes(), TokenPurpose::Auth)?,
    })
  }
}
