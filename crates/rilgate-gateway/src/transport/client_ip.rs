use http::HeaderMap;
use rilgate_config::ServerConfig;
use std::net::IpAddr;

/// Whether the immediate socket peer is one of the configured trusted proxies.
pub fn trusted_peer(peer: Option<IpAddr>, cfg: &ServerConfig) -> bool {
  peer.is_some_and(|peer| {
    let peer = peer.to_canonical();
    cfg.trusted_proxies.iter().any(|net| net.contains(&peer))
  })
}

pub fn client_ip(
  peer: Option<IpAddr>,
  headers: &HeaderMap,
  cfg: &ServerConfig,
) -> Result<Option<IpAddr>, &'static str> {
  let Some(peer) = peer else {
    return Ok(None);
  };
  let peer = peer.to_canonical();
  let trusted = |ip: &IpAddr| cfg.trusted_proxies.iter().any(|net| net.contains(ip));
  if !trusted(&peer) {
    return Ok(Some(peer));
  }
  if let Some(name) = &cfg.client_ip_header {
    let mut values = headers.get_all(name).iter();
    if let Some(value) = values.next() {
      if values.next().is_some() {
        return Err("Invalid forwarded client address");
      }
      let address: IpAddr = value
        .to_str()
        .map_err(|_| "Invalid forwarded client address")?
        .parse()
        .map_err(|_| "Invalid forwarded client address")?;
      return Ok(Some(address.to_canonical()));
    }
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
      current = raw
        .trim()
        .parse()
        .map_err(|_| "Invalid forwarded client address")?;
      current = current.to_canonical();
    }
  }
  Ok(Some(current))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn trusted_header_config() -> ServerConfig {
    ServerConfig {
      trusted_proxies: vec!["127.0.0.1/32".parse().unwrap()],
      client_ip_header: Some("cf-connecting-ip".parse().unwrap()),
      ..Default::default()
    }
  }

  #[test]
  fn client_ip_header_from_a_trusted_peer_names_the_client() {
    let mut headers = HeaderMap::new();
    headers.insert("cf-connecting-ip", "203.0.113.7".parse().unwrap());
    headers.insert(
      "x-forwarded-for",
      "203.0.113.7, 198.51.100.1".parse().unwrap(),
    );
    assert_eq!(
      client_ip(
        Some("127.0.0.1".parse().unwrap()),
        &headers,
        &trusted_header_config()
      ),
      Ok(Some("203.0.113.7".parse().unwrap()))
    );
  }

  #[test]
  fn client_ip_header_from_an_untrusted_peer_is_ignored() {
    let mut headers = HeaderMap::new();
    headers.insert("cf-connecting-ip", "203.0.113.7".parse().unwrap());
    headers.insert(
      "x-forwarded-for",
      "203.0.113.7, 198.51.100.1".parse().unwrap(),
    );
    assert_eq!(
      client_ip(
        Some("192.0.2.9".parse().unwrap()),
        &headers,
        &trusted_header_config()
      ),
      Ok(Some("192.0.2.9".parse().unwrap()))
    );
  }

  #[test]
  fn missing_client_ip_header_falls_back_to_forwarded_for() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", "203.0.113.8".parse().unwrap());
    assert_eq!(
      client_ip(
        Some("127.0.0.1".parse().unwrap()),
        &headers,
        &trusted_header_config()
      ),
      Ok(Some("203.0.113.8".parse().unwrap()))
    );
  }

  #[test]
  fn invalid_client_ip_header_is_rejected() {
    let mut headers = HeaderMap::new();
    headers.insert(
      "cf-connecting-ip",
      "203.0.113.7, 198.51.100.1".parse().unwrap(),
    );
    assert_eq!(
      client_ip(
        Some("127.0.0.1".parse().unwrap()),
        &headers,
        &trusted_header_config()
      ),
      Err("Invalid forwarded client address")
    );
    headers.insert("cf-connecting-ip", "203.0.113.7".parse().unwrap());
    headers.append("cf-connecting-ip", "198.51.100.1".parse().unwrap());
    assert_eq!(
      client_ip(
        Some("127.0.0.1".parse().unwrap()),
        &headers,
        &trusted_header_config()
      ),
      Err("Invalid forwarded client address")
    );
  }

  #[test]
  fn client_ip_header_maps_ipv4_mapped_ipv6() {
    let mut headers = HeaderMap::new();
    headers.insert("cf-connecting-ip", "::ffff:203.0.113.7".parse().unwrap());
    assert_eq!(
      client_ip(
        Some("127.0.0.1".parse().unwrap()),
        &headers,
        &trusted_header_config()
      ),
      Ok(Some("203.0.113.7".parse().unwrap()))
    );
  }

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

  #[test]
  fn only_the_socket_peer_decides_whether_a_proxy_is_trusted() {
    let cfg = ServerConfig {
      trusted_proxies: vec!["10.0.0.0/8".parse().unwrap()],
      ..Default::default()
    };
    assert!(trusted_peer(Some("10.1.2.3".parse().unwrap()), &cfg));
    assert!(trusted_peer(Some("::ffff:10.1.2.3".parse().unwrap()), &cfg));
    assert!(!trusted_peer(Some("192.0.2.1".parse().unwrap()), &cfg));
    assert!(!trusted_peer(None, &cfg));
  }
}
