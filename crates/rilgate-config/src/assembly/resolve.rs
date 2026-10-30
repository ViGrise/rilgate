use crate::{
  schema::{Document, pow::StratumPool, site::Seed},
  *,
};
use anyhow::{Context, Result, ensure};
use std::sync::Arc;

impl Document {
  pub fn resolve(&self) -> Result<Config> {
    self
      .config_sources
      .result(self.resolve_config(), &self.sources)
  }

  fn resolve_config(&self) -> Result<Config> {
    ensure!(
      self.pow.algorithms.randomx.serve.is_none(),
      "pow.algorithms.randomx.serve: use rilgate verifier serve; combined mode is not supported"
    );
    let addresses = super::listen_addresses(&self.server.listen, "server.listen", 1)?;
    let operations_listen_addresses = super::listen_addresses(
      &self.server.operations.listen,
      "server.operations.listen",
      0,
    )?;
    for (index, address) in operations_listen_addresses.iter().enumerate() {
      ensure!(
        !addresses.contains(address),
        "{}: address {address} overlaps server.listen",
        super::listen_path(
          &self.server.operations.listen,
          index,
          "server.operations.listen"
        )
      );
    }
    ensure!(
      !self.server.upstream.url.is_empty(),
      "server.upstream.url: required; expected an HTTP or HTTPS upstream URL"
    );
    let mut secret_generated = false;
    let (signing_key, auth_signing_key, signing_keys, auth_signing_keys) =
      match (&self.security.secret, &self.security.keys) {
        (None, None) => {
          secret_generated = true;
          let secret = rilgate_crypto::Secret::generate()?;
          let (challenge, auth) = rilgate_crypto::derivation::signing_keys(&secret)?;
          (challenge, auth, Default::default(), Default::default())
        }
        (Some(secret), None) => {
          super::validate_secret(secret, "security.secret")?;
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
    let needs_pool = self.pow.default_algorithm == PowAlgorithm::RandomX
      || self.pow.verification == PowEnforcement::StratumPoolAccepted
      || self
        .policy
        .challenges()?
        .iter()
        .any(|(_, c)| c.algorithm.unwrap_or(self.pow.default_algorithm) == PowAlgorithm::RandomX);
    ensure!(
      !needs_pool || self.pow.stratum_pool.is_some(),
      "pow.stratum_pool.pools: required when RandomX jobs or stratum_pool_accepted verification are enabled"
    );
    let fallback = StratumPool::default();
    let pool = self.pow.stratum_pool.as_ref().unwrap_or(&fallback);
    validate_proxies(&pool.proxies, "pow.stratum_pool.proxies")?;
    let largest_difficulty = self
      .policy
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
    let randomx = &self.pow.algorithms.randomx;
    let mut verify = randomx.verify.clone();
    verify.remote.server_name = verify.remote.effective_server_name();
    let site = &self.site;
    let config = Arc::new(Configuration {
      env: self.environment,
      app_port: addresses[0].port(),
      listen_addresses: addresses,
      operations_listen_addresses,
      secret_generated,
      server: ServerConfig {
        operations_reload: self.server.operations.reload,
        watch_config: self.server.watch_config,
        max_connections: self.server.max_connections,
        max_requests: self.server.max_requests,
        h2_max_streams: self.server.h2_max_streams,
        trusted_proxies: self
          .server
          .trusted_proxies
          .iter()
          .enumerate()
          .map(|(index, net)| {
            net.parse().with_context(|| {
              format!("server.trusted_proxies[{index}]: expected an IPv4 or IPv6 CIDR")
            })
          })
          .collect::<Result<_>>()?,
        client_ip_header: self
          .server
          .client_ip_header
          .as_ref()
          .map(|name| name.parse())
          .transpose()
          .context("server.client_ip_header: expected an HTTP header name")?,
        decision_log: self.server.decision_log,
        metrics_enabled: self.server.metrics_enabled,
        readiness_timeout: self.server.readiness_timeout.0,
      },
      upstream: UpstreamConfig {
        url: self.server.upstream.url.clone(),
        preserve_host: self.server.upstream.preserve_host,
        max_body_bytes: self.server.upstream.max_body_size.0,
        connect_timeout: self.server.upstream.timeout.connect.0,
        read_timeout: self.server.upstream.timeout.read.0,
      },
      open_graph: site.open_graph.resolve()?,
      web: WebConfig {
        development: matches!(self.environment, crate::Environment::Development),
        name: site.name.clone(),
        website_url: site.url.clone(),
        base_path: site.base_path.clone(),
        stylesheet_path: site.stylesheet.clone(),
        root: site.resources.directory.clone(),
        seed: rilgate_web::config::SeedConfig {
          empty_directory: matches!(site.resources.seed, Seed::IfEmpty),
        },
        watch: site.resources.watch.clone(),
        templates: site.templates.clone(),
        responses: site.responses.clone(),
        i18n: rilgate_web::config::I18nConfig {
          default_locale: site.i18n.default.clone(),
          supported_locales: site.i18n.supported.clone(),
          strict: site.i18n.strict,
          language_cookie_name: site.i18n.cookie.clone(),
        },
      },
      pow: PowConfig {
        randomx_fallback: randomx
          .fallback
          .as_ref()
          .map(|fallback| {
            Ok::<_, anyhow::Error>(crate::model::pow::RandomXFallback {
              algorithm: fallback.algorithm.parse().map_err(|_| {
                anyhow::anyhow!(
                  "pow.algorithms.randomx.fallback.algorithm: expected fast or hashwx"
                )
              })?,
              difficulty: fallback.difficulty,
            })
          })
          .transpose()?,
        hashwx_enabled: self.pow.verification == PowEnforcement::LocalVerify
          && self.policy.challenges()?.iter().any(|(_, choice)| {
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
        enforce: self.pow.verification,
        default_algorithm: self.pow.default_algorithm,
        signing_key,
        auth_signing_key,
        signing_keys,
        auth_signing_keys,
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
        stratum_pool: StratumPoolConfig {
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
        },
        difficulty: DifficultyConfig {
          base: randomx.difficulty.base,
          weight_step: randomx.difficulty.score_step,
          max: randomx.difficulty.max,
        },
        policy: self.policy.clone(),
        browser: BrowserConfig {
          solver_module_path: randomx.browser.module.clone(),
          max_workers: randomx.browser.max_workers,
          store_cache: randomx.browser.store_cache,
        },
      },
    });
    config.validate()?;
    Ok(config)
  }

  pub fn display(&self, defaults: bool) -> Result<String> {
    let mut value = serde_yaml_ng::to_value(self)?;
    if !defaults {
      let defaults = Self::default();
      let mut baseline = serde_yaml_ng::to_value(&defaults)?;
      if self.site.open_graph.request_headers != defaults.site.open_graph.request_headers {
        // Header maps replace their defaults as a unit during deserialization.
        baseline["site"]["open_graph"]["request_headers"] = serde_yaml_ng::Value::Null;
      }
      prune(&mut value, &baseline);
    }
    Ok(serde_yaml_ng::to_string(&value)?)
  }
}
fn prune(value: &mut serde_yaml_ng::Value, default: &serde_yaml_ng::Value) {
  if let (Some(map), Some(default)) = (value.as_mapping_mut(), default.as_mapping()) {
    map.retain(|key, value| {
      if let Some(baseline) = default.get(key) {
        if value == baseline {
          return false;
        }
        prune(value, baseline);
      }
      true
    });
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
