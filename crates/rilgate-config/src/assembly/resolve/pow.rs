use super::keys::SigningKeys;
use crate::{
  BrowserConfig, DifficultyConfig, PowAlgorithm, PowConfig, PowVerification, RateLimitConfig,
  StratumPoolConfig, VerificationModes,
  runtime::RandomXFallback,
  schema::{Document, pow::StratumPool},
};
use anyhow::{Context, Result, ensure};
use rilgate_policy::PolicyDocument;

impl Document {
  pub(super) fn requires_stratum_pool(&self, policy_document: &PolicyDocument) -> Result<bool> {
    Ok(
      self.pow.default_algorithm == PowAlgorithm::RandomX
        || self.pow.algorithms.randomx.verify.mode == PowVerification::PoolShare
        || policy_document
          .challenges()?
          .iter()
          .any(|(_, c)| c.algorithm.unwrap_or(self.pow.default_algorithm) == PowAlgorithm::RandomX),
    )
  }

  pub(super) fn resolve_stratum_pool(
    &self,
    policy_document: &PolicyDocument,
    needs_pool: bool,
  ) -> Result<StratumPoolConfig> {
    ensure!(
      !needs_pool || self.pow.stratum_pool.is_some(),
      "pow.stratum_pool.pools: required when RandomX jobs or stratum_pool_accepted verification are enabled"
    );
    let fallback = StratumPool::default();
    let pool = self.pow.stratum_pool.as_ref().unwrap_or(&fallback);
    validate_proxies(&pool.proxies, "pow.stratum_pool.proxies")?;
    let largest_difficulty = policy_document
      .challenges()?
      .iter()
      .filter(|(_, challenge)| {
        challenge.algorithm.unwrap_or(self.pow.default_algorithm) == PowAlgorithm::RandomX
      })
      .filter_map(|(_, challenge)| challenge.difficulty)
      .fold(
        self.pow.algorithms.randomx.difficulty.max.max(1000),
        u64::max,
      );
    let difficulty_digits = largest_difficulty.to_string().len();
    let mut names = std::collections::HashSet::new();
    let pools = pool.pools.iter().enumerate().map(|(index, entry)| {
      let path = format!("pow.stratum_pool.pools[{index}]");
      ensure!(valid_name(&entry.name) && names.insert(&entry.name), "{path}.name: expected a unique name of 1 to 64 lowercase letters, digits, hyphens or underscores");
      let url = url::Url::parse(&format!("tcp://{}", entry.address))
        .map_err(|_| anyhow::anyhow!("{path}.address: expected HOST:PORT or [IPv6]:PORT"))?;
      ensure!(url.host_str().is_some() && url.port().is_some() && url.username().is_empty() && url.password().is_none() && url.path().is_empty() && url.query().is_none() && url.fragment().is_none(), "{path}.address: expected HOST:PORT without credentials or a path");
      entry.tls.validate().with_context(|| format!("{path}.tls.server_name"))?;
      let user = entry.login.user.expose();
      let remainder = user.replace("{difficulty}", "");
      ensure!(user.matches("{difficulty}").count() == 1 && !remainder.contains(['{', '}']) && remainder.len() + difficulty_digits <= 256, "{path}.login.user: expected exactly one {{difficulty}}, no other placeholders, and at most 256 bytes after substitution");
      if let Some(proxies) = &entry.proxies { validate_proxies(proxies, &format!("{path}.proxies"))?; }
      Ok(rilgate_pool::StratumConfig {
        name: entry.name.clone(),
    difficulty_mismatches: Default::default(),
        tls: entry.tls.clone(),
        proxies: entry.proxies.clone(),
        host: url.host_str().unwrap().trim_matches(['[', ']']).into(),
        port: url.port().unwrap(),
        user: entry.login.user.clone(),
        password: entry.login.password.clone(),
        agent: pool.agent.clone(),
        connect_timeout: pool.timeout.connect.0,
        submit_timeout: pool.timeout.submit.0,
      })
    }).collect::<Result<Vec<_>>>()?;
    Ok(StratumPoolConfig {
      pools,
      proxies: pool.proxies.clone(),
      failover_timeout: pool.failover.timeout.0,
      cooldown: pool.failover.cooldown.0,
      agent: pool.agent.clone(),
      connect_timeout: pool.timeout.connect.0,
      submit_timeout: pool.timeout.submit.0,
      max_sessions: pool.sessions.max,
      session_ttl: pool.sessions.ttl.0,
      job_session_mode: pool.job_sessions.mode,
      max_job_sessions: pool.job_sessions.max,
      job_session_idle: pool.job_sessions.idle.0,
    })
  }

  pub(super) fn resolve_pow(
    &self,
    keys: SigningKeys,
    policy_document: PolicyDocument,
    stratum_pool: StratumPoolConfig,
    needs_pool: bool,
  ) -> Result<PowConfig> {
    let randomx = &self.pow.algorithms.randomx;
    let mut verify = randomx.verify.clone();
    verify.remote.server_name = verify.remote.effective_server_name();
    Ok(PowConfig {
      randomx_fallback: randomx
        .fallback
        .as_ref()
        .map(|fallback| {
          Ok::<_, anyhow::Error>(RandomXFallback {
            algorithm: fallback.algorithm.parse().map_err(|_| {
              anyhow::anyhow!("pow.algorithms.randomx.fallback.algorithm: expected fast or hashwx")
            })?,
            difficulty: fallback.difficulty,
          })
        })
        .transpose()?,
      hashwx_enabled: policy_document.challenges()?.iter().any(|(_, choice)| {
        choice.algorithm.unwrap_or(self.pow.default_algorithm) == PowAlgorithm::Hashwx
      }),
      hashwx: rilgate_pow_hashwx::HashWxConfig {
        nonces_per_seed: self.pow.algorithms.hashwx.nonces_per_seed,
        difficulty: DifficultyConfig {
          base: self.pow.algorithms.hashwx.difficulty.base,
          weight_step: self.pow.algorithms.hashwx.difficulty.score_step,
          max: self.pow.algorithms.hashwx.difficulty.max,
        },
        ttl: self.pow.challenge.ttl.whole_seconds("pow.challenge.ttl")?,
      },
      randomx_enabled: needs_pool,
      verification: VerificationModes {
        randomx: self.pow.algorithms.randomx.verify.mode,
      },
      default_algorithm: self.pow.default_algorithm,
      signing_key: keys.signing_key,
      auth_signing_key: keys.auth_signing_key,
      signing_keys: keys.signing_keys,
      auth_signing_keys: keys.auth_signing_keys,
      challenge_ttl_seconds: self.pow.challenge.ttl.whole_seconds("pow.challenge.ttl")?,
      receipt_ttl_seconds: self
        .pow
        .challenge
        .receipt_ttl
        .whole_seconds("pow.challenge.receipt_ttl")?,
      randomx_challenge_ttl_seconds: randomx
        .challenge_ttl
        .whole_seconds("pow.algorithms.randomx.challenge_ttl")?,
      auth_ttl_seconds: self.security.auth.ttl.whole_seconds("security.auth.ttl")?,
      auth_binding: self.security.auth.bind.clone(),
      auth_cookie_name: self.security.auth.cookie.clone(),
      nonce_offset: randomx.nonce_offset,
      verifier_queue_capacity: randomx.verifier_queue,
      randomx_verify: verify,
      randomx_cache_idle_timeout: randomx.cache.idle_timeout.0,
      rate_limit: RateLimitConfig {
        enabled: self.pow.rate_limit.enabled,
        max_clients: self.pow.rate_limit.max_clients,
        client_burst: self.pow.rate_limit.client.burst,
        client_refill_per_second: self.pow.rate_limit.client.refill_per_second,
        global_burst: self.pow.rate_limit.global.burst,
        global_refill_per_second: self.pow.rate_limit.global.refill_per_second,
      },
      stratum_pool,
      difficulty: DifficultyConfig {
        base: randomx.difficulty.base,
        weight_step: randomx.difficulty.score_step,
        max: randomx.difficulty.max,
      },
      policy: policy_document,
      browser: BrowserConfig {
        solver_module_path: randomx.browser.module.clone(),
        max_workers: randomx.browser.max_workers,
        store_cache: randomx.browser.store_cache,
        job_details: randomx.browser.job_details,
      },
    })
  }
}

fn valid_name(name: &str) -> bool {
  (1..=64).contains(&name.len())
    && name
      .bytes()
      .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_'))
}

fn validate_proxies(proxies: &[rilgate_pool::Proxy], path: &str) -> Result<()> {
  ensure!(proxies.len() <= 8, "{path}: expected at most 8 proxies");
  let mut names = std::collections::HashSet::new();
  for (index, proxy) in proxies.iter().enumerate() {
    ensure!(
      valid_name(&proxy.name) && names.insert(&proxy.name),
      "{path}[{index}].name: expected a unique name of 1 to 64 lowercase letters, digits, hyphens or underscores"
    );
    proxy
      .endpoint()
      .with_context(|| format!("{path}[{index}]"))?;
  }
  Ok(())
}
