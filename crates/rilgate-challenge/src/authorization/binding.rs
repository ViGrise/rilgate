use anyhow::{Result, ensure};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use http::{HeaderMap, HeaderName};
use ipnet::{Ipv4Net, Ipv6Net};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::net::IpAddr;

type HmacSha256 = Hmac<Sha256>;

const KEY_CONTEXT: &[u8] = b"rilgate/v1/auth-binding";
const DIGEST_BYTES: usize = 16;
const MAX_HEADERS: usize = 16;

/// Request attributes an authentication cookie can be bound to.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuthBinding {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub client_ip: Option<ClientIpBinding>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub user_agent: Option<UserAgentBinding>,
  pub headers: Vec<String>,
  pub proxy_headers: Vec<String>,
}

/// Client network binding settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClientIpBinding {
  pub ipv4_prefix: u8,
  pub ipv6_prefix: u8,
  pub max_networks: u8,
}

impl Default for ClientIpBinding {
  fn default() -> Self {
    Self {
      ipv4_prefix: 24,
      ipv6_prefix: 64,
      max_networks: 1,
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UserAgentBinding {
  /// The User-Agent header as sent.
  Full,
  /// The User-Agent header without version numbers.
  Family,
}

/// The request attributes a binding digest is computed from.
pub struct BindingInput<'a> {
  pub client_ip: Option<IpAddr>,
  pub headers: &'a HeaderMap,
  /// Whether the immediate TCP peer is a trusted proxy.
  pub trusted_peer: bool,
}

/// An authentication cookie presented from a request that differs from the one that earned it.
#[derive(Debug, thiserror::Error)]
#[error("Access token is bound to a different client")]
pub struct BindingMismatch;

impl<'a> BindingInput<'a> {
  /// An input with no client attributes, for contexts where binding is not evaluated.
  pub fn unbound(headers: &'a HeaderMap) -> Self {
    Self {
      client_ip: None,
      headers,
      trusted_peer: false,
    }
  }
}

impl AuthBinding {
  pub fn enabled(&self) -> bool {
    self.client_ip.is_some()
      || self.user_agent.is_some()
      || !self.headers.is_empty()
      || !self.proxy_headers.is_empty()
  }

  pub fn validate(&self) -> Result<()> {
    if let Some(ip) = &self.client_ip {
      ensure!(
        ip.ipv4_prefix <= 32,
        "security.auth.bind.client_ip.ipv4_prefix: expected 0 to 32"
      );
      ensure!(
        ip.ipv6_prefix <= 128,
        "security.auth.bind.client_ip.ipv6_prefix: expected 0 to 128"
      );
      ensure!(
        (1..=8).contains(&ip.max_networks),
        "security.auth.bind.client_ip.max_networks: expected 1 to 8"
      );
    }
    let mut seen = Vec::new();
    for (path, names) in [
      ("security.auth.bind.headers", &self.headers),
      ("security.auth.bind.proxy_headers", &self.proxy_headers),
    ] {
      ensure!(
        names.len() <= MAX_HEADERS,
        "{path}: expected at most {MAX_HEADERS} header names"
      );
      for name in names {
        let parsed = HeaderName::from_bytes(name.as_bytes())
          .map_err(|_| anyhow::anyhow!("{path}: {name:?} is not a valid header name"))?;
        ensure!(
          !seen.contains(&parsed),
          "{path}: header {name:?} is listed more than once"
        );
        seen.push(parsed);
      }
    }
    Ok(())
  }

  /// Derives the key that binding digests are computed with from the auth signing key.
  pub fn derive_key(auth_signing_key: &[u8]) -> Result<[u8; 32]> {
    let mut mac = HmacSha256::new_from_slice(auth_signing_key)?;
    mac.update(KEY_CONTEXT);
    Ok(mac.finalize().into_bytes().into())
  }

  /// Returns the digest of the enabled components, or `None` when binding is disabled.
  pub fn digest(&self, key: &[u8; 32], input: &BindingInput<'_>) -> Option<String> {
    if !self.enabled() {
      return None;
    }
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(self.canonical(input).as_bytes());
    Some(URL_SAFE_NO_PAD.encode(&mac.finalize().into_bytes()[..DIGEST_BYTES]))
  }

  pub fn client_digest(&self, key: &[u8; 32], input: &BindingInput<'_>) -> Option<String> {
    let client = Self {
      client_ip: None,
      ..self.clone()
    };
    client.digest(key, input)
  }

  pub fn network_digest(&self, key: &[u8; 32], input: &BindingInput<'_>) -> Option<String> {
    self.client_ip?;
    let network = Self {
      client_ip: self.client_ip,
      ..Default::default()
    };
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(b"net\n");
    mac.update(network.canonical(input).as_bytes());
    Some(URL_SAFE_NO_PAD.encode(&mac.finalize().into_bytes()[..DIGEST_BYTES]))
  }

  fn canonical(&self, input: &BindingInput<'_>) -> String {
    let mut out = String::new();
    if let Some(ip) = &self.client_ip {
      match input.client_ip.map(|address| address.to_canonical()) {
        Some(IpAddr::V4(address)) => {
          let prefix = ip.ipv4_prefix.min(32);
          let net = Ipv4Net::new(address, prefix)
            .expect("prefix is clamped")
            .trunc();
          out.push_str(&format!("ip:{}/{prefix}\n", net.network()));
        }
        Some(IpAddr::V6(address)) => {
          let prefix = ip.ipv6_prefix.min(128);
          let net = Ipv6Net::new(address, prefix)
            .expect("prefix is clamped")
            .trunc();
          out.push_str(&format!("ip:{}/{prefix}\n", net.network()));
        }
        None => out.push_str("ip:-\n"),
      }
    }
    if let Some(mode) = self.user_agent {
      let raw = input
        .headers
        .get(http::header::USER_AGENT)
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
        .unwrap_or_default();
      let value = match mode {
        UserAgentBinding::Full => raw,
        UserAgentBinding::Family => user_agent_family(&raw),
      };
      out.push_str(&format!("ua:{value}\n"));
    }
    for name in &self.headers {
      push_header(&mut out, "h", name, input.headers, true);
    }
    for name in &self.proxy_headers {
      push_header(&mut out, "p", name, input.headers, input.trusted_peer);
    }
    out
  }
}

fn push_header(out: &mut String, tag: &str, name: &str, headers: &HeaderMap, readable: bool) {
  let name = name.to_ascii_lowercase();
  let mut values = headers
    .get_all(name.as_str())
    .iter()
    .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
    .peekable();
  if !readable || values.peek().is_none() {
    out.push_str(&format!("{tag}:{name}!\n"));
    return;
  }
  out.push_str(&format!(
    "{tag}:{name}={}\n",
    values.collect::<Vec<_>>().join(",")
  ));
}

fn user_agent_family(raw: &str) -> String {
  let mut stripped = String::with_capacity(raw.len());
  let mut in_version = false;
  for c in raw.chars() {
    if in_version && (c.is_ascii_digit() || c == '.' || c == '_') {
      continue;
    }
    in_version = c.is_ascii_digit();
    if !in_version {
      stripped.push(c);
    }
  }
  stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
  use super::*;

  const CHROME_130: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/130.0.6723.58 Safari/537.36";
  const CHROME_131: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/131.0.0.0 Safari/537.36";
  const FIREFOX: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:132.0) Gecko/20100101 Firefox/132.0";

  fn key() -> [u8; 32] {
    AuthBinding::derive_key(b"test-auth-signing-key").unwrap()
  }

  fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
      map.append(
        HeaderName::from_bytes(name.as_bytes()).unwrap(),
        value.parse().unwrap(),
      );
    }
    map
  }

  fn digest(binding: &AuthBinding, ip: Option<&str>, headers: &HeaderMap, trusted: bool) -> String {
    binding
      .digest(
        &key(),
        &BindingInput {
          client_ip: ip.map(|ip| ip.parse().unwrap()),
          headers,
          trusted_peer: trusted,
        },
      )
      .unwrap()
  }

  fn ip_only() -> AuthBinding {
    AuthBinding {
      client_ip: Some(ClientIpBinding::default()),
      ..Default::default()
    }
  }

  #[test]
  fn client_ip_is_truncated_to_the_prefix() {
    let none = HeaderMap::new();
    let binding = ip_only();
    let base = digest(&binding, Some("192.0.2.10"), &none, false);
    assert_eq!(base, digest(&binding, Some("192.0.2.250"), &none, false));
    assert_ne!(base, digest(&binding, Some("192.0.3.10"), &none, false));
    let v6 = digest(&binding, Some("2001:db8:1:2::1"), &none, false);
    assert_eq!(
      v6,
      digest(&binding, Some("2001:db8:1:2:ffff::9"), &none, false)
    );
    assert_ne!(v6, digest(&binding, Some("2001:db8:1:3::1"), &none, false));
    assert_ne!(base, digest(&binding, None, &none, false));

    let wide = AuthBinding {
      client_ip: Some(ClientIpBinding {
        ipv4_prefix: 16,
        ipv6_prefix: 48,
        ..Default::default()
      }),
      ..Default::default()
    };
    assert_ne!(base, digest(&wide, Some("192.0.2.10"), &none, false));
    assert_eq!(
      digest(&wide, Some("192.0.2.10"), &none, false),
      digest(&wide, Some("192.0.3.10"), &none, false)
    );
  }

  #[test]
  fn mapped_ipv6_equals_ipv4() {
    let none = HeaderMap::new();
    assert_eq!(
      digest(&ip_only(), Some("::ffff:192.0.2.10"), &none, false),
      digest(&ip_only(), Some("192.0.2.10"), &none, false)
    );
  }

  #[test]
  fn user_agent_family_ignores_versions_only() {
    let family = AuthBinding {
      user_agent: Some(UserAgentBinding::Family),
      ..Default::default()
    };
    let full = AuthBinding {
      user_agent: Some(UserAgentBinding::Full),
      ..Default::default()
    };
    let d = |binding: &AuthBinding, ua: &str| {
      digest(binding, None, &headers(&[("user-agent", ua)]), false)
    };
    assert_eq!(d(&family, CHROME_130), d(&family, CHROME_131));
    assert_ne!(d(&family, CHROME_130), d(&family, FIREFOX));
    assert_ne!(d(&full, CHROME_130), d(&full, CHROME_131));
    assert_eq!(user_agent_family("Chrome/130.0.6723.58  x"), "Chrome/ x");
    assert_eq!(
      digest(&family, None, &HeaderMap::new(), false),
      digest(&family, None, &headers(&[("user-agent", "")]), false)
    );
  }

  #[test]
  fn absent_header_differs_from_empty_header() {
    let binding = AuthBinding {
      headers: vec!["Accept-Language".into()],
      ..Default::default()
    };
    let absent = digest(&binding, None, &HeaderMap::new(), false);
    let empty = digest(&binding, None, &headers(&[("accept-language", "")]), false);
    let set = digest(
      &binding,
      None,
      &headers(&[("accept-language", "en")]),
      false,
    );
    assert_ne!(absent, empty);
    assert_ne!(empty, set);
  }

  #[test]
  fn proxy_headers_are_read_only_from_trusted_peers() {
    let binding = AuthBinding {
      proxy_headers: vec!["x-ja4".into()],
      ..Default::default()
    };
    let a = headers(&[("x-ja4", "aaa")]);
    let b = headers(&[("x-ja4", "bbb")]);
    assert_eq!(
      digest(&binding, None, &a, false),
      digest(&binding, None, &b, false)
    );
    assert_eq!(
      digest(&binding, None, &HeaderMap::new(), false),
      digest(&binding, None, &a, false)
    );
    assert_ne!(
      digest(&binding, None, &a, true),
      digest(&binding, None, &b, true)
    );
  }

  #[test]
  fn disabled_binding_has_no_digest() {
    let none = HeaderMap::new();
    let input = BindingInput {
      client_ip: None,
      headers: &none,
      trusted_peer: false,
    };
    assert!(AuthBinding::default().digest(&key(), &input).is_none());
  }

  #[test]
  fn validation_reports_the_yaml_path() {
    let check = |binding: AuthBinding| binding.validate().unwrap_err().to_string();
    assert_eq!(
      check(AuthBinding {
        client_ip: Some(ClientIpBinding {
          ipv4_prefix: 33,
          ..Default::default()
        }),
        ..Default::default()
      }),
      "security.auth.bind.client_ip.ipv4_prefix: expected 0 to 32"
    );
    assert!(
      check(AuthBinding {
        client_ip: Some(ClientIpBinding {
          ipv6_prefix: 129,
          ..Default::default()
        }),
        ..Default::default()
      })
      .starts_with("security.auth.bind.client_ip.ipv6_prefix")
    );
    assert!(
      check(AuthBinding {
        headers: vec!["bad name".into()],
        ..Default::default()
      })
      .starts_with("security.auth.bind.headers")
    );
    assert!(
      check(AuthBinding {
        headers: vec!["X-Ja4".into()],
        proxy_headers: vec!["x-ja4".into()],
        ..Default::default()
      })
      .starts_with("security.auth.bind.proxy_headers")
    );
    assert!(
      check(AuthBinding {
        headers: (0..17).map(|i| format!("x-h{i}")).collect(),
        ..Default::default()
      })
      .contains("at most 16")
    );
  }
}
