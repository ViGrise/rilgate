use super::pow::PowConfig;
impl PowConfig {
  pub fn validate(&self) -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    self.rate_limit.validate()?;
    self
      .signing_keys
      .validate()
      .context("security.keys.challenge_rotation")?;
    self
      .auth_signing_keys
      .validate()
      .context("security.keys.auth_rotation")?;
    ensure!(
      !self.auth_cookie_name.is_empty()
        && self
          .auth_cookie_name
          .bytes()
          .all(|byte| { byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte) }),
      "security.auth.cookie must be a nonempty HTTP token"
    );
    ensure!(
      (1..=1024).contains(&self.verifier_queue_capacity),
      "pow.algorithms.randomx.verifier_queue must be between 1 and 1024"
    );
    self.p2pool.validate()?;
    for (path, ttl) in [
      ("pow.challenge.ttl", self.challenge_ttl_seconds),
      (
        "pow.algorithms.randomx.challenge_ttl",
        self.randomx_challenge_ttl_seconds,
      ),
      ("pow.challenge.receipt_ttl", self.receipt_ttl_seconds),
      ("security.auth.ttl", self.auth_ttl_seconds),
    ] {
      ensure!(
        (1..=31_536_000).contains(&ttl),
        "{path}: expected a whole-second TTL between 1s and one year"
      );
    }
    ensure!(
      self.nonce_offset <= 252,
      "pow.algorithms.randomx.nonce_offset must fit inside a 256-byte hashing blob"
    );
    ensure!(
      self.difficulty.base >= 1000
        && self.difficulty.max >= self.difficulty.base
        && self.difficulty.weight_step > 0,
      "pow.algorithms.randomx.difficulty: requires base >= 1000, max >= base, and a positive score_step"
    );
    anyhow::ensure!(
      !self.browser.solver_module_path.is_empty(),
      "pow.algorithms.randomx.browser.module: cannot be empty"
    );
    {
      let path = &self.browser.solver_module_path;
      ensure!(
        !path
          .chars()
          .any(|c| c.is_control()
            || matches!(c, '"' | '\'' | '<' | '>' | '\\' | '?' | '#' | '{' | '}')),
        "pow.algorithms.randomx.browser.module: contains unsupported characters"
      );
      ensure!(
        !path.split('/').any(|part| matches!(part, "." | "..")),
        "pow.algorithms.randomx.browser.module: cannot contain dot segments"
      );
    }
    for key in [&self.signing_key, &self.auth_signing_key] {
      ensure!(!key.expose().is_empty(), "PoW signing keys cannot be empty");
    }
    rilgate_policy::PolicyEngine::from_definition(&self.policy)?;
    for (name, selection) in self.policy.challenges()? {
      let algorithm = selection.algorithm.unwrap_or(self.default_algorithm);
      ensure!(
        self.enforce != crate::PowEnforcement::P2poolAccepted
          || algorithm == crate::PowAlgorithm::RandomX,
        "policy ({name}): p2pool_accepted requires RandomX challenges"
      );
      if let Some(difficulty) = selection.difficulty {
        let valid = match algorithm {
          crate::PowAlgorithm::Fast => (1..=64).contains(&difficulty),
          crate::PowAlgorithm::RandomX => difficulty >= 1000,
        };
        ensure!(
          valid,
          "policy ({name}).challenge.difficulty: fast requires 1..64; randomx requires at least 1000"
        );
      }
    }
    Ok(())
  }
}
