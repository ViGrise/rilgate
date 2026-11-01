use crate::{
  FIRST_RETRY, FeedClient, FetchOptions, Fetched, GeoState, Outcome, next_delay, resume_delay,
};
use anyhow::{Context, Result, bail, ensure};
use rilgate_policy::geoip::MAX_DATABASE_BYTES;
use rilgate_policy::{GeoDatabase, GeoDatabases, GeoSource, Progress};
use std::io::Read;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt as _;

pub const MAXMIND_DOWNLOAD_BASE: &str = "https://download.maxmind.com/geoip/databases";

/// Loads GeoIP databases from local files and MaxMind downloads.
pub struct GeoRefresher {
  client: FeedClient,
  maxmind_base: String,
  max_bytes: usize,
}

impl GeoRefresher {
  pub fn new() -> Result<Self> {
    Ok(Self {
      client: FeedClient::new()?,
      maxmind_base: MAXMIND_DOWNLOAD_BASE.to_string(),
      max_bytes: MAX_DATABASE_BYTES,
    })
  }

  pub fn with_maxmind_base(mut self, base: impl Into<String>) -> Self {
    self.maxmind_base = base.into();
    self
  }

  pub fn with_max_bytes(mut self, max_bytes: usize) -> Self {
    self.max_bytes = max_bytes;
    self
  }

  /// Loads the database once, publishing it on success.
  pub async fn refresh_once(&self, database: &GeoDatabase, state: &mut GeoState) -> Outcome {
    match self.load(database, state).await {
      Ok(outcome) => outcome,
      Err(error) => {
        database.record_failure();
        tracing::warn!(
          database = database.kind().label(),
          error = format!("{error:#}"),
          "GeoIP database refresh failed"
        );
        Outcome::Failed
      }
    }
  }

  async fn load(&self, database: &GeoDatabase, state: &mut GeoState) -> Result<Outcome> {
    match database.source() {
      GeoSource::File(path) => {
        let metadata = tokio::fs::metadata(path)
          .await
          .with_context(|| format!("cannot read {}", path.display()))?;
        let modified = metadata.modified().ok();
        if modified.is_some() && modified == state.modified && database.status().loaded {
          database.renew();
          return Ok(Outcome::Unchanged);
        }
        let file = tokio::fs::File::open(path)
          .await
          .with_context(|| format!("cannot read {}", path.display()))?;
        let mut bytes = database_buffer(metadata.len(), self.max_bytes);
        file
          .take(self.max_bytes as u64 + 1)
          .read_to_end(&mut bytes)
          .await
          .with_context(|| format!("cannot read {}", path.display()))?;
        ensure!(
          bytes.len() <= self.max_bytes,
          "database exceeds {} bytes",
          self.max_bytes
        );
        database.publish(bytes)?;
        state.modified = modified;
        Ok(Outcome::Updated)
      }
      GeoSource::MaxMind(source) => {
        let url = format!(
          "{}/{}/download?suffix=tar.gz",
          self.maxmind_base.trim_end_matches('/'),
          source.edition
        );
        let options = FetchOptions {
          max_bytes: self.max_bytes,
          basic_auth: Some((&source.account_id, source.license_key.expose())),
        };
        match self
          .client
          .fetch_with(&url, &state.validators, &options)
          .await?
        {
          Fetched::NotModified => {
            database.renew();
            Ok(Outcome::Unchanged)
          }
          Fetched::Modified { body, validators } => {
            let max_bytes = self.max_bytes;
            let bytes = tokio::task::spawn_blocking(move || extract_database(&body, max_bytes))
              .await
              .context("archive extraction stopped")??;
            database.publish(bytes)?;
            state.validators = validators;
            Ok(Outcome::Updated)
          }
        }
      }
    }
  }
}

/// Returns the first `.mmdb` entry of a gzip-compressed tar archive.
fn extract_database(archive: &[u8], max_bytes: usize) -> Result<Vec<u8>> {
  let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(archive));
  for entry in archive.entries().context("invalid archive")? {
    let entry = entry.context("invalid archive")?;
    let is_database = entry
      .path()
      .context("invalid archive")?
      .extension()
      .is_some_and(|extension| extension == "mmdb");
    if !is_database {
      continue;
    }
    let mut bytes = database_buffer(entry.size(), max_bytes);
    entry
      .take(max_bytes as u64 + 1)
      .read_to_end(&mut bytes)
      .context("invalid archive")?;
    ensure!(
      bytes.len() <= max_bytes,
      "database exceeds {max_bytes} bytes"
    );
    return Ok(bytes);
  }
  bail!("archive contains no .mmdb file")
}

/// A buffer sized for a database of `expected` bytes. A database stays in
/// memory for as long as it is current, so reading into an exact allocation
/// keeps its footprint at its file size.
fn database_buffer(expected: u64, max_bytes: usize) -> Vec<u8> {
  Vec::with_capacity(expected.min(max_bytes as u64) as usize)
}

/// Starts a background refresher for every database in the registry.
///
/// Dropping the returned handle stops the refreshers.
pub fn spawn_geoip(databases: &GeoDatabases) -> Result<crate::Refreshers> {
  let targets: Vec<_> = databases.iter().map(Arc::downgrade).collect();
  if targets.is_empty() {
    return Ok(crate::Refreshers::default());
  }
  let refresher = Arc::new(GeoRefresher::new()?);
  let mut refreshers = crate::Refreshers::default();
  for target in targets {
    let refresher = refresher.clone();
    refreshers.tasks.push(tokio::spawn(async move {
      let Some(database) = target.upgrade() else {
        return;
      };
      let saved = database.progress();
      let mut state = saved.markers;
      let mut failures = saved.consecutive_failures;
      let first_delay = resume_delay(
        database.loaded_age(),
        failures,
        saved.next_attempt,
        database.refresh().max(FIRST_RETRY),
        fastrand::f64(),
      );
      drop(database);
      tokio::time::sleep(first_delay).await;
      loop {
        let Some(database) = target.upgrade() else {
          break;
        };
        let outcome = refresher.refresh_once(&database, &mut state).await;
        failures = match outcome {
          Outcome::Failed => failures.saturating_add(1),
          _ => 0,
        };
        let delay = retry_delay(&database, failures);
        database.save_progress(Progress {
          markers: state.clone(),
          consecutive_failures: failures,
          next_attempt: Some(Instant::now() + delay),
        });
        drop(database);
        tokio::time::sleep(delay).await;
      }
    }));
  }
  Ok(refreshers)
}

fn retry_delay(database: &GeoDatabase, failures: u32) -> Duration {
  // MaxMind limits downloads per day, so a download source backs off from the
  // slow retry interval even before its first success.
  let loaded = database.status().loaded || matches!(database.source(), GeoSource::MaxMind(_));
  let refresh = database.refresh().max(FIRST_RETRY);
  next_delay(failures, loaded, refresh, fastrand::f64())
}
