//! DNS lookups for the `dnsbl` and `client_ip.reverse_dns` policy conditions.

mod hickory;

use async_trait::async_trait;
use moka::{Expiry, future::Cache};
use rilgate_policy::policy::schema::DnsDefinition;
use rilgate_policy::{LookupPlan, Lookups};
use std::fmt::Write as _;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The PTR names of one client that are checked against the forward records.
const MAX_PTR_NAMES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupError {
  /// The name or record does not exist.
  NotFound,
  /// The lookup failed or timed out, so the answer is unknown.
  Failed,
}

#[async_trait]
pub trait Resolver: Send + Sync {
  /// The PTR names of an address.
  async fn ptr(&self, ip: IpAddr) -> Result<Vec<String>, LookupError>;
  /// The A and AAAA addresses of a name.
  async fn addresses(&self, name: &str) -> Result<Vec<IpAddr>, LookupError>;
  /// The A addresses of a name.
  async fn ipv4(&self, name: &str) -> Result<Vec<Ipv4Addr>, LookupError>;
}

/// A cached answer with the time it stays valid.
#[derive(Clone)]
struct Entry<T> {
  value: T,
  ttl: Duration,
}

struct EntryExpiry;
impl<K, T> Expiry<K, Entry<T>> for EntryExpiry {
  fn expire_after_create(&self, _: &K, entry: &Entry<T>, _: Instant) -> Option<Duration> {
    Some(entry.ttl)
  }
}

fn cache<K, T>(capacity: usize) -> Cache<K, Entry<T>>
where
  K: std::hash::Hash + Eq + Send + Sync + 'static,
  T: Clone + Send + Sync + 'static,
{
  Cache::builder()
    .max_capacity(capacity as u64)
    .expire_after(EntryExpiry)
    .build()
}

#[derive(Clone)]
pub struct DnsLookups {
  resolver: Arc<dyn Resolver>,
  timeout: Duration,
  cache_ttl: Duration,
  negative_ttl: Duration,
  reverse: Cache<IpAddr, Entry<Option<String>>>,
  listings: Cache<(IpAddr, String), Entry<bool>>,
}

impl DnsLookups {
  pub fn new(config: &DnsDefinition, resolver: Arc<dyn Resolver>) -> Self {
    Self {
      resolver,
      timeout: config.timeout,
      cache_ttl: config.cache_ttl,
      negative_ttl: config.negative_ttl,
      reverse: cache(config.max_entries),
      listings: cache(config.max_entries),
    }
  }

  /// Creates lookups that use the DNS servers of the system configuration.
  pub fn system(config: &DnsDefinition) -> anyhow::Result<Self> {
    Ok(Self::new(
      config,
      Arc::new(hickory::SystemResolver::new(config.timeout)?),
    ))
  }

  /// Fills `into` with the results the plan asks for. A lookup that fails or times
  /// out leaves its entry unset.
  pub async fn resolve(&self, ip: IpAddr, plan: &LookupPlan, into: &mut Lookups) {
    let ip = ip.to_canonical();
    let reverse = async {
      if plan.reverse_dns {
        self.reverse_dns(ip).await
      } else {
        None
      }
    };
    let listings = futures_util::future::join_all(
      plan
        .dnsbl_zones
        .iter()
        .map(|zone| async move { (zone.clone(), self.listed(ip, zone).await) }),
    );
    let (reverse, listings) = tokio::join!(reverse, listings);
    if reverse.is_some() {
      into.reverse_dns = reverse;
    }
    for (zone, listed) in listings {
      if let Some(listed) = listed {
        into.dnsbl.insert(zone, listed);
      }
    }
  }

  fn ttl(&self, positive: bool) -> Duration {
    if positive {
      self.cache_ttl
    } else {
      self.negative_ttl
    }
  }

  /// The forward-confirmed PTR name, `Some(None)` when the client has none and `None`
  /// when the lookup did not finish.
  async fn reverse_dns(&self, ip: IpAddr) -> Option<Option<String>> {
    self
      .reverse
      .try_get_with(ip, async {
        let name = tokio::time::timeout(self.timeout, self.confirmed_name(ip))
          .await
          .map_err(|_| LookupError::Failed)??;
        Ok::<_, LookupError>(Entry {
          ttl: self.ttl(name.is_some()),
          value: name,
        })
      })
      .await
      .ok()
      .map(|entry| entry.value)
  }

  async fn confirmed_name(&self, ip: IpAddr) -> Result<Option<String>, LookupError> {
    let names = match self.resolver.ptr(ip).await {
      Ok(names) => names,
      Err(LookupError::NotFound) => return Ok(None),
      Err(LookupError::Failed) => return Err(LookupError::Failed),
    };
    let mut failed = false;
    for name in names.into_iter().take(MAX_PTR_NAMES) {
      match self.resolver.addresses(&name).await {
        Ok(addresses) if addresses.iter().any(|found| found.to_canonical() == ip) => {
          return Ok(Some(name));
        }
        Ok(_) | Err(LookupError::NotFound) => {}
        Err(LookupError::Failed) => failed = true,
      }
    }
    if failed {
      Err(LookupError::Failed)
    } else {
      Ok(None)
    }
  }

  /// Whether the zone lists the address, or `None` when the lookup did not finish.
  async fn listed(&self, ip: IpAddr, zone: &str) -> Option<bool> {
    self
      .listings
      .try_get_with((ip, zone.to_string()), async {
        let query = dnsbl_query(ip, zone);
        let answer = tokio::time::timeout(self.timeout, self.resolver.ipv4(&query))
          .await
          .map_err(|_| LookupError::Failed)?;
        let listed = match answer {
          Ok(addresses) => addresses.iter().any(|address| address.octets()[0] == 127),
          Err(LookupError::NotFound) => false,
          Err(LookupError::Failed) => return Err(LookupError::Failed),
        };
        Ok::<_, LookupError>(Entry {
          ttl: self.ttl(listed),
          value: listed,
        })
      })
      .await
      .ok()
      .map(|entry| entry.value)
  }
}

/// The DNSBL query name: the reversed octets of an IPv4 address or the reversed
/// nibbles of an IPv6 address, followed by the zone.
fn dnsbl_query(ip: IpAddr, zone: &str) -> String {
  let mut name = String::new();
  match ip {
    IpAddr::V4(ip) => {
      for octet in ip.octets().iter().rev() {
        let _ = write!(name, "{octet}.");
      }
    }
    IpAddr::V6(ip) => {
      for octet in ip.octets().iter().rev() {
        let _ = write!(name, "{:x}.{:x}.", octet & 0x0f, octet >> 4);
      }
    }
  }
  name.push_str(zone);
  name
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::collections::HashMap;
  use std::sync::Mutex;

  #[derive(Default)]
  struct FakeResolver {
    ptr: HashMap<IpAddr, Result<Vec<String>, LookupError>>,
    addresses: HashMap<String, Result<Vec<IpAddr>, LookupError>>,
    ipv4: HashMap<String, Result<Vec<Ipv4Addr>, LookupError>>,
    delay: Duration,
    calls: Mutex<Vec<String>>,
  }

  impl FakeResolver {
    fn record(&self, call: String) {
      self.calls.lock().unwrap().push(call);
    }
    fn calls(&self) -> Vec<String> {
      self.calls.lock().unwrap().clone()
    }
  }

  #[async_trait]
  impl Resolver for FakeResolver {
    async fn ptr(&self, ip: IpAddr) -> Result<Vec<String>, LookupError> {
      self.record(format!("ptr {ip}"));
      tokio::time::sleep(self.delay).await;
      self
        .ptr
        .get(&ip)
        .cloned()
        .unwrap_or(Err(LookupError::NotFound))
    }
    async fn addresses(&self, name: &str) -> Result<Vec<IpAddr>, LookupError> {
      self.record(format!("addresses {name}"));
      self
        .addresses
        .get(name)
        .cloned()
        .unwrap_or(Err(LookupError::NotFound))
    }
    async fn ipv4(&self, name: &str) -> Result<Vec<Ipv4Addr>, LookupError> {
      self.record(format!("ipv4 {name}"));
      self
        .ipv4
        .get(name)
        .cloned()
        .unwrap_or(Err(LookupError::NotFound))
    }
  }

  const CLIENT: &str = "192.0.2.5";

  fn lookups(resolver: &Arc<FakeResolver>) -> DnsLookups {
    let config = DnsDefinition {
      timeout: Duration::from_millis(100),
      ..Default::default()
    };
    DnsLookups::new(&config, resolver.clone())
  }

  fn reverse_plan() -> LookupPlan {
    LookupPlan {
      dnsbl_zones: vec![],
      reverse_dns: true,
    }
  }

  fn zone_plan(zone: &str) -> LookupPlan {
    LookupPlan {
      dnsbl_zones: vec![zone.to_string()],
      reverse_dns: false,
    }
  }

  async fn run(dns: &DnsLookups, ip: &str, plan: &LookupPlan) -> Lookups {
    let mut results = Lookups::NONE;
    dns.resolve(ip.parse().unwrap(), plan, &mut results).await;
    results
  }

  #[tokio::test]
  async fn reverse_dns_requires_forward_confirmation() {
    let client: IpAddr = CLIENT.parse().unwrap();
    let mut resolver = FakeResolver::default();
    resolver
      .ptr
      .insert(client, Ok(vec!["a.googlebot.com".into()]));
    resolver.addresses.insert(
      "a.googlebot.com".into(),
      Ok(vec!["192.0.2.99".parse().unwrap()]),
    );
    let resolver = Arc::new(resolver);
    let results = run(&lookups(&resolver), CLIENT, &reverse_plan()).await;
    assert_eq!(results.reverse_dns, Some(None));

    let mut resolver = FakeResolver::default();
    resolver
      .ptr
      .insert(client, Ok(vec!["a.googlebot.com".into()]));
    resolver
      .addresses
      .insert("a.googlebot.com".into(), Ok(vec![client]));
    let resolver = Arc::new(resolver);
    let results = run(&lookups(&resolver), CLIENT, &reverse_plan()).await;
    assert_eq!(results.reverse_dns, Some(Some("a.googlebot.com".into())));
  }

  #[tokio::test]
  async fn ipv4_mapped_clients_resolve_as_ipv4() {
    let client: IpAddr = CLIENT.parse().unwrap();
    let mut resolver = FakeResolver::default();
    resolver
      .ptr
      .insert(client, Ok(vec!["a.googlebot.com".into()]));
    resolver
      .addresses
      .insert("a.googlebot.com".into(), Ok(vec![client]));
    let resolver = Arc::new(resolver);
    let results = run(&lookups(&resolver), "::ffff:192.0.2.5", &reverse_plan()).await;
    assert_eq!(results.reverse_dns, Some(Some("a.googlebot.com".into())));
  }

  #[tokio::test]
  async fn resolver_failure_leaves_the_result_unresolved() {
    let client: IpAddr = CLIENT.parse().unwrap();
    let mut failing = FakeResolver::default();
    failing.ptr.insert(client, Err(LookupError::Failed));
    failing
      .ipv4
      .insert("5.2.0.192.dnsbl.example".into(), Err(LookupError::Failed));
    let failing = Arc::new(failing);
    let plan = LookupPlan {
      dnsbl_zones: vec!["dnsbl.example".into()],
      reverse_dns: true,
    };
    let results = run(&lookups(&failing), CLIENT, &plan).await;
    assert_eq!(results, Lookups::NONE);

    let slow = Arc::new(FakeResolver {
      delay: Duration::from_millis(500),
      ptr: [(client, Ok(vec!["a.googlebot.com".to_string()]))].into(),
      ..Default::default()
    });
    let results = run(&lookups(&slow), CLIENT, &reverse_plan()).await;
    assert_eq!(results.reverse_dns, None);
  }

  #[tokio::test]
  async fn dnsbl_query_uses_the_reversed_address() {
    let mut resolver = FakeResolver::default();
    resolver.ipv4.insert(
      "5.2.0.192.dnsbl.example".into(),
      Ok(vec!["127.0.0.2".parse().unwrap()]),
    );
    resolver.ipv4.insert(
      "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.b.d.0.1.0.0.2.dnsbl.example".into(),
      Ok(vec!["127.0.0.4".parse().unwrap()]),
    );
    resolver.ipv4.insert(
      "6.2.0.192.dnsbl.example".into(),
      Ok(vec!["192.0.2.1".parse().unwrap()]),
    );
    let resolver = Arc::new(resolver);
    let dns = lookups(&resolver);
    let plan = zone_plan("dnsbl.example");
    for (ip, expected) in [
      (CLIENT, true),
      ("2001:db8::1", true),
      ("192.0.2.6", false),
      ("192.0.2.7", false),
    ] {
      let results = run(&dns, ip, &plan).await;
      assert_eq!(results.dnsbl.get("dnsbl.example"), Some(&expected), "{ip}");
    }
  }

  #[tokio::test]
  async fn results_are_cached() {
    let client: IpAddr = CLIENT.parse().unwrap();
    let mut resolver = FakeResolver::default();
    resolver
      .ptr
      .insert(client, Ok(vec!["a.googlebot.com".into()]));
    resolver
      .addresses
      .insert("a.googlebot.com".into(), Ok(vec![client]));
    let resolver = Arc::new(resolver);
    let dns = lookups(&resolver);
    let plan = LookupPlan {
      dnsbl_zones: vec!["dnsbl.example".into()],
      reverse_dns: true,
    };
    let first = run(&dns, CLIENT, &plan).await;
    let calls = resolver.calls().len();
    assert_eq!(run(&dns, CLIENT, &plan).await, first);
    assert_eq!(resolver.calls().len(), calls);
  }
}
