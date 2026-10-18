use ipnet::{IpNet, Ipv4Net};
use std::net::IpAddr;

/// A merged index of IP networks answering membership by binary search.
///
/// Addresses are canonicalized so an IPv4-mapped IPv6 address resolves to its
/// IPv4 form, and a network inside the IPv4-mapped prefix is folded into the
/// equivalent IPv4 network. Host bits are truncated and adjacent or
/// overlapping networks are merged into inclusive ranges.
#[derive(Debug, Clone, Default)]
pub struct IpRanges {
  v4: Vec<(u32, u32)>,
  v6: Vec<(u128, u128)>,
  prefixes: usize,
}

impl IpRanges {
  pub fn from_networks(networks: impl IntoIterator<Item = IpNet>) -> Self {
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    let mut prefixes = 0;
    for network in networks {
      prefixes += 1;
      match canonical_network(network) {
        IpNet::V4(net) => v4.push((u32::from(net.network()), u32::from(net.broadcast()))),
        IpNet::V6(net) => v6.push((u128::from(net.network()), u128::from(net.broadcast()))),
      }
    }
    merge(&mut v4, |end: u32| end.saturating_add(1));
    merge(&mut v6, |end: u128| end.saturating_add(1));
    Self { v4, v6, prefixes }
  }

  pub fn contains(&self, ip: IpAddr) -> bool {
    match ip.to_canonical() {
      IpAddr::V4(ip) => contains_in(&self.v4, u32::from(ip)),
      IpAddr::V6(ip) => contains_in(&self.v6, u128::from(ip)),
    }
  }

  /// Number of networks passed to [`IpRanges::from_networks`], before merging.
  pub fn prefix_count(&self) -> usize {
    self.prefixes
  }
}

fn canonical_network(network: IpNet) -> IpNet {
  let network = network.trunc();
  if let IpNet::V6(net) = network
    && net.prefix_len() >= 96
    && let Some(addr) = net.addr().to_ipv4_mapped()
  {
    let prefix = net.prefix_len() - 96;
    return IpNet::V4(Ipv4Net::new(addr, prefix).expect("mapped prefix length within 0..=32"));
  }
  network
}

fn contains_in<T: Ord + Copy>(ranges: &[(T, T)], value: T) -> bool {
  let index = ranges.partition_point(|(start, _)| *start <= value);
  index > 0 && ranges[index - 1].1 >= value
}

fn merge<T: Ord + Copy>(ranges: &mut Vec<(T, T)>, next: impl Fn(T) -> T) {
  ranges.sort_unstable();
  let mut merged: Vec<(T, T)> = Vec::with_capacity(ranges.len());
  for (start, end) in ranges.drain(..) {
    match merged.last_mut() {
      Some(last) if start <= next(last.1) => {
        if end > last.1 {
          last.1 = end;
        }
      }
      _ => merged.push((start, end)),
    }
  }
  *ranges = merged;
}
