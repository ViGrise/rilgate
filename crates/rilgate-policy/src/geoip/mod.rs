mod database;
mod registry;
pub use database::{GeoDatabase, GeoKind, GeoSource, GeoStatus};
pub use registry::GeoDatabases;
use std::time::Duration;

pub const MAX_DATABASE_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_LIST_ENTRIES: usize = 256;
pub const DEFAULT_REFRESH: Duration = Duration::from_secs(24 * 3600);
pub const MIN_MAXMIND_REFRESH: Duration = Duration::from_secs(3600);
pub const MIN_FILE_REFRESH: Duration = Duration::from_secs(60);
pub const MAX_REFRESH: Duration = Duration::from_secs(7 * 24 * 3600);
pub const DEFAULT_MAX_STALE: Duration = Duration::from_secs(30 * 24 * 3600);
pub const MAX_MAX_STALE: Duration = Duration::from_secs(90 * 24 * 3600);

/// The answer of a database for one address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lookup<T> {
  /// The database is not loaded, is stale or cannot answer.
  Unknown,
  Known(T),
}
