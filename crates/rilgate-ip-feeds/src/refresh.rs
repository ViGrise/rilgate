use crate::{
  FIRST_LOAD_RETRY, FIRST_RETRY, FeedClient, Fetched, JITTER_FRACTION, Validators, parse,
};
use anyhow::Result;
use rilgate_policy::{IpSet, IpSets};
use std::sync::Arc;
use std::time::Duration;

/// The result of a single refresh cycle.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
  Updated,
  Unchanged,
  Failed,
}

/// Fetches a remote set once, publishing a new snapshot on success.
pub async fn refresh_once(
  client: &FeedClient,
  set: &IpSet,
  validators: &mut Validators,
) -> Outcome {
  let Some(remote) = set.remote().cloned() else {
    return Outcome::Failed;
  };
  match client.fetch(&remote.url, validators).await {
    Ok(Fetched::NotModified) => {
      set.renew();
      Outcome::Unchanged
    }
    Ok(Fetched::Modified {
      body,
      validators: new,
    }) => match parse(remote.format, &body) {
      Ok(ranges) => {
        *validators = new;
        set.publish(ranges);
        Outcome::Updated
      }
      Err(error) => {
        set.record_failure();
        tracing::warn!(
          ip_set = set.name(),
          error = format!("{error:#}"),
          "IP set refresh failed"
        );
        Outcome::Failed
      }
    },
    Err(error) => {
      set.record_failure();
      tracing::warn!(
        ip_set = set.name(),
        error = format!("{error:#}"),
        "IP set refresh failed"
      );
      Outcome::Failed
    }
  }
}

/// The delay before the next fetch after a number of consecutive failures.
///
/// A set that has never loaded retries quickly, up to [`FIRST_RETRY`], because
/// it holds the instance out of readiness. A loaded set keeps serving its last
/// snapshot and backs off from [`FIRST_RETRY`] up to the refresh interval.
/// Every delay adds up to [`JITTER_FRACTION`] so instances drift apart.
pub fn next_delay(
  consecutive_failures: u32,
  loaded: bool,
  refresh: Duration,
  jitter: f64,
) -> Duration {
  let base = if consecutive_failures == 0 {
    refresh
  } else {
    let factor = 1u32
      .checked_shl(consecutive_failures - 1)
      .unwrap_or(u32::MAX);
    if loaded {
      FIRST_RETRY.saturating_mul(factor).min(refresh)
    } else {
      FIRST_LOAD_RETRY.saturating_mul(factor).min(FIRST_RETRY)
    }
  };
  base + base.mul_f64(JITTER_FRACTION * jitter)
}

/// Starts a background refresher for every remote set in the registry.
///
/// Returns the number of refreshers started.
pub fn spawn(sets: &IpSets) -> Result<usize> {
  let remotes: Vec<_> = sets
    .iter()
    .filter_map(|set| Some((Arc::downgrade(set), set.remote()?.clone())))
    .collect();
  if remotes.is_empty() {
    return Ok(0);
  }
  let client = Arc::new(FeedClient::new()?);
  let started = remotes.len();
  for (set, remote) in remotes {
    let client = client.clone();
    tokio::spawn(async move {
      let mut validators = Validators::default();
      let mut failures = 0u32;
      loop {
        let Some(set) = set.upgrade() else {
          break;
        };
        let outcome = refresh_once(&client, &set, &mut validators).await;
        failures = match outcome {
          Outcome::Failed => failures.saturating_add(1),
          _ => 0,
        };
        let loaded = set.status().loaded;
        drop(set);
        let delay = next_delay(failures, loaded, remote.refresh, fastrand::f64());
        tokio::time::sleep(delay).await;
      }
    });
  }
  Ok(started)
}
