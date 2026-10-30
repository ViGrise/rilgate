use super::client_key::key;
use rilgate_policy::policy::schema::HoneypotDefinition;
use std::{
  collections::{BTreeMap, HashMap},
  net::IpAddr,
  sync::Mutex,
  time::{Duration, Instant},
};

/// Clients that requested the hidden link, keyed by IPv4 address or IPv6 /64.
pub struct Honeypot {
  ttl: Duration,
  max_entries: usize,
  entries: Mutex<Entries>,
}

#[derive(Default)]
struct Entries {
  expiry: HashMap<IpAddr, (Instant, u64)>,
  /// Keys ordered by expiry. Every entry lives for the same time, so the first
  /// key is the oldest and any expired key precedes every live one.
  order: BTreeMap<(Instant, u64), IpAddr>,
  sequence: u64,
}

impl Honeypot {
  pub fn new(settings: &HoneypotDefinition) -> Self {
    Self {
      ttl: settings.ttl,
      max_entries: settings.max_entries,
      entries: Mutex::default(),
    }
  }

  /// Flags the client for the configured time.
  pub fn flag(&self, ip: IpAddr) {
    self.flag_at(ip, Instant::now());
  }

  pub fn is_flagged(&self, ip: IpAddr) -> bool {
    self.is_flagged_at(ip, Instant::now())
  }

  /// Flags the client when its address is known. A request without a resolved
  /// client address flags nobody, so one hit cannot flag every client behind a proxy.
  pub fn record_hit(&self, ip: Option<IpAddr>) {
    if let Some(ip) = ip {
      self.flag(ip);
    }
  }

  fn flag_at(&self, ip: IpAddr, now: Instant) {
    let key = key(ip);
    let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(previous) = entries.expiry.remove(&key) {
      entries.order.remove(&previous);
    } else {
      while let Some((&(expires, sequence), &oldest)) = entries.order.first_key_value() {
        if expires > now && entries.expiry.len() < self.max_entries {
          break;
        }
        entries.order.remove(&(expires, sequence));
        entries.expiry.remove(&oldest);
      }
    }
    entries.sequence += 1;
    let stamp = (now + self.ttl, entries.sequence);
    entries.expiry.insert(key, stamp);
    entries.order.insert(stamp, key);
  }

  fn is_flagged_at(&self, ip: IpAddr, now: Instant) -> bool {
    let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
    entries
      .expiry
      .get(&key(ip))
      .is_some_and(|(expires, _)| *expires > now)
  }

  #[cfg(test)]
  fn len(&self) -> usize {
    self.entries.lock().unwrap().expiry.len()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn honeypot(ttl: u64, max_entries: usize) -> Honeypot {
    Honeypot::new(&HoneypotDefinition {
      ttl: Duration::from_secs(ttl),
      max_entries,
    })
  }

  fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
  }

  #[test]
  fn flags_expire_after_the_ttl() {
    let honeypot = honeypot(60, 8);
    let start = Instant::now();
    honeypot.flag_at(ip("192.0.2.1"), start);
    assert!(honeypot.is_flagged_at(ip("192.0.2.1"), start + Duration::from_secs(59)));
    assert!(!honeypot.is_flagged_at(ip("192.0.2.1"), start + Duration::from_secs(61)));
    assert!(!honeypot.is_flagged_at(ip("192.0.2.2"), start));
  }

  #[test]
  fn ipv6_clients_share_a_slash_64_and_mapped_ipv4_is_ipv4() {
    let honeypot = honeypot(60, 8);
    let start = Instant::now();
    honeypot.flag_at(ip("2001:db8:1:2::5"), start);
    assert!(honeypot.is_flagged_at(ip("2001:db8:1:2:ffff::9"), start));
    assert!(!honeypot.is_flagged_at(ip("2001:db8:1:3::5"), start));
    honeypot.flag_at(ip("::ffff:192.0.2.1"), start);
    assert!(honeypot.is_flagged_at(ip("192.0.2.1"), start));
    assert_eq!(honeypot.len(), 2);
  }

  #[test]
  fn a_hit_without_a_client_address_flags_nobody() {
    let honeypot = honeypot(60, 8);
    honeypot.record_hit(None);
    assert_eq!(honeypot.len(), 0);
    honeypot.record_hit(Some(ip("192.0.2.1")));
    assert_eq!(honeypot.len(), 1);
  }

  #[test]
  fn a_full_table_drops_expired_entries_and_then_the_oldest() {
    let honeypot = honeypot(60, 2);
    let start = Instant::now();
    honeypot.flag_at(ip("192.0.2.1"), start);
    honeypot.flag_at(ip("192.0.2.2"), start + Duration::from_secs(10));
    // The first entry has expired and makes room.
    honeypot.flag_at(ip("192.0.2.3"), start + Duration::from_secs(65));
    let now = start + Duration::from_secs(66);
    assert!(!honeypot.is_flagged_at(ip("192.0.2.1"), now));
    assert!(honeypot.is_flagged_at(ip("192.0.2.2"), now));
    assert!(honeypot.is_flagged_at(ip("192.0.2.3"), now));
    // Every entry is live, so the oldest goes.
    honeypot.flag_at(ip("192.0.2.4"), now);
    assert!(!honeypot.is_flagged_at(ip("192.0.2.2"), now));
    assert!(honeypot.is_flagged_at(ip("192.0.2.3"), now));
    assert!(honeypot.is_flagged_at(ip("192.0.2.4"), now));
    assert_eq!(honeypot.len(), 2);
  }
}
