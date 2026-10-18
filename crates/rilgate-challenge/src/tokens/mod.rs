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
      presentation: TokenSigner::rotating(key, TokenPurpose::Presentation, &cfg.signing_keys)?,
      challenge: TokenSigner::rotating(key, TokenPurpose::Challenge, &cfg.signing_keys)?,
      receipt: TokenSigner::rotating(key, TokenPurpose::Receipt, &cfg.signing_keys)?,
      auth: TokenSigner::rotating(
        cfg.auth_signing_key.expose().as_bytes(),
        TokenPurpose::Auth,
        &cfg.auth_signing_keys,
      )?,
    })
  }
}
