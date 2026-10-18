use super::pow::PowConfig;
impl PowConfig {
  pub fn validate(&self) -> anyhow::Result<()> {
    use anyhow::ensure;
    self.rate_limit.validate()?;
    self.signing_keys.validate()?;
    self.auth_signing_keys.validate()?;
    ensure!(
      !self.auth_cookie_name.is_empty()
        && self
          .auth_cookie_name
          .bytes()
          .all(|byte| { byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte) }),
      "pow.auth_cookie_name must be a nonempty HTTP token"
    );
    ensure!(
      (1..=1024).contains(&self.verifier_queue_capacity),
      "pow.verifier_queue_capacity must be between 1 and 1024"
    );
    self.p2pool.validate()?;
    for ttl in [
      self.challenge_ttl_seconds,
      self.randomx_challenge_ttl_seconds,
      self.receipt_ttl_seconds,
      self.auth_ttl_seconds,
      self.p2pool.session_ttl_seconds,
    ] {
      ensure!(
        (1..=31_536_000).contains(&ttl),
        "Token and session TTLs must be between 1 second and 1 year"
      );
    }
    ensure!(
      self.nonce_offset <= 252,
      "pow.nonce_offset must fit inside a 256-byte hashing blob"
    );
    ensure!(
      self.difficulty.base >= 1000
        && self.difficulty.max >= self.difficulty.base
        && self.difficulty.weight_step > 0,
      "RandomX difficulty requires base >= 1000, max >= base, and a positive weight_step"
    );
    anyhow::ensure!(
      !self.browser.solver_module_path.is_empty(),
      "Browser solver path cannot be empty"
    );
    {
      let path = &self.browser.solver_module_path;
      ensure!(
        !path
          .chars()
          .any(|c| c.is_control()
            || matches!(c, '"' | '\'' | '<' | '>' | '\\' | '?' | '#' | '{' | '}')),
        "Gateway paths contain unsupported characters"
      );
      ensure!(
        !path.split('/').any(|part| matches!(part, "." | "..")),
        "Gateway paths cannot contain dot segments"
      );
    }
    for key in [&self.signing_key, &self.auth_signing_key] {
      ensure!(!key.expose().is_empty(), "PoW signing keys cannot be empty");
    }
    http::HeaderName::from_bytes(self.risk.debug_header.as_bytes())?;
    Ok(())
  }
}
