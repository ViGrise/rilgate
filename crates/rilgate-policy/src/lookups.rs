use std::collections::BTreeMap;

/// Lookup results and counter values for one request, filled in by the caller.
///
/// A missing entry means the lookup was not performed or did not finish. Conditions
/// that depend on a missing entry evaluate to unknown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lookups {
  /// Whether each DNSBL zone lists the client.
  pub dnsbl: BTreeMap<String, bool>,
  /// The forward-confirmed PTR name of the client. `Some(None)` means the client has none.
  pub reverse_dns: Option<Option<String>>,
  /// Whether the client followed the honeypot link. A missing entry counts as not flagged.
  pub honeypot: Option<bool>,
  /// Counts indexed by compiled counter position, including selected current requests.
  pub rates: Vec<Option<u32>>,
}

impl Lookups {
  /// A value with no results, for callers that perform no lookups.
  pub const NONE: Self = Self {
    dnsbl: BTreeMap::new(),
    reverse_dns: None,
    honeypot: None,
    rates: Vec::new(),
  };
}

/// The lookups that the conditions of one policy can use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LookupPlan {
  pub dnsbl_zones: Vec<String>,
  pub reverse_dns: bool,
}

impl LookupPlan {
  pub fn is_empty(&self) -> bool {
    self.dnsbl_zones.is_empty() && !self.reverse_dns
  }
}

/// Reports whether a PTR name equals one of the domains or is a subdomain of it.
/// Domains are lowercase without a trailing dot.
pub(crate) fn in_domains(name: &str, domains: &[String]) -> bool {
  let name = name.trim_end_matches('.').to_ascii_lowercase();
  domains.iter().any(|domain| {
    name == *domain
      || name
        .strip_suffix(domain.as_str())
        .is_some_and(|prefix| prefix.ends_with('.'))
  })
}

/// Normalizes a DNS name to lowercase without a trailing dot. Returns `None` when the
/// text is not a valid host name of at most `max_len` bytes.
pub(crate) fn normalize_name(text: &str, max_len: usize) -> Option<String> {
  let name = text.strip_suffix('.').unwrap_or(text).to_ascii_lowercase();
  let valid = !name.is_empty()
    && name.len() <= max_len
    && name.split('.').all(|label| {
      (1..=63).contains(&label.len())
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
          .bytes()
          .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    });
  valid.then_some(name)
}
