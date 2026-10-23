use super::pow::PowConfig;
impl PowConfig {
  pub fn validate(&self) -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    self.rate_limit.validate()?;
    let verify = &self.randomx_verify;
    ensure!(
      (1..=2).contains(&verify.order.len())
        && (verify.order.len() == 1 || verify.order[0] != verify.order[1]),
      "pow.algorithms.randomx.verify.order: expected one or two distinct hashers"
    );
    let remote = &verify.remote;
    ensure!(
      (std::time::Duration::from_millis(100)..=std::time::Duration::from_secs(30))
        .contains(&remote.timeout.0),
      "pow.algorithms.randomx.verify.remote.timeout: expected 100ms to 30s"
    );
    ensure!(
      remote.tls.client_cert.is_some() == remote.tls.client_key.is_some(),
      "pow.algorithms.randomx.verify.remote.tls: client_cert and client_key must both be set or absent"
    );
    if let Some(endpoint) = &remote.endpoint {
      let url =
        url::Url::parse(endpoint).context("pow.algorithms.randomx.verify.remote.endpoint")?;
      ensure!(
        matches!(url.scheme(), "https" | "http")
          && url.host_str().is_some()
          && matches!(url.path(), "" | "/")
          && url.query().is_none()
          && url.fragment().is_none()
          && url.username().is_empty()
          && url.password().is_none(),
        "pow.algorithms.randomx.verify.remote.endpoint: expected an HTTP or HTTPS origin without path, query or credentials"
      );
    }
    if let Some(name) = &remote.server_name {
      ensure!(
        !name.is_empty() && !name.chars().any(|c| c.is_whitespace()),
        "pow.algorithms.randomx.verify.remote.server_name: expected a certificate name"
      );
    }
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
    ensure!(
      self.randomx_cache_idle_timeout <= std::time::Duration::from_secs(604_800),
      "pow.algorithms.randomx.cache.idle_timeout must be at most 7d"
    );
    self.auth_binding.validate()?;
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
