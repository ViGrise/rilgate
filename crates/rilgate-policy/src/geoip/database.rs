use crate::geoip::Lookup;
use crate::policy::schema::MaxMindSource;
use anyhow::{Context, Result, ensure};
use arc_swap::ArcSwap;
use maxminddb::{Reader, geoip2};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// The record type a database answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoKind {
  Country,
  Asn,
}

impl GeoKind {
  pub fn label(self) -> &'static str {
    match self {
      Self::Country => "country",
      Self::Asn => "asn",
    }
  }

  fn accepts(self, database_type: &str) -> bool {
    match self {
      Self::Country => database_type.contains("Country") || database_type.contains("City"),
      Self::Asn => database_type.contains("ASN"),
    }
  }
}

/// Where the bytes of a database come from.
#[derive(Debug, Clone)]
pub enum GeoSource {
  File(PathBuf),
  MaxMind(MaxMindSource),
}

#[derive(Default)]
struct Snapshot {
  reader: Option<Arc<Reader<Vec<u8>>>>,
  loaded_at: Option<Instant>,
  loaded_at_wall: Option<SystemTime>,
}

/// A MaxMind database queried by client address.
///
/// The reader is swapped atomically on publish, so lookups racing a refresh
/// observe one complete database. A database that never loaded, or that went
/// longer than `max_stale` without a successful refresh, answers unknown.
pub struct GeoDatabase {
  kind: GeoKind,
  source: GeoSource,
  refresh: Duration,
  max_stale: Duration,
  state: ArcSwap<Snapshot>,
  failures: AtomicU64,
}

impl std::fmt::Debug for GeoDatabase {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("GeoDatabase")
      .field("kind", &self.kind)
      .field("source", &self.source)
      .finish_non_exhaustive()
  }
}

impl GeoDatabase {
  pub(crate) fn new(
    kind: GeoKind,
    source: GeoSource,
    refresh: Duration,
    max_stale: Duration,
  ) -> Self {
    Self {
      kind,
      source,
      refresh,
      max_stale,
      state: ArcSwap::from_pointee(Snapshot::default()),
      failures: AtomicU64::new(0),
    }
  }

  pub fn kind(&self) -> GeoKind {
    self.kind
  }

  pub fn source(&self) -> &GeoSource {
    &self.source
  }

  pub fn refresh(&self) -> Duration {
    self.refresh
  }

  /// Parses `bytes` and publishes them as the current database.
  ///
  /// An invalid database leaves the previous reader in place.
  pub fn publish(&self, bytes: Vec<u8>) -> Result<()> {
    ensure!(
      bytes.len() <= super::MAX_DATABASE_BYTES,
      "database exceeds {} bytes",
      super::MAX_DATABASE_BYTES
    );
    let reader = Reader::from_source(bytes).context("invalid MaxMind database")?;
    let database_type = &reader.metadata().database_type;
    ensure!(
      self.kind.accepts(database_type),
      "database type {database_type} does not provide {} records",
      self.kind.label()
    );
    self.state.store(Arc::new(Snapshot {
      reader: Some(Arc::new(reader)),
      loaded_at: Some(Instant::now()),
      loaded_at_wall: Some(SystemTime::now()),
    }));
    Ok(())
  }

  /// Re-stamps the current database after the source reported no change.
  pub fn renew(&self) {
    let snapshot = self.state.load();
    if snapshot.reader.is_some() {
      self.state.store(Arc::new(Snapshot {
        reader: snapshot.reader.clone(),
        loaded_at: Some(Instant::now()),
        loaded_at_wall: Some(SystemTime::now()),
      }));
    }
  }

  pub fn record_failure(&self) {
    self.failures.fetch_add(1, Ordering::Relaxed);
  }

  pub fn status(&self) -> GeoStatus {
    let snapshot = self.state.load();
    let loaded = snapshot.reader.is_some();
    GeoStatus {
      loaded,
      stale: !loaded || self.is_stale(&snapshot),
      last_success: snapshot.loaded_at_wall,
      refresh_failures: self.failures.load(Ordering::Relaxed),
    }
  }

  /// Returns the ISO 3166-1 alpha-2 code recorded for `ip`.
  pub(crate) fn country(&self, ip: IpAddr) -> Lookup<Option<[u8; 2]>> {
    self.lookup(ip, |result| {
      let record = result.decode::<geoip2::Country>().ok()?;
      Some(record.and_then(|record| {
        let code = record.country.iso_code?.as_bytes();
        <[u8; 2]>::try_from(code).ok()
      }))
    })
  }

  /// Returns the autonomous system number recorded for `ip`.
  pub(crate) fn asn(&self, ip: IpAddr) -> Lookup<Option<u32>> {
    self.lookup(ip, |result| {
      let record = result.decode::<geoip2::Asn>().ok()?;
      Some(record.and_then(|record| record.autonomous_system_number))
    })
  }

  fn lookup<T>(
    &self,
    ip: IpAddr,
    read: impl FnOnce(&maxminddb::LookupResult<'_, Vec<u8>>) -> Option<Option<T>>,
  ) -> Lookup<Option<T>> {
    let snapshot = self.state.load();
    let Some(reader) = &snapshot.reader else {
      return Lookup::Unknown;
    };
    if self.is_stale(&snapshot) {
      return Lookup::Unknown;
    }
    let Ok(result) = reader.lookup(ip.to_canonical()) else {
      return Lookup::Unknown;
    };
    match read(&result) {
      Some(value) => Lookup::Known(value),
      None => Lookup::Unknown,
    }
  }

  fn is_stale(&self, snapshot: &Snapshot) -> bool {
    snapshot
      .loaded_at
      .is_some_and(|at| at.elapsed() > self.max_stale)
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoStatus {
  pub loaded: bool,
  pub stale: bool,
  pub last_success: Option<SystemTime>,
  pub refresh_failures: u64,
}
