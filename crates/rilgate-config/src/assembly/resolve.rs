use crate::{
  schema::{Document, pow::P2Pool, site::Seed},
  *,
};
use anyhow::{Context, Result, ensure};
use std::sync::Arc;

impl Document {
  pub fn resolve(&self) -> Result<Config> {
    let address = super::listen_address(&self.server.listen)?;
    ensure!(
      !self.server.upstream.url.is_empty(),
      "server.upstream.url: required; expected an HTTP or HTTPS upstream URL"
    );
    let (signing_key, auth_signing_key, signing_keys, auth_signing_keys) =
      match (&self.security.secret, &self.security.keys) {
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
        (None, None) => anyhow::bail!("security: set secret or both keys.challenge and keys.auth"),
      };
    let needs_pool = self.pow.default_algorithm == PowAlgorithm::RandomX
      || self.pow.verification == PowEnforcement::P2poolAccepted
      || self
        .policy
        .challenges()?
        .iter()
        .any(|(_, c)| c.algorithm.unwrap_or(self.pow.default_algorithm) == PowAlgorithm::RandomX);
    ensure!(
      !needs_pool || self.pow.p2pool.is_some(),
      "pow.p2pool.address: required when RandomX jobs or p2pool_accepted verification are enabled"
    );
    let fallback = P2Pool::default();
    let pool = self.pow.p2pool.as_ref().unwrap_or(&fallback);
    let url = url::Url::parse(&format!("tcp://{}", pool.address))
      .context("pow.p2pool.address: expected HOST:PORT or [IPv6]:PORT")?;
    ensure!(
      url.host_str().is_some()
        && url.port().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path().is_empty()
        && url.query().is_none()
        && url.fragment().is_none(),
      "pow.p2pool.address: expected HOST:PORT without credentials or a path"
    );
    let randomx = &self.pow.algorithms.randomx;
    let site = &self.site;
    let config = Arc::new(Configuration {
      env: self.environment,
      app_port: address.port(),
      listen_address: address,
      server: ServerConfig {
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
      web: WebConfig {
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
        i18n: rilgate_web::config::I18nConfig {
          default_locale: site.i18n.default.clone(),
          supported_locales: site.i18n.supported.clone(),
          strict: site.i18n.strict,
          language_cookie_name: site.i18n.cookie.clone(),
        },
      },
      pow: PowConfig {
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
        auth_cookie_name: self.security.auth.cookie.clone(),
        nonce_offset: randomx.nonce_offset,
        verifier_queue_capacity: randomx.verifier_queue,
        rate_limit: RateLimitConfig {
          enabled: self.pow.rate_limit.enabled,
          max_clients: self.pow.rate_limit.max_clients,
          client_burst: self.pow.rate_limit.client.burst,
          client_refill_per_second: self.pow.rate_limit.client.refill_per_second,
          global_burst: self.pow.rate_limit.global.burst,
          global_refill_per_second: self.pow.rate_limit.global.refill_per_second,
        },
        p2pool: P2PoolConfig {
          host: url.host_str().unwrap().trim_matches(['[', ']']).into(),
          port: url.port().unwrap(),
          password: pool.password.clone(),
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
        },
      },
    });
    config.validate()?;
    Ok(config)
  }

  pub fn display(&self, defaults: bool) -> Result<String> {
    let mut value = serde_yaml_ng::to_value(self)?;
    if !defaults {
      prune(&mut value, &serde_yaml_ng::to_value(Self::default())?);
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
