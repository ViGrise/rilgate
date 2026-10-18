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
const POOL: &str = "P2Pool";
const POLICY: &str = "Policy";

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
  spec!(SERVER, Text, "server.listen", "server-listen", "RILGATE_SERVER_LISTEN", Some('l'), "Listen address, such as :8080 or [::]:8080"),
  spec!(SERVER, Text, "server.upstream.url", "server-upstream-url", "RILGATE_SERVER_UPSTREAM_URL", Some('u'), "Application upstream URL"),
  spec!(SERVER, Switch, "server.upstream.preserve_host", "server-upstream-preserve-host", "RILGATE_SERVER_UPSTREAM_PRESERVE_HOST", None, "Forward the client Host header upstream"),
  spec!(SERVER, Text, "server.upstream.max_body_size", "server-upstream-max-body-size", "RILGATE_SERVER_UPSTREAM_MAX_BODY_SIZE", None, "Largest forwarded request body, such as 16MiB"),
  spec!(SERVER, Text, "server.upstream.timeout.connect", "server-upstream-timeout-connect", "RILGATE_SERVER_UPSTREAM_TIMEOUT_CONNECT", None, "Upstream connect timeout"),
  spec!(SERVER, Text, "server.upstream.timeout.read", "server-upstream-timeout-read", "RILGATE_SERVER_UPSTREAM_TIMEOUT_READ", None, "Upstream read timeout"),
  spec!(SERVER, Text, "server.max_connections", "server-max-connections", "RILGATE_SERVER_MAX_CONNECTIONS", None, "Maximum concurrent client connections"),
  spec!(SERVER, Text, "server.max_requests", "server-max-requests", "RILGATE_SERVER_MAX_REQUESTS", None, "Maximum concurrent requests"),
  spec!(SERVER, Text, "server.h2_max_streams", "server-h2-max-streams", "RILGATE_SERVER_H2_MAX_STREAMS", None, "Maximum concurrent HTTP/2 streams per connection"),
  spec!(SERVER, List, "server.trusted_proxies", "server-trusted-proxies", "RILGATE_SERVER_TRUSTED_PROXIES", None, "Proxy networks allowed to set X-Forwarded-For"),
  spec!(SERVER, Switch, "server.metrics_enabled", "server-metrics-enabled", "RILGATE_SERVER_METRICS_ENABLED", None, "Expose Prometheus metrics"),
  spec!(SERVER, Text, "server.readiness_timeout", "server-readiness-timeout", "RILGATE_SERVER_READINESS_TIMEOUT", None, "Dependency timeout for readiness probes"),
  spec!(SITE, Text, "site.name", "site-name", "RILGATE_SITE_NAME", None, "Site name shown on challenge pages"),
  spec!(SITE, Text, "site.url", "site-url", "RILGATE_SITE_URL", None, "Site URL linked from challenge pages"),
  spec!(SITE, Text, "site.base_path", "site-base-path", "RILGATE_SITE_BASE_PATH", None, "Path prefix of the internal namespace"),
  spec!(SITE, Text, "site.stylesheet", "site-stylesheet", "RILGATE_SITE_STYLESHEET", None, "Stylesheet path under static resources"),
  spec!(SITE, Text, "site.resources.directory", "site-resources-directory", "RILGATE_SITE_RESOURCES_DIRECTORY", None, "Directory with template, translation and static overrides"),
  spec!(SITE, Text, "site.resources.seed", "site-resources-seed", "RILGATE_SITE_RESOURCES_SEED", None, "Seed the resources directory: never or if_empty"),
  spec!(SITE, Switch, "site.resources.watch.enabled", "site-resources-watch-enabled", "RILGATE_SITE_RESOURCES_WATCH_ENABLED", None, "Reload resources when the directory changes"),
  spec!(SITE, Text, "site.resources.watch.debounce", "site-resources-watch-debounce", "RILGATE_SITE_RESOURCES_WATCH_DEBOUNCE", None, "Delay before reloading changed resources"),
  spec!(SITE, Switch, "site.templates.cache", "site-templates-cache", "RILGATE_SITE_TEMPLATES_CACHE", None, "Cache compiled templates"),
  spec!(SITE, Switch, "site.templates.strict", "site-templates-strict", "RILGATE_SITE_TEMPLATES_STRICT", None, "Fail on undefined template variables"),
  spec!(SITE, Text, "site.templates.recursion_limit", "site-templates-recursion-limit", "RILGATE_SITE_TEMPLATES_RECURSION_LIMIT", None, "Template recursion limit"),
  spec!(SITE, Text, "site.templates.fuel", "site-templates-fuel", "RILGATE_SITE_TEMPLATES_FUEL", None, "Template execution fuel per render"),
  spec!(SITE, Text, "site.responses.format", "site-responses-format", "RILGATE_SITE_RESPONSES_FORMAT", None, "Challenge response format selection"),
  spec!(SITE, List, "site.responses.json_paths", "site-responses-json-paths", "RILGATE_SITE_RESPONSES_JSON_PATHS", None, "Path prefixes that always receive JSON responses"),
  spec!(SITE, Text, "site.i18n.default", "site-i18n-default", "RILGATE_SITE_I18N_DEFAULT", None, "Default locale"),
  spec!(SITE, List, "site.i18n.supported", "site-i18n-supported", "RILGATE_SITE_I18N_SUPPORTED", None, "Supported locales; empty discovers them from resources"),
  spec!(SITE, Switch, "site.i18n.strict", "site-i18n-strict", "RILGATE_SITE_I18N_STRICT", None, "Fail on missing translations"),
  spec!(SITE, Text, "site.i18n.cookie", "site-i18n-cookie", "RILGATE_SITE_I18N_COOKIE", None, "Locale cookie name"),
  spec!(SECURITY, Secret, "security.secret", "security-secret", "RILGATE_SECURITY_SECRET", Some('s'), "Signing secret of at least 32 bytes; prefer the variable over the flag"),
  spec!(SECURITY, Text, "security.auth.cookie", "security-auth-cookie", "RILGATE_SECURITY_AUTH_COOKIE", None, "Authentication cookie name"),
  spec!(SECURITY, Text, "security.auth.ttl", "security-auth-ttl", "RILGATE_SECURITY_AUTH_TTL", None, "Authentication cookie lifetime"),
  spec!(SECURITY, Text, "security.auth.bind.client_ip.ipv4_prefix", "security-auth-bind-client-ip-ipv4-prefix", "RILGATE_SECURITY_AUTH_BIND_CLIENT_IP_IPV4_PREFIX", None, "IPv4 prefix length the cookie is bound to; setting either prefix enables client IP binding"),
  spec!(SECURITY, Text, "security.auth.bind.client_ip.ipv6_prefix", "security-auth-bind-client-ip-ipv6-prefix", "RILGATE_SECURITY_AUTH_BIND_CLIENT_IP_IPV6_PREFIX", None, "IPv6 prefix length the cookie is bound to; setting either prefix enables client IP binding"),
  spec!(SECURITY, Text, "security.auth.bind.user_agent", "security-auth-bind-user-agent", "RILGATE_SECURITY_AUTH_BIND_USER_AGENT", None, "Bind the cookie to the User-Agent: full or family"),
  spec!(SECURITY, List, "security.auth.bind.headers", "security-auth-bind-headers", "RILGATE_SECURITY_AUTH_BIND_HEADERS", None, "Client request headers the cookie is bound to"),
  spec!(SECURITY, List, "security.auth.bind.proxy_headers", "security-auth-bind-proxy-headers", "RILGATE_SECURITY_AUTH_BIND_PROXY_HEADERS", None, "Headers set by trusted proxies the cookie is bound to"),
  spec!(POW, Text, "pow.verification", "pow-verification", "RILGATE_POW_VERIFICATION", None, "Proof verification: local_verify or p2pool_accepted"),
  spec!(POW, Text, "pow.default_algorithm", "pow-default-algorithm", "RILGATE_POW_DEFAULT_ALGORITHM", None, "Algorithm for challenges without one: fast or randomx"),
  spec!(POW, Text, "pow.challenge.ttl", "pow-challenge-ttl", "RILGATE_POW_CHALLENGE_TTL", None, "Challenge lifetime"),
  spec!(POW, Text, "pow.challenge.receipt_ttl", "pow-challenge-receipt-ttl", "RILGATE_POW_CHALLENGE_RECEIPT_TTL", None, "Proof receipt lifetime"),
  spec!(POW, Switch, "pow.rate_limit.enabled", "pow-rate-limit-enabled", "RILGATE_POW_RATE_LIMIT_ENABLED", None, "Rate limit proof API requests"),
  spec!(POW, Text, "pow.rate_limit.max_clients", "pow-rate-limit-max-clients", "RILGATE_POW_RATE_LIMIT_MAX_CLIENTS", None, "Tracked rate limit clients"),
  spec!(POW, Text, "pow.rate_limit.client.burst", "pow-rate-limit-client-burst", "RILGATE_POW_RATE_LIMIT_CLIENT_BURST", None, "Proof requests a client may burst"),
  spec!(POW, Text, "pow.rate_limit.client.refill_per_second", "pow-rate-limit-client-refill-per-second", "RILGATE_POW_RATE_LIMIT_CLIENT_REFILL_PER_SECOND", None, "Client proof request refill rate"),
  spec!(POW, Text, "pow.rate_limit.global.burst", "pow-rate-limit-global-burst", "RILGATE_POW_RATE_LIMIT_GLOBAL_BURST", None, "Proof requests all clients may burst"),
  spec!(POW, Text, "pow.rate_limit.global.refill_per_second", "pow-rate-limit-global-refill-per-second", "RILGATE_POW_RATE_LIMIT_GLOBAL_REFILL_PER_SECOND", None, "Global proof request refill rate"),
  spec!(POW, Text, "pow.algorithms.randomx.challenge_ttl", "pow-algorithms-randomx-challenge-ttl", "RILGATE_POW_ALGORITHMS_RANDOMX_CHALLENGE_TTL", None, "RandomX challenge lifetime"),
  spec!(POW, Text, "pow.algorithms.randomx.nonce_offset", "pow-algorithms-randomx-nonce-offset", "RILGATE_POW_ALGORITHMS_RANDOMX_NONCE_OFFSET", None, "Nonce byte offset in the RandomX blob"),
  spec!(POW, Text, "pow.algorithms.randomx.verifier_queue", "pow-algorithms-randomx-verifier-queue", "RILGATE_POW_ALGORITHMS_RANDOMX_VERIFIER_QUEUE", None, "Queued RandomX verifications"),
  spec!(POW, Text, "pow.algorithms.randomx.difficulty.base", "pow-algorithms-randomx-difficulty-base", "RILGATE_POW_ALGORITHMS_RANDOMX_DIFFICULTY_BASE", None, "RandomX difficulty at score 0"),
  spec!(POW, Text, "pow.algorithms.randomx.difficulty.score_step", "pow-algorithms-randomx-difficulty-score-step", "RILGATE_POW_ALGORITHMS_RANDOMX_DIFFICULTY_SCORE_STEP", None, "Score that doubles RandomX difficulty"),
  spec!(POW, Text, "pow.algorithms.randomx.difficulty.max", "pow-algorithms-randomx-difficulty-max", "RILGATE_POW_ALGORITHMS_RANDOMX_DIFFICULTY_MAX", None, "Highest RandomX difficulty"),
  spec!(POW, Text, "pow.algorithms.randomx.browser.module", "pow-algorithms-randomx-browser-module", "RILGATE_POW_ALGORITHMS_RANDOMX_BROWSER_MODULE", None, "Browser RandomX solver module path"),
  spec!(POOL, Text, "pow.p2pool.address", "pow-p2pool-address", "RILGATE_POW_P2POOL_ADDRESS", None, "P2Pool Stratum address; setting any P2Pool value enables P2Pool"),
  spec!(POOL, Secret, "pow.p2pool.password", "pow-p2pool-password", "RILGATE_POW_P2POOL_PASSWORD", None, "P2Pool Stratum password"),
  spec!(POOL, Text, "pow.p2pool.agent", "pow-p2pool-agent", "RILGATE_POW_P2POOL_AGENT", None, "Stratum user agent"),
  spec!(POOL, Text, "pow.p2pool.timeout.connect", "pow-p2pool-timeout-connect", "RILGATE_POW_P2POOL_TIMEOUT_CONNECT", None, "P2Pool connect timeout"),
  spec!(POOL, Text, "pow.p2pool.timeout.submit", "pow-p2pool-timeout-submit", "RILGATE_POW_P2POOL_TIMEOUT_SUBMIT", None, "P2Pool share submit timeout"),
  spec!(POOL, Text, "pow.p2pool.sessions.max", "pow-p2pool-sessions-max", "RILGATE_POW_P2POOL_SESSIONS_MAX", None, "Concurrent browser mining sessions"),
  spec!(POOL, Text, "pow.p2pool.sessions.ttl", "pow-p2pool-sessions-ttl", "RILGATE_POW_P2POOL_SESSIONS_TTL", None, "Browser mining session lifetime"),
  spec!(POOL, Text, "pow.p2pool.job_sessions.mode", "pow-p2pool-job-sessions-mode", "RILGATE_POW_P2POOL_JOB_SESSIONS_MODE", None, "Job connections: pooled or per_challenge"),
  spec!(POOL, Text, "pow.p2pool.job_sessions.max", "pow-p2pool-job-sessions-max", "RILGATE_POW_P2POOL_JOB_SESSIONS_MAX", None, "Pooled job connections"),
  spec!(POOL, Text, "pow.p2pool.job_sessions.idle", "pow-p2pool-job-sessions-idle", "RILGATE_POW_P2POOL_JOB_SESSIONS_IDLE", None, "Idle time before a pooled job connection closes"),
  spec!(POLICY, Text, "policy.mode", "policy-mode", "RILGATE_POLICY_MODE", None, "Policy preset: off, light, balanced, strict or custom"),
];

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
          path: spec.path,
          source,
          value,
        });
      }
    }
    Ok(Self { values })
  }
  fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
    *self = Self::from_arg_matches(matches)?;
    Ok(())
  }
}

/// A document that enables the optional sections whose defaults are shown.
const OPTIONAL_SECTIONS: &str =
  "pow:\n  p2pool: {}\nsecurity:\n  auth:\n    bind:\n      client_ip: {}\n";

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
      "security:\n  secret: placeholder\n  auth:\n    bind:\n      client_ip: {}\n      user_agent: family\npow:\n  p2pool: {}\n",
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
  fn flags_are_unique() {
    let mut flags: Vec<_> = SPECS.iter().map(|spec| spec.flag).collect();
    flags.sort_unstable();
    flags.dedup();
    assert_eq!(flags.len(), SPECS.len());
  }
}
