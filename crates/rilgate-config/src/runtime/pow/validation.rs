use super::PowConfig;

impl PowConfig {
  pub fn validate(&self) -> anyhow::Result<()> {
    self.rate_limit.validate()?;
    validate_randomx_fallback(self)?;
    validate_default_algorithm(self)?;
    validate_hashwx(self)?;
    validate_randomx_verify(self)?;
    validate_key_rotation_and_cookie(self)?;
    validate_randomx_limits(self)?;
    self.auth_binding.validate()?;
    self.stratum_pool.validate()?;
    validate_ttls(self)?;
    validate_difficulty(self)?;
    validate_browser(self)?;
    validate_signing_keys(self)?;
    validate_policy(self)
  }
}

fn validate_randomx_fallback(config: &PowConfig) -> anyhow::Result<()> {
  use anyhow::ensure;
  if let Some(fallback) = &config.randomx_fallback {
    use crate::{PowAlgorithm, PowEnforcement};
    use rilgate_pow::Algorithm;
    ensure!(
      matches!(
        fallback.algorithm,
        PowAlgorithm::Fast | PowAlgorithm::Hashwx
      ),
      "pow.algorithms.randomx.fallback.algorithm: expected fast or hashwx"
    );
    ensure!(
      config.enforce == PowEnforcement::LocalVerify,
      "pow.algorithms.randomx.fallback: requires pow.verification: local_verify"
    );
    ensure!(
      fallback.algorithm != PowAlgorithm::Hashwx || config.hashwx_enabled,
      "pow.algorithms.randomx.fallback.algorithm: hashwx is not enabled"
    );
    let backend: Box<dyn Algorithm> = match fallback.algorithm {
      PowAlgorithm::Fast => Box::new(rilgate_pow_hash::FastAlgorithm::new(
        &rilgate_pow_hash::FastConfig {
          ttl: config.challenge_ttl_seconds,
        },
      )),
      PowAlgorithm::Hashwx => Box::new(rilgate_pow_hashwx::HashWxAlgorithm::new(&config.hashwx)?),
      _ => unreachable!(),
    };
    backend
      .validate_difficulty(fallback.difficulty)
      .map_err(|error| anyhow::anyhow!("pow.algorithms.randomx.fallback.difficulty: {error}"))?;
  }
  Ok(())
}

fn validate_default_algorithm(config: &PowConfig) -> anyhow::Result<()> {
  use anyhow::ensure;
  ensure!(
    config.default_algorithm != crate::PowAlgorithm::Refresh,
    "pow.default_algorithm: refresh needs an explicit difficulty, so select it in a policy rule"
  );
  Ok(())
}

fn validate_hashwx(config: &PowConfig) -> anyhow::Result<()> {
  use anyhow::Context;
  if config.hashwx_enabled {
    config.hashwx.validate().context("pow.algorithms.hashwx")?;
  }
  Ok(())
}

fn validate_randomx_verify(config: &PowConfig) -> anyhow::Result<()> {
  use anyhow::{Context, ensure};
  let verify = &config.randomx_verify;
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
    let url = url::Url::parse(endpoint).context("pow.algorithms.randomx.verify.remote.endpoint")?;
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
  Ok(())
}

fn validate_key_rotation_and_cookie(config: &PowConfig) -> anyhow::Result<()> {
  use anyhow::{Context, ensure};
  config
    .signing_keys
    .validate()
    .context("security.keys.challenge_rotation")?;
  config
    .auth_signing_keys
    .validate()
    .context("security.keys.auth_rotation")?;
  ensure!(
    !config.auth_cookie_name.is_empty()
      && config
        .auth_cookie_name
        .bytes()
        .all(|byte| { byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte) }),
    "security.auth.cookie must be a nonempty HTTP token"
  );
  Ok(())
}

fn validate_randomx_limits(config: &PowConfig) -> anyhow::Result<()> {
  use anyhow::ensure;
  ensure!(
    (1..=1024).contains(&config.verifier_queue_capacity),
    "pow.algorithms.randomx.verifier_queue must be between 1 and 1024"
  );
  ensure!(
    config.randomx_cache_idle_timeout <= std::time::Duration::from_secs(604_800),
    "pow.algorithms.randomx.cache.idle_timeout must be at most 7d"
  );
  Ok(())
}

fn validate_ttls(config: &PowConfig) -> anyhow::Result<()> {
  use anyhow::ensure;
  for (path, ttl) in [
    ("pow.challenge.ttl", config.challenge_ttl_seconds),
    (
      "pow.algorithms.randomx.challenge_ttl",
      config.randomx_challenge_ttl_seconds,
    ),
    ("pow.challenge.receipt_ttl", config.receipt_ttl_seconds),
    ("security.auth.ttl", config.auth_ttl_seconds),
  ] {
    ensure!(
      (1..=31_536_000).contains(&ttl),
      "{path}: expected a whole-second TTL between 1s and one year"
    );
  }
  Ok(())
}

fn validate_difficulty(config: &PowConfig) -> anyhow::Result<()> {
  use anyhow::ensure;
  ensure!(
    config.nonce_offset <= 252,
    "pow.algorithms.randomx.nonce_offset must fit inside a 256-byte hashing blob"
  );
  ensure!(
    config.difficulty.base >= 1000
      && config.difficulty.max >= config.difficulty.base
      && config.difficulty.weight_step > 0,
    "pow.algorithms.randomx.difficulty: requires base >= 1000, max >= base, and a positive score_step"
  );
  Ok(())
}

fn validate_browser(config: &PowConfig) -> anyhow::Result<()> {
  use anyhow::ensure;
  ensure!(
    (1..=8).contains(&config.browser.max_workers),
    "pow.algorithms.randomx.browser.max_workers: expected 1 to 8"
  );
  anyhow::ensure!(
    !config.browser.solver_module_path.is_empty(),
    "pow.algorithms.randomx.browser.module: cannot be empty"
  );
  {
    let path = &config.browser.solver_module_path;
    ensure!(
      !path.chars().any(
        |c| c.is_control() || matches!(c, '"' | '\'' | '<' | '>' | '\\' | '?' | '#' | '{' | '}')
      ),
      "pow.algorithms.randomx.browser.module: contains unsupported characters"
    );
    ensure!(
      !path.split('/').any(|part| matches!(part, "." | "..")),
      "pow.algorithms.randomx.browser.module: cannot contain dot segments"
    );
  }
  Ok(())
}

fn validate_signing_keys(config: &PowConfig) -> anyhow::Result<()> {
  use anyhow::ensure;
  for key in [&config.signing_key, &config.auth_signing_key] {
    ensure!(!key.expose().is_empty(), "PoW signing keys cannot be empty");
  }
  Ok(())
}

fn validate_policy(config: &PowConfig) -> anyhow::Result<()> {
  use anyhow::ensure;
  rilgate_policy::PolicyEngine::from_definition(&config.policy)?;
  for (name, selection) in config.policy.challenges()? {
    let algorithm = selection.algorithm.unwrap_or(config.default_algorithm);
    ensure!(
      config.enforce != crate::PowEnforcement::StratumPoolAccepted
        || algorithm == crate::PowAlgorithm::RandomX,
      "policy ({name}): stratum_pool_accepted requires RandomX challenges"
    );
    if let Some(difficulty) = selection.difficulty {
      let valid = match algorithm {
        crate::PowAlgorithm::Hashwx => {
          (1..=rilgate_pow_hashwx::MAX_DIFFICULTY).contains(&difficulty)
        }
        crate::PowAlgorithm::Fast => (1..=64).contains(&difficulty),
        crate::PowAlgorithm::RandomX => difficulty >= 1000,
        crate::PowAlgorithm::Refresh => (1..=10).contains(&difficulty),
      };
      ensure!(
        valid,
        "policy ({name}).challenge.difficulty: fast requires 1..64; hashwx requires 1..2^40; randomx requires at least 1000; refresh requires 1..10 seconds"
      );
    }
  }
  Ok(())
}
