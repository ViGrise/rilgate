//! Command-line flags and environment variables for scalar configuration settings.
//!
//! Each entry assigns one YAML path. A flag is the path with `.` and `_` turned
//! into `-`, and its variable is `RILGATE_` followed by the flag in upper snake
//! case. Values are validated by the configuration schema, so they accept the
//! same syntax as YAML values.
use clap::{Arg, ArgAction, ArgMatches, Args, Command, FromArgMatches};
use rilgate_config::{Setting, SettingValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
  Text,
  /// A text value whose default is never displayed.
  Secret,
  /// `--flag` means true; `--flag=false` sets false.
  Switch,
  /// Comma-separated or repeated values replacing the whole list.
  List,
}

pub(crate) struct Spec {
  pub(crate) path: &'static str,
  pub(crate) flag: &'static str,
  pub(crate) env: &'static str,
  pub(crate) short: Option<char>,
  pub(crate) kind: Kind,
  pub(crate) heading: &'static str,
  pub(crate) help: &'static str,
}

const ENVIRONMENT: &str = "Environment";
const SERVER: &str = "Server";
const SITE: &str = "Site";
const SECURITY: &str = "Security";
const POW: &str = "Proof of work";
const POOL: &str = "Stratum pool";

macro_rules! spec {
  ($heading:expr, $kind:ident, $path:literal, $flag:literal, $env:literal, $short:expr, $help:literal) => {
    Spec {
      path: $path,
      flag: $flag,
      env: $env,
      short: $short,
      kind: Kind::$kind,
      heading: $heading,
      help: $help,
    }
  };
}

#[rustfmt::skip]
pub(crate) const SPECS: &[Spec] = &[
  spec!(ENVIRONMENT, Text, "environment", "environment", "RILGATE_ENVIRONMENT", Some('E'), "Runtime environment: development or production"),
  spec!(SERVER, List, "server.listen", "server-listen", "RILGATE_SERVER_LISTEN", Some('l'), "Comma-separated listen addresses, such as :8080 or [::]:8080 (IPv4 and IPv6)"),
  spec!(SERVER, Switch, "server.operations.reload", "server-operations-reload", "RILGATE_SERVER_OPERATIONS_RELOAD", None, "Enable the operations reload route"),
  spec!(SERVER, Switch, "server.watch_config", "server-watch-config", "RILGATE_SERVER_WATCH_CONFIG", None, "Watch configuration files for signals and policy changes"),
  spec!(SERVER, List, "server.operations.listen", "server-operations-listen", "RILGATE_SERVER_OPERATIONS_LISTEN", None, "Comma-separated addresses serving only operational routes, such as 127.0.0.1:9090; empty disables the routes"),
  spec!(SERVER, Text, "server.upstream.url", "server-upstream-url", "RILGATE_SERVER_UPSTREAM_URL", Some('u'), "Application upstream URL"),
  spec!(SERVER, Switch, "server.upstream.preserve_host", "server-upstream-preserve-host", "RILGATE_SERVER_UPSTREAM_PRESERVE_HOST", None, "Forward the client Host header upstream"),
  spec!(SERVER, Text, "server.upstream.max_body_size", "server-upstream-max-body-size", "RILGATE_SERVER_UPSTREAM_MAX_BODY_SIZE", None, "Largest forwarded request body, such as 16MiB"),
  spec!(SERVER, List, "server.upstream.client_ip_headers", "server-upstream-client-ip-headers", "RILGATE_SERVER_UPSTREAM_CLIENT_IP_HEADERS", None, "Headers that carry the resolved client address to the upstream"),
  spec!(SERVER, Text, "server.upstream.timeout.connect", "server-upstream-timeout-connect", "RILGATE_SERVER_UPSTREAM_TIMEOUT_CONNECT", None, "Upstream connect timeout"),
  spec!(SERVER, Text, "server.upstream.timeout.read", "server-upstream-timeout-read", "RILGATE_SERVER_UPSTREAM_TIMEOUT_READ", None, "Upstream read timeout"),
  spec!(SERVER, Text, "server.max_connections", "server-max-connections", "RILGATE_SERVER_MAX_CONNECTIONS", None, "Maximum concurrent client connections"),
  spec!(SERVER, Text, "server.max_requests", "server-max-requests", "RILGATE_SERVER_MAX_REQUESTS", None, "Maximum concurrent requests"),
  spec!(SERVER, Text, "server.h2_max_streams", "server-h2-max-streams", "RILGATE_SERVER_H2_MAX_STREAMS", None, "Maximum concurrent HTTP/2 streams per connection"),
  spec!(SERVER, List, "server.trusted_proxies", "server-trusted-proxies", "RILGATE_SERVER_TRUSTED_PROXIES", None, "Proxy networks allowed to supply forwarding headers"),
  spec!(SERVER, Text, "server.client_ip_header", "server-client-ip-header", "RILGATE_SERVER_CLIENT_IP_HEADER", None, "Header containing one client IP address from a trusted proxy"),
  spec!(SERVER, Text, "server.decision_log", "server-decision-log", "RILGATE_SERVER_DECISION_LOG", None, "Policy decision logs at info level: off, deny, challenge or all"),
  spec!(SERVER, Switch, "server.metrics_enabled", "server-metrics-enabled", "RILGATE_SERVER_METRICS_ENABLED", None, "Expose Prometheus metrics"),
  spec!(SERVER, Text, "server.readiness_timeout", "server-readiness-timeout", "RILGATE_SERVER_READINESS_TIMEOUT", None, "Dependency timeout for readiness probes"),
  spec!(SERVER, Text, "server.request_timeout", "server-request-timeout", "RILGATE_SERVER_REQUEST_TIMEOUT", None, "Time limit to receive a request and connect to the upstream"),
  spec!(SITE, Text, "site.name", "site-name", "RILGATE_SITE_NAME", None, "Site name shown on challenge pages"),
  spec!(SITE, Text, "site.url", "site-url", "RILGATE_SITE_URL", None, "Site URL linked from challenge pages"),
  spec!(SITE, Text, "site.base_path", "site-base-path", "RILGATE_SITE_BASE_PATH", None, "Path prefix of the internal namespace"),
  spec!(SITE, Switch, "site.content_security_policy.enabled", "site-content-security-policy-enabled", "RILGATE_SITE_CONTENT_SECURITY_POLICY_ENABLED", None, "Send the generated page Content Security Policy"),
  spec!(SITE, Text, "site.stylesheet", "site-stylesheet", "RILGATE_SITE_STYLESHEET", None, "Stylesheet path under static resources or absolute HTTP/HTTPS URL"),
  spec!(SITE, Text, "site.resources.directory", "site-resources-directory", "RILGATE_SITE_RESOURCES_DIRECTORY", None, "Directory with template, translation and static overrides"),
  spec!(SITE, Text, "site.resources.seed", "site-resources-seed", "RILGATE_SITE_RESOURCES_SEED", None, "Seed the resources directory: never or if_empty"),
  spec!(SITE, Switch, "site.resources.watch.enabled", "site-resources-watch-enabled", "RILGATE_SITE_RESOURCES_WATCH_ENABLED", None, "Reload resources when the directory changes"),
  spec!(SITE, Text, "site.resources.watch.debounce", "site-resources-watch-debounce", "RILGATE_SITE_RESOURCES_WATCH_DEBOUNCE", None, "Delay before reloading changed resources"),
  spec!(SITE, Switch, "site.templates.cache", "site-templates-cache", "RILGATE_SITE_TEMPLATES_CACHE", None, "Cache compiled templates"),
  spec!(SITE, Switch, "site.templates.strict", "site-templates-strict", "RILGATE_SITE_TEMPLATES_STRICT", None, "Fail on undefined template variables"),
  spec!(SITE, Text, "site.templates.recursion_limit", "site-templates-recursion-limit", "RILGATE_SITE_TEMPLATES_RECURSION_LIMIT", None, "Template recursion limit"),
  spec!(SITE, Text, "site.templates.fuel", "site-templates-fuel", "RILGATE_SITE_TEMPLATES_FUEL", None, "Template execution fuel per render"),
  spec!(SITE, Text, "site.responses.format", "site-responses-format", "RILGATE_SITE_RESPONSES_FORMAT", None, "Challenge response format selection"),
  spec!(SITE, Text, "site.responses.challenge_status", "site-responses-challenge-status", "RILGATE_SITE_RESPONSES_CHALLENGE_STATUS", None, "HTTP status for HTML challenge responses"),
  spec!(SITE, Text, "site.responses.deny_status", "site-responses-deny-status", "RILGATE_SITE_RESPONSES_DENY_STATUS", None, "HTTP status for HTML and JSON denial responses"),
  spec!(SITE, List, "site.responses.json_paths", "site-responses-json-paths", "RILGATE_SITE_RESPONSES_JSON_PATHS", None, "Path prefixes that always receive JSON responses"),
  spec!(SITE, Text, "site.i18n.default", "site-i18n-default", "RILGATE_SITE_I18N_DEFAULT", None, "Default locale"),
  spec!(SITE, List, "site.i18n.supported", "site-i18n-supported", "RILGATE_SITE_I18N_SUPPORTED", None, "Supported locales; empty discovers them from resources"),
  spec!(SITE, Switch, "site.i18n.strict", "site-i18n-strict", "RILGATE_SITE_I18N_STRICT", None, "Fail on missing translations"),
  spec!(SITE, Text, "site.i18n.cookie", "site-i18n-cookie", "RILGATE_SITE_I18N_COOKIE", None, "Locale cookie name"),
  spec!(SECURITY, Secret, "security.secret", "security-secret", "RILGATE_SECURITY_SECRET", Some('s'), "Signing secret of at least 32 bytes; prefer the variable over the flag"),
  spec!(SECURITY, Text, "security.auth.cookie", "security-auth-cookie", "RILGATE_SECURITY_AUTH_COOKIE", None, "Authentication cookie name"),
  spec!(SECURITY, Text, "security.auth.ttl", "security-auth-ttl", "RILGATE_SECURITY_AUTH_TTL", None, "Authentication cookie lifetime"),
  spec!(SECURITY, Text, "security.auth.bind.client_ip.max_networks", "security-auth-bind-client-ip-max-networks", "RILGATE_SECURITY_AUTH_BIND_CLIENT_IP_MAX_NETWORKS", None, "Maximum client networks remembered in an access cookie (1 to 8; default 1)"),
  spec!(SECURITY, Text, "security.auth.bind.client_ip.ipv4_prefix", "security-auth-bind-client-ip-ipv4-prefix", "RILGATE_SECURITY_AUTH_BIND_CLIENT_IP_IPV4_PREFIX", None, "IPv4 prefix length the cookie is bound to; setting either prefix enables client IP binding"),
  spec!(SECURITY, Text, "security.auth.bind.client_ip.ipv6_prefix", "security-auth-bind-client-ip-ipv6-prefix", "RILGATE_SECURITY_AUTH_BIND_CLIENT_IP_IPV6_PREFIX", None, "IPv6 prefix length the cookie is bound to; setting either prefix enables client IP binding"),
  spec!(SECURITY, Text, "security.auth.bind.user_agent", "security-auth-bind-user-agent", "RILGATE_SECURITY_AUTH_BIND_USER_AGENT", None, "Bind the cookie to the User-Agent: full or family"),
  spec!(SECURITY, List, "security.auth.bind.headers", "security-auth-bind-headers", "RILGATE_SECURITY_AUTH_BIND_HEADERS", None, "Client request headers the cookie is bound to"),
  spec!(SECURITY, List, "security.auth.bind.proxy_headers", "security-auth-bind-proxy-headers", "RILGATE_SECURITY_AUTH_BIND_PROXY_HEADERS", None, "Headers set by trusted proxies the cookie is bound to"),
  spec!(POW, Text, "pow.default_algorithm", "pow-default-algorithm", "RILGATE_POW_DEFAULT_ALGORITHM", None, "Algorithm for challenges without one: fast, hashwx or randomx"),
  spec!(POW, Text, "pow.challenge.ttl", "pow-challenge-ttl", "RILGATE_POW_CHALLENGE_TTL", None, "Challenge lifetime"),
  spec!(POW, Text, "pow.challenge.receipt_ttl", "pow-challenge-receipt-ttl", "RILGATE_POW_CHALLENGE_RECEIPT_TTL", None, "Proof receipt lifetime"),
  spec!(POW, Switch, "pow.rate_limit.enabled", "pow-rate-limit-enabled", "RILGATE_POW_RATE_LIMIT_ENABLED", None, "Rate limit proof API requests"),
  spec!(POW, Text, "pow.rate_limit.max_clients", "pow-rate-limit-max-clients", "RILGATE_POW_RATE_LIMIT_MAX_CLIENTS", None, "Tracked rate limit clients"),
  spec!(POW, Text, "pow.rate_limit.client.burst", "pow-rate-limit-client-burst", "RILGATE_POW_RATE_LIMIT_CLIENT_BURST", None, "Proof requests a client may burst"),
  spec!(POW, Text, "pow.rate_limit.client.refill_per_second", "pow-rate-limit-client-refill-per-second", "RILGATE_POW_RATE_LIMIT_CLIENT_REFILL_PER_SECOND", None, "Client proof request refill rate"),
  spec!(POW, Text, "pow.rate_limit.global.burst", "pow-rate-limit-global-burst", "RILGATE_POW_RATE_LIMIT_GLOBAL_BURST", None, "Proof requests all clients may burst"),
  spec!(POW, Text, "pow.rate_limit.global.refill_per_second", "pow-rate-limit-global-refill-per-second", "RILGATE_POW_RATE_LIMIT_GLOBAL_REFILL_PER_SECOND", None, "Global proof request refill rate"),
  spec!(POW, Text, "pow.algorithms.hashwx.nonces_per_seed", "pow-algorithms-hashwx-nonces-per-seed", "RILGATE_POW_ALGORITHMS_HASHWX_NONCES_PER_SEED", None, "HashWX nonces per seed: 463 to 1048576"),
  spec!(POW, Text, "pow.algorithms.hashwx.difficulty.base", "pow-algorithms-hashwx-difficulty-base", "RILGATE_POW_ALGORITHMS_HASHWX_DIFFICULTY_BASE", None, "HashWX base difficulty: 1 to 2^40; provisional default"),
  spec!(POW, Text, "pow.algorithms.hashwx.difficulty.score_step", "pow-algorithms-hashwx-difficulty-score-step", "RILGATE_POW_ALGORITHMS_HASHWX_DIFFICULTY_SCORE_STEP", None, "Positive score interval that doubles HashWX difficulty"),
  spec!(POW, Text, "pow.algorithms.hashwx.difficulty.max", "pow-algorithms-hashwx-difficulty-max", "RILGATE_POW_ALGORITHMS_HASHWX_DIFFICULTY_MAX", None, "Maximum mapped HashWX difficulty, from base to 2^40; provisional default"),
  spec!(POW, Text, "pow.algorithms.randomx.fallback.algorithm", "pow-algorithms-randomx-fallback-algorithm", "RILGATE_POW_ALGORITHMS_RANDOMX_FALLBACK_ALGORITHM", None, "Fallback algorithm when every Stratum pool is unavailable: fast or hashwx"),
  spec!(POW, Text, "pow.algorithms.randomx.fallback.difficulty", "pow-algorithms-randomx-fallback-difficulty", "RILGATE_POW_ALGORITHMS_RANDOMX_FALLBACK_DIFFICULTY", None, "Fallback challenge difficulty"),
  spec!(POW, Text, "pow.algorithms.randomx.challenge_ttl", "pow-algorithms-randomx-challenge-ttl", "RILGATE_POW_ALGORITHMS_RANDOMX_CHALLENGE_TTL", None, "RandomX challenge lifetime"),
  spec!(POW, Text, "pow.algorithms.randomx.nonce_offset", "pow-algorithms-randomx-nonce-offset", "RILGATE_POW_ALGORITHMS_RANDOMX_NONCE_OFFSET", None, "Nonce byte offset in the RandomX blob"),
  spec!(POW, Text, "pow.algorithms.randomx.verifier_queue", "pow-algorithms-randomx-verifier-queue", "RILGATE_POW_ALGORITHMS_RANDOMX_VERIFIER_QUEUE", None, "Queued RandomX verifications"),
  spec!(POW, Text, "pow.algorithms.randomx.cache.idle_timeout", "pow-algorithms-randomx-cache-idle-timeout", "RILGATE_POW_ALGORITHMS_RANDOMX_CACHE_IDLE_TIMEOUT", None, "Idle time before releasing the RandomX cache; 0s keeps it resident"),
  spec!(POW, Text, "pow.algorithms.randomx.verify.mode", "pow-algorithms-randomx-verify-mode", "RILGATE_POW_ALGORITHMS_RANDOMX_VERIFY_MODE", None, "RandomX proof check: gateway or pool_share"),
  spec!(POW, List, "pow.algorithms.randomx.verify.order", "pow-algorithms-randomx-verify-order", "RILGATE_POW_ALGORITHMS_RANDOMX_VERIFY_ORDER", None, "Ordered RandomX hashers: local, remote"),
  spec!(POW, Text, "pow.algorithms.randomx.verify.remote.endpoint", "pow-algorithms-randomx-verify-remote-endpoint", "RILGATE_POW_ALGORITHMS_RANDOMX_VERIFY_REMOTE_ENDPOINT", None, "Remote verifier HTTP or HTTPS origin"),
  spec!(POW, Text, "pow.algorithms.randomx.verify.remote.server_name", "pow-algorithms-randomx-verify-remote-server-name", "RILGATE_POW_ALGORITHMS_RANDOMX_VERIFY_REMOTE_SERVER_NAME", None, "Remote verifier certificate name; defaults to endpoint host"),
  spec!(POW, Text, "pow.algorithms.randomx.verify.remote.timeout", "pow-algorithms-randomx-verify-remote-timeout", "RILGATE_POW_ALGORITHMS_RANDOMX_VERIFY_REMOTE_TIMEOUT", None, "Remote hash request timeout"),
  spec!(POW, Text, "pow.algorithms.randomx.verify.remote.on_deadline_rejected", "pow-algorithms-randomx-verify-remote-on-deadline-rejected", "RILGATE_POW_ALGORITHMS_RANDOMX_VERIFY_REMOTE_ON_DEADLINE_REJECTED", None, "Rejected deadline policy: fallback or reject"),
  spec!(POW, Text, "pow.algorithms.randomx.verify.remote.tls.ca", "pow-algorithms-randomx-verify-remote-tls-ca", "RILGATE_POW_ALGORITHMS_RANDOMX_VERIFY_REMOTE_TLS_CA", None, "Private CA PEM file for the remote verifier"),
  spec!(POW, Text, "pow.algorithms.randomx.verify.remote.tls.client_cert", "pow-algorithms-randomx-verify-remote-tls-client-cert", "RILGATE_POW_ALGORITHMS_RANDOMX_VERIFY_REMOTE_TLS_CLIENT_CERT", None, "Gateway client certificate PEM file"),
  spec!(POW, Text, "pow.algorithms.randomx.verify.remote.tls.client_key", "pow-algorithms-randomx-verify-remote-tls-client-key", "RILGATE_POW_ALGORITHMS_RANDOMX_VERIFY_REMOTE_TLS_CLIENT_KEY", None, "Gateway client private key PEM file"),
  spec!(POW, Text, "pow.algorithms.randomx.serve.listen", "pow-algorithms-randomx-serve-listen", "RILGATE_POW_ALGORITHMS_RANDOMX_SERVE_LISTEN", None, "Dedicated verifier TLS listen address"),
  spec!(POW, Text, "pow.algorithms.randomx.serve.workers", "pow-algorithms-randomx-serve-workers", "RILGATE_POW_ALGORITHMS_RANDOMX_SERVE_WORKERS", None, "Verifier worker count; must be 1"),
  spec!(POW, Text, "pow.algorithms.randomx.serve.queue", "pow-algorithms-randomx-serve-queue", "RILGATE_POW_ALGORITHMS_RANDOMX_SERVE_QUEUE", None, "Verifier admission limit, from 1 to 4096"),
  spec!(POW, Text, "pow.algorithms.randomx.serve.max_connections", "pow-algorithms-randomx-serve-max-connections", "RILGATE_POW_ALGORITHMS_RANDOMX_SERVE_MAX_CONNECTIONS", None, "Maximum verifier connections, from 1 to 65536"),
  spec!(POW, Text, "pow.algorithms.randomx.serve.tls.cert", "pow-algorithms-randomx-serve-tls-cert", "RILGATE_POW_ALGORITHMS_RANDOMX_SERVE_TLS_CERT", None, "Verifier TLS certificate PEM file"),
  spec!(POW, Text, "pow.algorithms.randomx.serve.tls.key", "pow-algorithms-randomx-serve-tls-key", "RILGATE_POW_ALGORITHMS_RANDOMX_SERVE_TLS_KEY", None, "Verifier TLS private key PEM file"),
  spec!(POW, Text, "pow.algorithms.randomx.serve.tls.client_ca", "pow-algorithms-randomx-serve-tls-client-ca", "RILGATE_POW_ALGORITHMS_RANDOMX_SERVE_TLS_CLIENT_CA", None, "CA file for gateway client certificates"),
  spec!(POW, Text, "pow.algorithms.randomx.difficulty.base", "pow-algorithms-randomx-difficulty-base", "RILGATE_POW_ALGORITHMS_RANDOMX_DIFFICULTY_BASE", None, "RandomX difficulty at score 0"),
  spec!(POW, Text, "pow.algorithms.randomx.difficulty.score_step", "pow-algorithms-randomx-difficulty-score-step", "RILGATE_POW_ALGORITHMS_RANDOMX_DIFFICULTY_SCORE_STEP", None, "Score that doubles RandomX difficulty"),
  spec!(POW, Text, "pow.algorithms.randomx.difficulty.max", "pow-algorithms-randomx-difficulty-max", "RILGATE_POW_ALGORITHMS_RANDOMX_DIFFICULTY_MAX", None, "Highest RandomX difficulty"),
  spec!(POW, Text, "pow.algorithms.randomx.browser.module", "pow-algorithms-randomx-browser-module", "RILGATE_POW_ALGORITHMS_RANDOMX_BROWSER_MODULE", None, "Browser RandomX solver module path"),
  spec!(POW, Text, "pow.algorithms.randomx.browser.max_workers", "pow-algorithms-randomx-browser-max-workers", "RILGATE_POW_ALGORITHMS_RANDOMX_BROWSER_MAX_WORKERS", None, "Maximum browser RandomX workers (1 to 8)"),
  spec!(POW, Switch, "pow.algorithms.randomx.browser.store_cache", "pow-algorithms-randomx-browser-store-cache", "RILGATE_POW_ALGORITHMS_RANDOMX_BROWSER_STORE_CACHE", None, "Store the browser RandomX cache in IndexedDB"),
  spec!(POW, Switch, "pow.algorithms.randomx.browser.job_details", "pow-algorithms-randomx-browser-job-details", "RILGATE_POW_ALGORITHMS_RANDOMX_BROWSER_JOB_DETAILS", None, "Show the RandomX job, block and seed on the challenge page"),
  spec!(POOL, Text, "pow.stratum_pool.failover.cooldown", "pow-stratum-pool-failover-cooldown", "RILGATE_POW_STRATUM_POOL_FAILOVER_COOLDOWN", None, "Pool and proxy connection failure cooldown"),
  spec!(POOL, Text, "pow.stratum_pool.failover.timeout", "pow-stratum-pool-failover-timeout", "RILGATE_POW_STRATUM_POOL_FAILOVER_TIMEOUT", None, "Total Stratum pool connection timeout"),
  spec!(POOL, Text, "pow.stratum_pool.agent", "pow-stratum-pool-agent", "RILGATE_POW_STRATUM_POOL_AGENT", None, "Stratum user agent"),
  spec!(POOL, Text, "pow.stratum_pool.timeout.connect", "pow-stratum-pool-timeout-connect", "RILGATE_POW_STRATUM_POOL_TIMEOUT_CONNECT", None, "Stratum pool connect timeout"),
  spec!(POOL, Text, "pow.stratum_pool.timeout.submit", "pow-stratum-pool-timeout-submit", "RILGATE_POW_STRATUM_POOL_TIMEOUT_SUBMIT", None, "Stratum pool share submit timeout"),
  spec!(POOL, Text, "pow.stratum_pool.sessions.max", "pow-stratum-pool-sessions-max", "RILGATE_POW_STRATUM_POOL_SESSIONS_MAX", None, "Concurrent browser mining sessions"),
  spec!(POOL, Text, "pow.stratum_pool.sessions.ttl", "pow-stratum-pool-sessions-ttl", "RILGATE_POW_STRATUM_POOL_SESSIONS_TTL", None, "Browser mining session lifetime"),
  spec!(POOL, Text, "pow.stratum_pool.job_sessions.mode", "pow-stratum-pool-job-sessions-mode", "RILGATE_POW_STRATUM_POOL_JOB_SESSIONS_MODE", None, "Job connections: pooled or per_challenge"),
  spec!(POOL, Text, "pow.stratum_pool.job_sessions.max", "pow-stratum-pool-job-sessions-max", "RILGATE_POW_STRATUM_POOL_JOB_SESSIONS_MAX", None, "Pooled job connections"),
  spec!(POOL, Text, "pow.stratum_pool.job_sessions.idle", "pow-stratum-pool-job-sessions-idle", "RILGATE_POW_STRATUM_POOL_JOB_SESSIONS_IDLE", None, "Idle time before a pooled job connection closes"),
];

pub(crate) const POOL_FIELDS: &[(&str, &str)] = &[
  ("NAME", "name"),
  ("ADDRESS", "address"),
  ("LOGIN_USER", "login.user"),
  ("LOGIN_PASSWORD", "login.password"),
  ("TLS_ENABLED", "tls.enabled"),
  ("TLS_SERVER_NAME", "tls.server_name"),
  ("TLS_CA", "tls.ca"),
  ("PROXIES", "proxies"),
];

pub(crate) const PROXY_FIELDS: &[(&str, &str)] = &[
  ("NAME", "name"),
  ("URL", "url"),
  ("USERNAME", "username"),
  ("PASSWORD", "password"),
];

fn indexed_item<'a>(name: &str, rest: &'a str) -> Result<(usize, &'a str), String> {
  let (index, field) = rest
    .split_once('_')
    .ok_or_else(|| format!("{name}: expected an index and a field"))?;
  let index: usize = index
    .parse()
    .map_err(|_| format!("{name}: expected a numeric index"))?;
  if index >= 8 {
    return Err(format!("{name}: index must be 0 to 7"));
  }
  Ok((index, field))
}

fn indexed_field(name: &str, field: &str, fields: &[(&str, &str)]) -> Result<String, String> {
  fields
    .iter()
    .find(|(key, _)| *key == field)
    .map(|(_, path)| path.to_string())
    .ok_or_else(|| {
      format!(
        "{name}: unknown field; valid fields: {}",
        fields
          .iter()
          .map(|(key, _)| *key)
          .collect::<Vec<_>>()
          .join(", ")
      )
    })
}

fn indexed_settings(
  variables: impl Iterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> Result<Vec<Setting>, String> {
  let mut settings = Vec::new();
  for (name, value) in variables {
    let Some(name) = name.to_str() else {
      continue;
    };
    let Some(rest) = name.strip_prefix("RILGATE_POW_STRATUM_POOL_") else {
      continue;
    };
    let path = if let Some(rest) = rest.strip_prefix("POOLS_") {
      let (index, field) = indexed_item(name, rest)?;
      let field = if let Some(rest) = field.strip_prefix("PROXIES_") {
        let (proxy, field) = indexed_item(name, rest)?;
        format!(
          "proxies.{proxy}.{}",
          indexed_field(name, field, PROXY_FIELDS)?
        )
      } else {
        indexed_field(name, field, POOL_FIELDS)?
      };
      format!("pow.stratum_pool.pools.{index}.{field}")
    } else if let Some(rest) = rest.strip_prefix("PROXIES_") {
      let (index, field) = indexed_item(name, rest)?;
      format!(
        "pow.stratum_pool.proxies.{index}.{}",
        indexed_field(name, field, PROXY_FIELDS)?
      )
    } else {
      continue;
    };
    let value = value
      .into_string()
      .map_err(|_| format!("{name}: expected a UTF-8 value"))?;
    let value = if path.ends_with(".proxies") {
      if !value.is_empty() {
        return Err(format!(
          "{name}: use an empty value for a direct connection, or indexed proxy fields"
        ));
      }
      SettingValue::List(Vec::new())
    } else {
      SettingValue::Scalar(value)
    };
    settings.push(Setting {
      path,
      source: name.into(),
      value,
    });
  }
  settings.sort_by(|a, b| a.path.cmp(&b.path));
  Ok(settings)
}

/// The flag values present on the command line or in the environment.
#[derive(Debug, Default)]
pub struct Settings {
  values: Vec<Setting>,
}

impl Settings {
  pub fn into_settings(self) -> Vec<Setting> {
    self.values
  }
}

impl Args for Settings {
  fn augment_args(command: Command) -> Command {
    let defaults = defaults();
    command.args(SPECS.iter().map(|spec| {
      let mut help = format!("{} [{}]", spec.help, spec.path);
      if let Some(value) = defaults
        .as_ref()
        .filter(|_| spec.kind != Kind::Secret)
        .and_then(|d| default_text(d, spec.path))
      {
        help.push_str(&format!(" [default: {value}]"));
      }
      let arg = Arg::new(spec.flag)
        .long(spec.flag)
        .env(spec.env)
        .hide_env_values(true)
        .help(help)
        .help_heading(spec.heading)
        .global(true)
        .value_name(match spec.kind {
          Kind::Switch => "BOOL",
          Kind::List => "LIST",
          Kind::Text | Kind::Secret => "VALUE",
        });
      let arg = match spec.short {
        Some(short) => arg.short(short),
        None => arg,
      };
      match spec.kind {
        Kind::Text | Kind::Secret => arg.action(ArgAction::Set),
        Kind::Switch => arg
          .action(ArgAction::Set)
          .num_args(0..=1)
          .require_equals(true)
          .default_missing_value("true")
          .value_parser(["true", "false"]),
        Kind::List => arg.action(ArgAction::Append).value_delimiter(','),
      }
    }))
  }
  fn augment_args_for_update(command: Command) -> Command {
    Self::augment_args(command)
  }
}

impl FromArgMatches for Settings {
  fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
    let mut values = Vec::new();
    for spec in SPECS {
      let value = match spec.kind {
        Kind::List => matches.get_many::<String>(spec.flag).map(|items| {
          SettingValue::List(items.filter(|item| !item.is_empty()).cloned().collect())
        }),
        Kind::Text | Kind::Secret | Kind::Switch => matches
          .get_one::<String>(spec.flag)
          .map(|value| SettingValue::Scalar(value.clone())),
      };
      if let Some(value) = value {
        let source = match matches.value_source(spec.flag) {
          Some(clap::parser::ValueSource::EnvVariable) => spec.env.to_string(),
          _ => format!("--{}", spec.flag),
        };
        values.push(Setting {
          path: spec.path.into(),
          source,
          value,
        });
      }
    }
    values.extend(
      indexed_settings(std::env::vars_os())
        .map_err(|message| clap::Error::raw(clap::error::ErrorKind::ValueValidation, message))?,
    );
    Ok(Self { values })
  }
  fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
    *self = Self::from_arg_matches(matches)?;
    Ok(())
  }
}

/// A document that enables the optional sections whose defaults are shown.
const OPTIONAL_SECTIONS: &str = "pow:\n  algorithms:\n    randomx:\n      serve: {}\n  stratum_pool: {}\nsecurity:\n  auth:\n    bind:\n      client_ip: {}\n";

/// The schema defaults, with the optional sections present so their defaults
/// are shown too.
pub(crate) fn defaults() -> Option<serde_yaml_ng::Value> {
  defaults_of(OPTIONAL_SECTIONS)
}

fn defaults_of(document: &str) -> Option<serde_yaml_ng::Value> {
  serde_yaml_ng::to_value(rilgate_config::loader::parse(document).ok()?).ok()
}

pub(crate) fn default_text(root: &serde_yaml_ng::Value, path: &str) -> Option<String> {
  use serde_yaml_ng::Value;
  let value = path.split('.').try_fold(root, |node, key| node.get(key))?;
  match value {
    Value::String(text) if !text.is_empty() => Some(text.clone()),
    Value::Bool(value) => Some(value.to_string()),
    Value::Number(value) => Some(value.to_string()),
    Value::Sequence(items) if !items.is_empty() => Some(
      items
        .iter()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect::<Vec<_>>()
        .join(","),
    ),
    _ => None,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn flags_and_variables_follow_the_naming_rule() {
    for spec in SPECS {
      assert_eq!(
        spec.flag,
        spec.path.replace(['.', '_'], "-"),
        "{}",
        spec.path
      );
      assert_eq!(
        spec.env,
        format!("RILGATE_{}", spec.flag.replace('-', "_").to_uppercase()),
        "{}",
        spec.path
      );
    }
  }

  #[test]
  fn every_path_exists_in_the_schema() {
    let defaults = defaults_of(
      "server:\n  client_ip_header: cf-connecting-ip\nsecurity:\n  secret: placeholder\n  auth:\n    bind:\n      client_ip: {}\n      user_agent: family\npow:\n  algorithms:\n    randomx:\n      serve: {}\n      fallback:\n        algorithm: fast\n        difficulty: 4\n  stratum_pool: {}\n",
    )
    .unwrap();
    for spec in SPECS {
      assert!(
        spec
          .path
          .split('.')
          .try_fold(&defaults, |node, key| node.get(key))
          .is_some(),
        "{} is not a configuration path",
        spec.path
      );
    }
  }

  #[test]
  fn remote_order_and_endpoint_flags_parse_lists_and_origins() {
    use clap::Parser;
    #[derive(Parser)]
    struct CommandLine {
      #[command(flatten)]
      settings: Settings,
    }
    let args = CommandLine::try_parse_from([
      "rilgate",
      "--pow-algorithms-randomx-verify-order",
      "remote,local",
      "--pow-algorithms-randomx-verify-remote-endpoint",
      "https://flag.test:9443",
    ])
    .unwrap();
    let values = args.settings.into_settings();
    assert_eq!(
      values
        .iter()
        .find(|v| v.path.ends_with("verify.order"))
        .unwrap()
        .value,
      SettingValue::List(vec!["remote".into(), "local".into()])
    );
    assert_eq!(
      values
        .iter()
        .find(|v| v.path.ends_with("remote.endpoint"))
        .unwrap()
        .value,
      SettingValue::Scalar("https://flag.test:9443".into())
    );
  }

  #[test]
  fn flags_are_unique() {
    let mut flags: Vec<_> = SPECS.iter().map(|spec| spec.flag).collect();
    flags.sort_unstable();
    flags.dedup();
    assert_eq!(flags.len(), SPECS.len());
  }
}

#[cfg(test)]
mod indexed_tests {
  use super::*;

  #[test]
  fn mistyped_indexed_fields_fail_instead_of_silently_using_a_default() {
    let error = indexed_settings(
      [(
        "RILGATE_POW_STRATUM_POOL_POOLS_0_ADRESS".into(),
        "secret".into(),
      )]
      .into_iter(),
    )
    .unwrap_err();
    assert!(error.contains("RILGATE_POW_STRATUM_POOL_POOLS_0_ADRESS"));
    assert!(error.contains("valid fields: NAME, ADDRESS, LOGIN_USER, LOGIN_PASSWORD"));
    assert!(!error.contains("secret"));
  }
}

#[cfg(test)]
mod proxy_indexed_tests {
  use super::*;

  #[test]
  fn indexed_proxy_override_can_clear_inheritance_and_build_a_nested_route() {
    let values = indexed_settings(
      [
        ("RILGATE_POW_STRATUM_POOL_POOLS_0_PROXIES".into(), "".into()),
        (
          "RILGATE_POW_STRATUM_POOL_POOLS_1_PROXIES_0_URL".into(),
          "socks5h://localhost:1080".into(),
        ),
        (
          "RILGATE_POW_STRATUM_POOL_PROXIES_0_NAME".into(),
          "default".into(),
        ),
      ]
      .into_iter(),
    )
    .unwrap();
    assert!(
      values
        .iter()
        .any(|setting| setting.path == "pow.stratum_pool.pools.0.proxies"
          && setting.value == SettingValue::List(Vec::new()))
    );
    assert!(
      values
        .iter()
        .any(|setting| setting.path == "pow.stratum_pool.pools.1.proxies.0.url")
    );
    assert!(
      values
        .iter()
        .any(|setting| setting.path == "pow.stratum_pool.proxies.0.name")
    );
    let error = indexed_settings(
      [(
        "RILGATE_POW_STRATUM_POOL_POOLS_0_PROXIES_0_URI".into(),
        "secret".into(),
      )]
      .into_iter(),
    )
    .unwrap_err();
    assert!(error.contains("valid fields: NAME, URL, USERNAME, PASSWORD"));
  }
}

#[cfg(all(test, unix))]
mod environment_encoding_tests {
  use super::*;
  use std::{ffi::OsString, os::unix::ffi::OsStringExt};

  #[test]
  fn unrelated_non_utf8_environment_entries_are_ignored() {
    let invalid = OsString::from_vec(vec![0xff]);
    assert!(
      indexed_settings(
        [
          (invalid.clone(), invalid.clone()),
          (OsString::from("UNRELATED"), invalid.clone()),
          (OsString::from("RILGATE_POW_STRATUM_POOL_AGENT"), invalid),
        ]
        .into_iter()
      )
      .unwrap()
      .is_empty()
    );
  }

  #[test]
  fn indexed_non_utf8_values_report_the_variable() {
    let name = "RILGATE_POW_STRATUM_POOL_POOLS_0_LOGIN_PASSWORD";
    let error =
      indexed_settings([(OsString::from(name), OsString::from_vec(vec![0xff]))].into_iter())
        .unwrap_err();
    assert!(error.contains(name), "{error}");
    assert!(error.contains("UTF-8"), "{error}");
  }
}

#[cfg(test)]
mod hashwx_tests {
  use super::*;
  use clap::Parser;
  use rilgate_pow::{Algorithm, Work, WorkRequest};

  #[tokio::test]
  async fn hashwx_flags_change_issued_batches_and_score_mapping() {
    #[derive(Parser)]
    struct CommandLine {
      #[command(flatten)]
      settings: Settings,
    }
    let command = CommandLine::try_parse_from([
      "rilgate",
      "--pow-default-algorithm",
      "hashwx",
      "--pow-algorithms-hashwx-nonces-per-seed",
      "1024",
      "--pow-algorithms-hashwx-difficulty-base",
      "123",
      "--pow-algorithms-hashwx-difficulty-score-step",
      "7",
      "--pow-algorithms-hashwx-difficulty-max",
      "999",
    ])
    .unwrap();
    let document = rilgate_config::loader::parse_with(
      "server:\n  upstream:\n    url: http://127.0.0.1:3000\nsecurity:\n  secret: '0123456789abcdef0123456789abcdef'\n",
      &command.settings.into_settings(),
    ).unwrap();
    let config = document.resolve().unwrap();
    assert!(!config.pow.randomx_enabled);
    let algorithm = rilgate_pow_hashwx::HashWxAlgorithm::new(&config.pow.hashwx).unwrap();
    assert_eq!(algorithm.difficulty_for_weight(7), 246);
    assert_eq!(algorithm.difficulty_for_weight(i32::MAX), 999);
    let prepared = algorithm
      .issue(WorkRequest {
        difficulty: algorithm.difficulty_for_weight(7),
        verification: config
          .pow
          .verification
          .of(rilgate_protocol::PowAlgorithm::Hashwx),
      })
      .await
      .unwrap();
    assert!(matches!(
      prepared.work,
      Work::Hashwx {
        nonces_per_seed: 1024,
        difficulty: 246,
        ..
      }
    ));
  }
}
