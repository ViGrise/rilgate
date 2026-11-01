//! Environment-aware security findings over a resolved configuration.
//!
//! Errors describe settings that are unsafe in every deployment of their
//! environment and stop validation. Warnings describe settings that are valid in
//! some deployments, so they are reported without stopping startup.

use super::Configuration;
use crate::{Environment, schema::pow::Hasher};
use ipnet::IpNet;
use std::net::{IpAddr, SocketAddr};

const MAX_AUTH_TTL_SECONDS: u64 = 7 * 24 * 3600;
/// Shortest prefix accepted for a public trusted proxy network, per address family.
const MIN_TRUSTED_PREFIX: (u8, u8) = (8, 16);
/// Loopback, private, shared and link-local networks that never route publicly.
const NON_PUBLIC_NETWORKS: &[&str] = &[
  "127.0.0.0/8",
  "10.0.0.0/8",
  "172.16.0.0/12",
  "192.168.0.0/16",
  "100.64.0.0/10",
  "169.254.0.0/16",
  "::1/128",
  "fc00::/7",
  "fe80::/10",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
  Error,
  Warning,
}

/// A finding with a stable identifier and a message that starts with its YAML path.
#[derive(Debug, Clone)]
pub struct Finding {
  pub id: &'static str,
  pub severity: Severity,
  pub message: String,
}

impl std::fmt::Display for Finding {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(formatter, "{} [{}]", self.message, self.id)
  }
}

fn report(findings: &mut Vec<Finding>, id: &'static str, severity: Severity, message: String) {
  findings.push(Finding {
    id,
    severity,
    message,
  });
}

pub(super) fn audit(config: &Configuration) -> Vec<Finding> {
  let mut findings = Vec::new();
  let production = matches!(config.env, Environment::Production);
  if production {
    audit_production(config, &mut findings);
  } else {
    audit_development(config, &mut findings);
  }
  audit_generated_secret(config, &mut findings);
  let transport_severity = if production {
    Severity::Error
  } else {
    Severity::Warning
  };
  audit_stratum_pools(config, transport_severity, &mut findings);
  audit_stratum_proxies(config, transport_severity, &mut findings);
  audit_verifier(config, production, &mut findings);
  audit_metrics(config, &mut findings);
  findings
}

fn audit_production(config: &Configuration, findings: &mut Vec<Finding>) {
  for network in config
    .server
    .trusted_proxies
    .iter()
    .filter(|network| !is_non_public(network))
  {
    let minimum = match network {
      IpNet::V4(_) => MIN_TRUSTED_PREFIX.0,
      IpNet::V6(_) => MIN_TRUSTED_PREFIX.1,
    };
    if network.prefix_len() < minimum {
      report(
        findings,
        "trusted-proxy-broad",
        Severity::Error,
        format!(
          "server.trusted_proxies: {network} is a public network broader than /{minimum}; clients inside it can set their own address through X-Forwarded-For"
        ),
      );
    } else {
      report(
        findings,
        "trusted-proxy-public",
        Severity::Warning,
        format!(
          "server.trusted_proxies: {network} contains public addresses; trust only the networks of your edge or CDN"
        ),
      );
    }
  }
  if config
    .pow
    .policy
    .policy
    .score
    .as_ref()
    .is_some_and(|score| score.debug_header.enabled)
  {
    report(
      findings,
      "debug-score-header",
      Severity::Error,
      "policy.score.debug_header.enabled: debug score headers are disabled in production".into(),
    );
  }
  for (name, set) in &config.pow.policy.signals.ip_sets {
    if let Some(url) = &set.url
      && !url
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
    {
      report(
        findings,
        "ip-set-plain-http",
        Severity::Error,
        format!("signals.ip_sets.{name}.url: production requires https"),
      );
    }
  }
  for (name, set) in &config.pow.policy.signals.asn_sets {
    if let Some(url) = &set.url
      && !url
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
    {
      report(
        findings,
        "asn-set-plain-http",
        Severity::Error,
        format!("signals.asn_sets.{name}.url: production requires https"),
      );
    }
  }
  for address in public(&config.operations_listen_addresses) {
    report(
      findings,
      "operations-listen-public",
      Severity::Warning,
      format!(
        "server.operations.listen: {address} is reachable beyond loopback and private networks; operational routes have no authentication"
      ),
    );
  }
  if !config.pow.rate_limit.enabled {
    report(
      findings,
      "proof-rate-limit-disabled",
      Severity::Warning,
      "pow.rate_limit.enabled: proof API quotas are disabled; verification client quotas at the edge"
        .into(),
    );
  }
  if config.web.watch.enabled {
    report(findings,
      "resource-watch-enabled",
      Severity::Warning,
      "site.resources.watch.enabled: production resources should be mounted read-only without watching"
        .into(),
    );
  }
  if config.pow.auth_ttl_seconds > MAX_AUTH_TTL_SECONDS {
    report(findings,
      "auth-ttl-long",
      Severity::Warning,
      "security.auth.ttl: access cookies are reusable bearer credentials; keep their lifetime at most 7d"
        .into(),
    );
  }
}

fn audit_development(config: &Configuration, findings: &mut Vec<Finding>) {
  for address in config
    .listen_addresses
    .iter()
    .filter(|address| !address.ip().to_canonical().is_loopback())
  {
    report(
      findings,
      "development-public-listener",
      Severity::Warning,
      format!(
        "server.listen: {address} is reachable beyond loopback; development issues access cookies without Secure"
      ),
    );
  }
}

fn audit_generated_secret(config: &Configuration, findings: &mut Vec<Finding>) {
  if config.secret_generated {
    report(findings,
      "generated-secret",
      Severity::Warning,
      "security.secret: not set, so this process signs with a random secret; access cookies end at restart and other instances cannot verify them. Set RILGATE_SECURITY_SECRET to keep them"
        .into(),
    );
  }
}

fn audit_stratum_pools(
  config: &Configuration,
  transport_severity: Severity,
  findings: &mut Vec<Finding>,
) {
  let pools = &config.pow.stratum_pool;
  for (index, pool) in pools.pools.iter().enumerate() {
    if host_is_public(&pool.host) {
      if !pool.tls.enabled {
        report(
          findings,
          "stratum-pool-plain-tcp",
          transport_severity,
          format!(
            "pow.stratum_pool.pools[{index}].tls.enabled: pool {} requires TLS outside loopback and private networks",
            pool.name
          ),
        );
      }
      if pools.job_session_mode == rilgate_pool::JobSessionMode::PerChallenge {
        report(
          findings,
          "stratum-pool-per-challenge-public",
          Severity::Warning,
          format!(
            "pow.stratum_pool.job_sessions.mode: pool {} opens a public connection for each challenge; use pooled sessions",
            pool.name
          ),
        );
      }
    }
  }
}

fn audit_stratum_proxies(
  config: &Configuration,
  transport_severity: Severity,
  findings: &mut Vec<Finding>,
) {
  let pools = &config.pow.stratum_pool;
  let proxy_lists = std::iter::once((
    "pow.stratum_pool.proxies".to_string(),
    pools.proxies.as_slice(),
  ))
  .chain(pools.pools.iter().enumerate().filter_map(|(index, pool)| {
    pool
      .proxies
      .as_deref()
      .map(|proxies| (format!("pow.stratum_pool.pools[{index}].proxies"), proxies))
  }));
  for (path, proxies) in proxy_lists {
    for (index, proxy) in proxies.iter().enumerate() {
      if proxy.username.is_some()
        && proxy
          .endpoint()
          .is_ok_and(|url| host_is_public(url.host_str().unwrap().trim_matches(['[', ']'])))
      {
        report(
          findings,
          "stratum-proxy-plain-credentials",
          transport_severity,
          format!(
            "{path}[{index}]: proxy {} sends credentials outside loopback and private networks; use a private network or a tunnel",
            proxy.name
          ),
        );
      }
    }
  }
}

fn audit_verifier(config: &Configuration, production: bool, findings: &mut Vec<Finding>) {
  let verify = &config.pow.randomx_verify;
  let remote = &verify.remote;
  if production && (remote.endpoint.is_some() || verify.order.contains(&Hasher::Remote)) {
    if remote
      .endpoint
      .as_ref()
      .is_some_and(|endpoint| !endpoint.to_ascii_lowercase().starts_with("https://"))
    {
      report(
        findings,
        "verifier-remote-plain-http",
        Severity::Error,
        "pow.algorithms.randomx.verify.remote.endpoint: production requires https".into(),
      );
    }
    if remote.tls.ca.is_none()
      || remote.tls.client_cert.is_none()
      || remote.tls.client_key.is_none()
    {
      report(findings, "verifier-remote-credentials", Severity::Error,
          "pow.algorithms.randomx.verify.remote.tls: production requires ca, client_cert and client_key".into());
    }
  }
  if verify.order.contains(&Hasher::Remote) {
    if remote.endpoint.is_none() {
      report(
        findings,
        "verifier-remote-missing",
        Severity::Error,
        "pow.algorithms.randomx.verify.remote.endpoint: required when order contains remote".into(),
      );
    }
    if verify.order == [Hasher::Remote, Hasher::Local] {
      report(findings, "verifier-local-fallback", Severity::Warning,
        "pow.algorithms.randomx.verify.order: remote outages can build a 256 MiB cache on every gateway".into());
    }
    if !config.pow.selects_randomx() {
      report(findings, "verifier-remote-unused", Severity::Warning,
        "pow.algorithms.randomx.verify.order: remote hashing is unused by the configured challenges".into());
    }
  }
}

fn audit_metrics(config: &Configuration, findings: &mut Vec<Finding>) {
  if config.server.metrics_enabled && config.operations_listen_addresses.is_empty() {
    report(
      findings,
      "metrics-unreachable",
      Severity::Warning,
      "server.metrics_enabled: metrics are served only on server.operations.listen, which is empty"
        .into(),
    );
  }
}

fn is_non_public(network: &IpNet) -> bool {
  let network = match network {
    IpNet::V6(v6) => match v6.network().to_ipv4_mapped() {
      Some(v4) => {
        IpNet::new(IpAddr::V4(v4), v6.prefix_len().saturating_sub(96)).unwrap_or(*network)
      }
      None => *network,
    },
    IpNet::V4(_) => *network,
  };
  NON_PUBLIC_NETWORKS
    .iter()
    .filter_map(|range| range.parse::<IpNet>().ok())
    .any(|range| range.contains(&network))
}

/// Addresses outside loopback and private networks, including unspecified wildcards.
pub(super) fn public(addresses: &[SocketAddr]) -> impl Iterator<Item = &SocketAddr> {
  addresses
    .iter()
    .filter(|address| !is_non_public(&IpNet::from(address.ip().to_canonical())))
}

fn host_is_public(host: &str) -> bool {
  if let Ok(ip) = host.parse::<IpAddr>() {
    return !is_non_public(&IpNet::from(ip.to_canonical()));
  }
  let host = host.trim_end_matches('.').to_ascii_lowercase();
  host.contains('.')
    && ![
      ".local",
      ".internal",
      ".lan",
      ".home.arpa",
      ".svc",
      ".cluster.local",
    ]
    .iter()
    .any(|suffix| host.ends_with(suffix))
}
