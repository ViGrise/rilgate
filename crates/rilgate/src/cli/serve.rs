use anyhow::{Context, Result};
use rilgate_config::Config;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{IpAddr, SocketAddr};
use tokio::net::TcpListener;

const BACKLOG: i32 = 1024;

pub async fn run(config: Config, overrides: rilgate_config::Overrides) -> Result<()> {
  let gateway = crate::app::build(config.clone()).await?;
  crate::reload::spawn_reloader(gateway.policy_handle(), config.clone(), overrides)?;
  let mut listeners = bind_all(
    &[
      config.listen_addresses.as_slice(),
      config.operations_listen_addresses.as_slice(),
    ]
    .concat(),
  )?;
  let operations_listeners = listeners.split_off(config.listen_addresses.len());
  for listener in &operations_listeners {
    tracing::info!(
      "Starting Rilgate operations endpoint on {}",
      listener.local_addr()?
    );
  }
  for listener in &listeners {
    tracing::info!("Starting Rilgate on {}", listener.local_addr()?);
  }
  gateway
    .serve(
      listeners,
      operations_listeners,
      crate::shutdown::shutdown_signal(),
    )
    .await
    .context("Gateway server failed")
}

/// Binds every address. An IPv6 wildcard accepts IPv4 too on every OS, except when the list
/// holds the IPv4 wildcard for the same port, which then serves IPv4 on its own socket.
pub fn bind_all(addresses: &[SocketAddr]) -> Result<Vec<TcpListener>> {
  addresses
    .iter()
    .map(|address| bind(*address, ipv6_only(*address, addresses)))
    .collect()
}

fn ipv6_only(address: SocketAddr, all: &[SocketAddr]) -> bool {
  match address.ip() {
    IpAddr::V4(_) => false,
    IpAddr::V6(ip) if ip.is_unspecified() => all.iter().any(|other| {
      other.port() == address.port() && other.ip().is_unspecified() && other.is_ipv4()
    }),
    IpAddr::V6(_) => true,
  }
}

fn bind(address: SocketAddr, ipv6_only: bool) -> Result<TcpListener> {
  let open = || -> std::io::Result<TcpListener> {
    let socket = Socket::new(
      Domain::for_address(address),
      Type::STREAM,
      Some(Protocol::TCP),
    )?;
    #[cfg(unix)]
    socket.set_reuse_address(true)?;
    if address.is_ipv6() {
      socket.set_only_v6(ipv6_only)?;
    }
    socket.set_nonblocking(true)?;
    socket.bind(&address.into())?;
    socket.listen(BACKLOG)?;
    TcpListener::from_std(socket.into())
  };
  open().with_context(|| format!("binding {address}"))
}

#[cfg(test)]
mod tests {
  use super::*;
  use tokio::net::TcpStream;

  fn addr(text: &str) -> SocketAddr {
    text.parse().unwrap()
  }
  fn ipv6_unavailable(error: &anyhow::Error) -> bool {
    error
      .chain()
      .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
      .any(|io| {
        matches!(
          io.kind(),
          std::io::ErrorKind::AddrNotAvailable | std::io::ErrorKind::Unsupported
        ) || io
          .raw_os_error()
          .is_some_and(|code| code == 97 || code == 47)
      })
  }
  async fn free_port() -> u16 {
    let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
    probe.local_addr().unwrap().port()
  }

  #[test]
  fn ipv6_wildcard_is_ipv6_only_only_when_an_ipv4_wildcard_shares_the_port() {
    let both = [addr("0.0.0.0:80"), addr("[::]:80")];
    assert!(ipv6_only(both[1], &both));
    assert!(!ipv6_only(both[0], &both));
    let other_port = [addr("0.0.0.0:81"), addr("[::]:80")];
    assert!(!ipv6_only(other_port[1], &other_port));
    let loopback = [addr("127.0.0.1:80"), addr("[::]:80")];
    assert!(!ipv6_only(loopback[1], &loopback));
    assert!(ipv6_only(addr("[::1]:80"), &[addr("[::1]:80")]));
  }

  #[tokio::test]
  async fn ipv6_wildcard_alone_accepts_ipv4_connections() {
    let listeners = match bind_all(&[addr("[::]:0")]) {
      Ok(listeners) => listeners,
      Err(error) if ipv6_unavailable(&error) => return,
      Err(error) => panic!("{error:#}"),
    };
    let port = listeners[0].local_addr().unwrap().port();
    let (accepted, connected) = tokio::join!(
      listeners[0].accept(),
      TcpStream::connect(("127.0.0.1", port))
    );
    accepted.unwrap();
    connected.unwrap();
  }

  #[tokio::test]
  async fn ipv4_and_ipv6_wildcards_share_a_port_and_each_accepts_its_family() {
    let port = free_port().await;
    let listeners = match bind_all(&[
      addr(&format!("0.0.0.0:{port}")),
      addr(&format!("[::]:{port}")),
    ]) {
      Ok(listeners) => listeners,
      Err(error) if ipv6_unavailable(&error) => return,
      Err(error) => panic!("{error:#}"),
    };
    let (v4, v4_client) = tokio::join!(
      listeners[0].accept(),
      TcpStream::connect(("127.0.0.1", port))
    );
    v4.unwrap();
    v4_client.unwrap();
    let (v6, v6_client) = tokio::join!(listeners[1].accept(), TcpStream::connect(("::1", port)));
    v6.unwrap();
    v6_client.unwrap();
  }

  #[tokio::test]
  async fn loopback_listeners_of_both_families_bind_together() {
    let listeners = match bind_all(&[addr("127.0.0.1:0"), addr("[::1]:0")]) {
      Ok(listeners) => listeners,
      Err(error) if ipv6_unavailable(&error) => return,
      Err(error) => panic!("{error:#}"),
    };
    for listener in &listeners {
      let local = listener.local_addr().unwrap();
      let (accepted, connected) = tokio::join!(listener.accept(), TcpStream::connect(local));
      accepted.unwrap();
      connected.unwrap();
    }
  }

  #[tokio::test]
  async fn bind_errors_name_the_address() {
    let taken = bind_all(&[addr("127.0.0.1:0")]).unwrap();
    let local = taken[0].local_addr().unwrap();
    let error = bind_all(&[local]).unwrap_err();
    assert!(
      error.to_string().starts_with(&format!("binding {local}")),
      "{error:#}"
    );
  }
}
