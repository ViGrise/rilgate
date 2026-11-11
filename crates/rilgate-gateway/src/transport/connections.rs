use super::rate_limit::client_network;
use std::{
  collections::{HashMap, HashSet},
  net::{IpAddr, SocketAddr},
  sync::{Arc, Mutex, Weak},
  time::Duration,
};
use tokio::{sync::watch, time::Instant};

#[derive(Clone, Copy, Debug)]
enum Phase {
  Header(Instant),
  Idle(Instant),
  Active(usize),
}

#[derive(Default, Debug)]
struct Entries {
  networks: HashMap<IpAddr, usize>,
  sockets: HashMap<(SocketAddr, SocketAddr), Weak<Connection>>,
}

#[derive(Debug)]
pub(crate) struct Connections {
  entries: Mutex<Entries>,
  limit: usize,
  prefix: u8,
  header_timeout: Duration,
  idle_timeout: Duration,
}

#[derive(Debug)]
pub(crate) struct Connection {
  owner: Arc<Connections>,
  socket: (SocketAddr, SocketAddr),
  networks: Mutex<Networks>,
  phase: watch::Sender<Phase>,
}

#[derive(Default, Debug)]
struct Networks {
  held: HashSet<IpAddr>,
  active: HashMap<IpAddr, usize>,
}

impl Connections {
  pub fn new(
    limit: usize,
    prefix: u8,
    header_timeout: Duration,
    idle_timeout: Duration,
  ) -> Arc<Self> {
    Arc::new(Self {
      entries: Mutex::default(),
      limit,
      prefix,
      header_timeout,
      idle_timeout,
    })
  }

  pub fn accept(
    self: &Arc<Self>,
    local: SocketAddr,
    peer: SocketAddr,
    trusted: bool,
  ) -> Option<Arc<Connection>> {
    let connection = Arc::new(Connection {
      owner: self.clone(),
      socket: (local, peer),
      networks: Mutex::default(),
      phase: watch::channel(Phase::Header(Instant::now() + self.header_timeout)).0,
    });
    if !trusted && !connection.reserve(peer.ip()) {
      return None;
    }
    self
      .entries
      .lock()
      .unwrap()
      .sockets
      .insert((local, peer), Arc::downgrade(&connection));
    Some(connection)
  }

  pub fn get(&self, local: SocketAddr, peer: SocketAddr) -> Option<Arc<Connection>> {
    self
      .entries
      .lock()
      .unwrap()
      .sockets
      .get(&(local, peer))
      .and_then(Weak::upgrade)
  }
}

impl Connection {
  fn reserve(&self, ip: IpAddr) -> bool {
    let mut networks = self.networks.lock().unwrap();
    self.reserve_locked(&mut networks, ip)
  }

  fn reserve_locked(&self, networks: &mut Networks, ip: IpAddr) -> bool {
    let key = client_network(ip, self.owner.prefix);
    if networks.held.contains(&key) {
      return true;
    }
    let mut entries = self.owner.entries.lock().unwrap();
    let count = entries.networks.entry(key).or_default();
    if *count >= self.owner.limit {
      return false;
    }
    *count += 1;
    networks.held.insert(key);
    true
  }

  pub fn admit(&self, ip: IpAddr) -> bool {
    let mut networks = self.networks.lock().unwrap();
    if !self.reserve_locked(&mut networks, ip) {
      return false;
    }
    *networks
      .active
      .entry(client_network(ip, self.owner.prefix))
      .or_default() += 1;
    true
  }

  pub fn received(&self) {
    self.phase.send_if_modified(|phase| {
      if matches!(phase, Phase::Idle(_)) {
        *phase = Phase::Header(Instant::now() + self.owner.header_timeout);
        true
      } else {
        false
      }
    });
  }

  pub fn start_request(&self) {
    self.phase.send_modify(|phase| {
      *phase = Phase::Active(match phase {
        Phase::Active(count) => *count + 1,
        _ => 1,
      });
    });
  }

  pub fn finish_request(&self, ip: Option<IpAddr>) {
    {
      let mut networks = self.networks.lock().unwrap();
      let key = ip.map(|ip| client_network(ip, self.owner.prefix));
      if let Some(key) = key
        && let Some(count) = networks.active.get_mut(&key)
      {
        *count -= 1;
        if *count == 0 {
          networks.active.remove(&key);
        }
      }
      // Active networks and the last idle client retain their connection slots.
      let keep_idle = networks.active.is_empty();
      let released: Vec<_> = networks
        .held
        .iter()
        .filter(|network| {
          !networks.active.contains_key(network) && !(keep_idle && Some(**network) == key)
        })
        .copied()
        .collect();
      let mut entries = self.owner.entries.lock().unwrap();
      for network in released {
        networks.held.remove(&network);
        if let Some(count) = entries.networks.get_mut(&network) {
          *count -= 1;
          if *count == 0 {
            entries.networks.remove(&network);
          }
        }
      }
    }
    self.phase.send_modify(|phase| {
      *phase = match phase {
        Phase::Active(count) if *count > 1 => Phase::Active(*count - 1),
        _ => Phase::Idle(Instant::now() + self.owner.idle_timeout),
      };
    });
  }

  pub async fn expired(&self) {
    let mut changes = self.phase.subscribe();
    loop {
      let phase = *changes.borrow_and_update();
      let deadline = async {
        match phase {
          Phase::Header(deadline) | Phase::Idle(deadline) => {
            tokio::time::sleep_until(deadline).await
          }
          Phase::Active(_) => std::future::pending().await,
        }
      };
      tokio::select! {
        _ = deadline => return,
        result = changes.changed() => if result.is_err() { return; },
      }
    }
  }
}

impl Drop for Connection {
  fn drop(&mut self) {
    let mut entries = self.owner.entries.lock().unwrap();
    entries.sockets.remove(&self.socket);
    for network in self.networks.get_mut().unwrap().held.drain() {
      if let Some(count) = entries.networks.get_mut(&network) {
        *count -= 1;
        if *count == 0 {
          entries.networks.remove(&network);
        }
      }
    }
  }
}
