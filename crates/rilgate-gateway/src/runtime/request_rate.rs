use super::client_key::key;
use rilgate_policy::policy::schema::RequestRateDefinition;
use std::{
  collections::{BTreeMap, HashMap},
  net::IpAddr,
  sync::Mutex,
  time::Instant,
};

/// Sliding request counts keyed by IPv4 address or IPv6 /64.
pub struct RequestRate {
  pub(super) settings: RequestRateDefinition,
  entries: Mutex<Entries>,
}

#[derive(Default)]
struct Entries {
  clients: HashMap<IpAddr, Client>,
  order: BTreeMap<(Instant, u64), IpAddr>,
  sequence: u64,
}

struct Client {
  start: Instant,
  previous: u32,
  current: u32,
  last: (Instant, u64),
}

impl RequestRate {
  pub fn new(settings: &RequestRateDefinition) -> Self {
    Self {
      settings: settings.clone(),
      entries: Mutex::default(),
    }
  }

  pub fn record(&self, ip: IpAddr) -> u32 {
    self.record_at(ip, Instant::now())
  }

  fn record_at(&self, ip: IpAddr, now: Instant) -> u32 {
    let key = key(ip);
    let window = self.settings.window;
    let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
    while let Some((&stamp, &oldest)) = entries.order.first_key_value() {
      if now.duration_since(stamp.0) < window * 2 {
        break;
      }
      entries.order.remove(&stamp);
      entries.clients.remove(&oldest);
    }
    let existing = entries.clients.remove(&key);
    if let Some(client) = &existing {
      entries.order.remove(&client.last);
    } else if entries.clients.len() >= self.settings.max_entries
      && let Some((_, oldest)) = entries.order.pop_first()
    {
      entries.clients.remove(&oldest);
    }
    entries.sequence += 1;
    let stamp = (now, entries.sequence);
    let mut client = existing.unwrap_or(Client {
      start: now,
      previous: 0,
      current: 0,
      last: stamp,
    });
    let elapsed = now.duration_since(client.start);
    if elapsed >= window * 2 {
      // Preserve the bucket boundary through idle windows.
      client.start =
        now - std::time::Duration::from_nanos((elapsed.as_nanos() % window.as_nanos()) as u64);
      client.previous = 0;
      client.current = 0;
    } else if elapsed >= window {
      client.start += window;
      client.previous = client.current;
      client.current = 0;
    }
    client.current = client.current.saturating_add(1);
    client.last = stamp;
    let remaining = window.as_nanos() - now.duration_since(client.start).as_nanos();
    let weighted = u128::from(client.previous) * remaining / window.as_nanos();
    let count = client.current.saturating_add(weighted as u32);
    entries.clients.insert(key, client);
    entries.order.insert(stamp, key);
    count
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::time::Duration;

  fn counter(max_entries: usize) -> RequestRate {
    RequestRate::new(&RequestRateDefinition {
      window: Duration::from_secs(60),
      max_entries,
    })
  }

  #[test]
  fn counts_slide_and_ipv6_clients_share_a_prefix() {
    let rate = counter(8);
    let start = Instant::now();
    let ip = "2001:db8:1:2::5".parse().unwrap();
    let sibling = "2001:db8:1:2:ffff::9".parse().unwrap();
    assert_eq!(rate.record_at(ip, start), 1);
    for count in 2..=10 {
      assert_eq!(
        rate.record_at(sibling, start + Duration::from_secs(10)),
        count
      );
    }
    assert_eq!(rate.record_at(ip, start + Duration::from_secs(60)), 11);
    assert_eq!(rate.record_at(ip, start + Duration::from_secs(90)), 7);
    assert_eq!(rate.record_at(ip, start + Duration::from_secs(135)), 2);
    assert_eq!(rate.record_at(ip, start + Duration::from_secs(240)), 1);
    let ipv4 = "192.0.2.1".parse().unwrap();
    assert_eq!(rate.record_at(ipv4, start + Duration::from_secs(240)), 1);
    assert_eq!(
      rate.record_at(
        "::ffff:192.0.2.1".parse().unwrap(),
        start + Duration::from_secs(240)
      ),
      2
    );
  }

  #[test]
  fn capacity_evicts_the_least_recent_client_and_reclaims_expired_clients() {
    let rate = counter(2);
    let start = Instant::now();
    let a = "192.0.2.1".parse().unwrap();
    let b = "192.0.2.2".parse().unwrap();
    let c = "192.0.2.3".parse().unwrap();
    rate.record_at(a, start);
    rate.record_at(b, start + Duration::from_secs(1));
    rate.record_at(a, start + Duration::from_secs(2));
    rate.record_at(c, start + Duration::from_secs(3));
    {
      let entries = rate.entries.lock().unwrap();
      assert!(entries.clients.contains_key(&a));
      assert!(!entries.clients.contains_key(&b));
      assert!(entries.clients.contains_key(&c));
      assert_eq!(entries.clients.len(), 2);
      assert_eq!(entries.order.len(), 2);
    }
    rate.record_at(b, start + Duration::from_secs(123));
    let entries = rate.entries.lock().unwrap();
    assert_eq!(entries.clients.len(), 1);
    assert_eq!(entries.order.len(), 1);
  }
}
