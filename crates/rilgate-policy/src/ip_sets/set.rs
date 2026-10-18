use crate::ip_sets::IpRanges;
use arc_swap::ArcSwap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Membership {
  Member,
  NotMember,
  Unknown,
}

#[derive(Debug)]
struct Snapshot {
  ranges: Option<Arc<IpRanges>>,
}

/// A named address set evaluated against the client address.
///
/// The membership snapshot is swapped atomically, so evaluations racing a
/// publish always observe one complete snapshot.
#[derive(Debug)]
pub struct IpSet {
  name: String,
  state: ArcSwap<Snapshot>,
}

impl IpSet {
  pub(crate) fn static_set(name: String, ranges: IpRanges) -> Self {
    Self {
      name,
      state: ArcSwap::from_pointee(Snapshot {
        ranges: Some(Arc::new(ranges)),
      }),
    }
  }

  pub fn name(&self) -> &str {
    &self.name
  }

  pub fn membership(&self, ip: IpAddr) -> Membership {
    self.membership_at(ip, Instant::now())
  }

  pub fn membership_at(&self, ip: IpAddr, _now: Instant) -> Membership {
    let snapshot = self.state.load();
    match &snapshot.ranges {
      Some(ranges) if ranges.contains(ip) => Membership::Member,
      Some(_) => Membership::NotMember,
      None => Membership::Unknown,
    }
  }
}
