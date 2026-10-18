use http::HeaderMap;
use rilgate_config::ServerConfig;
use std::net::IpAddr;

pub fn client_ip(
  peer: Option<IpAddr>,
  headers: &HeaderMap,
  cfg: &ServerConfig,
) -> Result<Option<IpAddr>, &'static str> {
  let Some(peer) = peer else {
    return Ok(None);
  };
  let peer = normalize(peer);
  let trusted = |ip: &IpAddr| cfg.trusted_proxies.iter().any(|net| net.contains(ip));
  if !trusted(&peer) {
    return Ok(Some(peer));
  }
  let mut current = peer;
  let mut count = 0;
  // Walk from the socket towards the client; only trusted hops may assert another address.
  for value in headers.get_all("x-forwarded-for").iter().rev() {
    let value = value
      .to_str()
      .map_err(|_| "Invalid forwarded client address")?;
    if value.len() > 2048 {
      return Err("Forwarded client chain is too long");
    }
    for raw in value.rsplit(',') {
      if !trusted(&current) {
        return Ok(Some(current));
      }
      count += 1;
      if count > 32 {
        return Err("Forwarded client chain is too long");
      }
      current = normalize(
        raw
          .trim()
          .parse()
          .map_err(|_| "Invalid forwarded client address")?,
      );
    }
  }
  Ok(Some(current))
}

fn normalize(ip: IpAddr) -> IpAddr {
  match ip {
    IpAddr::V6(value) => value.to_ipv4_mapped().map_or(ip, IpAddr::V4),
    _ => ip,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn trust_stops_at_the_nearest_untrusted_hop() {
    let cfg = ServerConfig {
      trusted_proxies: vec!["10.0.0.0/8".parse().unwrap()],
      ..Default::default()
    };
    let mut headers = HeaderMap::new();
    headers.insert(
      "x-forwarded-for",
      "192.0.2.9, 198.51.100.1, 10.0.0.2".parse().unwrap(),
    );
    assert_eq!(
      client_ip(Some("10.0.0.1".parse().unwrap()), &headers, &cfg).unwrap(),
      Some("198.51.100.1".parse().unwrap())
    );
    headers.insert("x-forwarded-for", "malformed".parse().unwrap());
    assert_eq!(
      client_ip(Some("192.0.2.1".parse().unwrap()), &headers, &cfg).unwrap(),
      Some("192.0.2.1".parse().unwrap())
    );
    assert!(client_ip(Some("10.0.0.1".parse().unwrap()), &headers, &cfg).is_err());
    assert_eq!(
      client_ip(Some("::ffff:192.0.2.1".parse().unwrap()), &headers, &cfg).unwrap(),
      Some("192.0.2.1".parse().unwrap())
    );
  }
}
