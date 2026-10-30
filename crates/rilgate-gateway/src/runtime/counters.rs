use rilgate_policy::{counters::CounterKey, policy::schema::CounterDefinition};
use std::{
  collections::{BTreeMap, HashMap},
  sync::Mutex,
  time::Instant,
};

/// Sliding request counts for one named counter.
pub struct CounterStore {
  pub(super) settings: CounterDefinition,
  entries: Mutex<Entries>,
}

#[derive(Default)]
struct Entries {
  clients: HashMap<CounterKey, Client>,
  order: BTreeMap<(Instant, u64), CounterKey>,
  sequence: u64,
}

struct Client {
  start: Instant,
  previous: u32,
  current: u32,
  last: (Instant, u64),
}

impl CounterStore {
  pub fn new(settings: &CounterDefinition) -> Self {
    Self {
      settings: settings.clone(),
      entries: Mutex::default(),
    }
  }

  pub fn record(&self, key: CounterKey) -> u32 {
    self.access_at(key, Instant::now(), true)
  }

  pub fn peek(&self, key: CounterKey) -> u32 {
    self.access_at(key, Instant::now(), false)
  }

  fn access_at(&self, key: CounterKey, now: Instant, record: bool) -> u32 {
    let window = self.settings.window;
    let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
    while let Some((&stamp, oldest)) = entries.order.first_key_value() {
      let oldest = oldest.clone();
      if now.duration_since(stamp.0) < window * 2 {
        break;
      }
      entries.order.remove(&stamp);
      entries.clients.remove(&oldest);
    }
    let existing = entries.clients.remove(&key);
    if existing.is_none() && !record {
      return 0;
    }
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
    client.current = client.current.saturating_add(u32::from(record));
    client.last = stamp;
    let remaining = window.as_nanos() - now.duration_since(client.start).as_nanos();
    let weighted = u128::from(client.previous) * remaining / window.as_nanos();
    let count = client.current.saturating_add(weighted as u32);
    entries.clients.insert(key.clone(), client);
    entries.order.insert(stamp, key);
    count
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use rilgate_policy::counters::KeyPart;
  use std::time::Duration;

  fn key(number: u32) -> CounterKey {
    vec![KeyPart::Number(number)].into_boxed_slice()
  }

  #[test]
  fn counts_decay_peeks_do_not_count_and_capacity_is_bounded() {
    let settings = serde_json::from_value(
      serde_json::json!({"key": ["client_ip"], "window": "1m", "max_entries": 2}),
    )
    .unwrap();
    let store = CounterStore::new(&settings);
    let start = Instant::now();
    for count in 1..=10 {
      assert_eq!(store.access_at(key(1), start, true), count);
    }
    assert_eq!(store.access_at(key(1), start, false), 10);
    assert_eq!(
      store.access_at(key(1), start + Duration::from_secs(90), false),
      5
    );
    assert_eq!(
      store.access_at(key(1), start + Duration::from_secs(120), false),
      0
    );
    store.access_at(key(2), start + Duration::from_secs(121), true);
    store.access_at(key(1), start + Duration::from_secs(122), true);
    store.access_at(key(3), start + Duration::from_secs(123), true);
    assert_eq!(
      store.access_at(key(2), start + Duration::from_secs(124), false),
      0
    );
    assert_eq!(store.entries.lock().unwrap().clients.len(), 2);
    store.access_at(key(4), start + Duration::from_secs(244), true);
    assert_eq!(store.entries.lock().unwrap().clients.len(), 1);
  }
}
