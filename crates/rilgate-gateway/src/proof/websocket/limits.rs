use std::{
  collections::HashMap,
  net::IpAddr,
  sync::{Arc, Mutex},
};

#[derive(Default)]
pub struct SessionLimits(Mutex<HashMap<IpAddr, usize>>);
pub struct SessionPermit {
  limits: Arc<SessionLimits>,
  network: IpAddr,
}
impl SessionLimits {
  pub fn acquire(
    self: &Arc<Self>,
    client: IpAddr,
    ipv6_prefix: u8,
    max: usize,
  ) -> Option<SessionPermit> {
    let network = crate::transport::rate_limit::client_network(client, ipv6_prefix);
    let mut clients = self.0.lock().unwrap_or_else(|error| error.into_inner());
    let count = clients.entry(network).or_default();
    if *count >= max {
      return None;
    }
    *count += 1;
    Some(SessionPermit {
      limits: self.clone(),
      network,
    })
  }
}
impl Drop for SessionPermit {
  fn drop(&mut self) {
    let mut clients = self
      .limits
      .0
      .lock()
      .unwrap_or_else(|error| error.into_inner());
    let count = clients
      .get_mut(&self.network)
      .expect("Active session network");
    *count -= 1;
    if *count == 0 {
      clients.remove(&self.network);
    }
  }
}
