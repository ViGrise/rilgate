use std::net::{IpAddr, Ipv6Addr};

pub(super) fn key(ip: IpAddr) -> IpAddr {
  match ip.to_canonical() {
    IpAddr::V6(ip) => IpAddr::V6(Ipv6Addr::from(u128::from(ip) & (u128::MAX << 64))),
    ip => ip,
  }
}
