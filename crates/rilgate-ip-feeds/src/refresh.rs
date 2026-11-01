use crate::{
  FIRST_LOAD_RETRY, FIRST_RETRY, FeedClient, FeedParse, Fetched, JITTER_FRACTION, Validators,
};
use anyhow::Result;
use rilgate_policy::{FeedSet, FeedSets, Progress};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The result of a single refresh cycle.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
  Updated,
  Unchanged,
  Failed,
}

/// Fetches a remote set once, publishing a new snapshot on success.
pub async fn refresh_once<I: FeedParse>(
  client: &FeedClient,
  set: &FeedSet<I>,
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
    }) => {
      let body = body.clone();
      let format = remote.format.clone();
      match tokio::task::spawn_blocking(move || I::parse(&format, &body))
        .await
        .unwrap_or_else(|_| Err(anyhow::anyhow!("parser stopped")))
      {
        Ok(ranges) => {
          *validators = new;
          set.publish(ranges);
          Outcome::Updated
        }
        Err(error) => {
          set.record_failure();
          tracing::warn!(
            set = set.name(),
            error = format!("{error:#}"),
            "{} refresh failed",
            I::LABEL
          );
          Outcome::Failed
        }
      }
    }
    Err(error) => {
      set.record_failure();
      tracing::warn!(
        set = set.name(),
        error = format!("{error:#}"),
        "{} refresh failed",
        I::LABEL
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

/// The delay before the first fetch of a refresher that takes over loaded data.
///
/// Data that loaded without a later failure is due one refresh interval after
/// its last success. Otherwise the saved retry time applies. Data that never
/// loaded and has no saved retry time is due at once.
pub fn resume_delay(
  age: Option<Duration>,
  consecutive_failures: u32,
  next_attempt: Option<Instant>,
  refresh: Duration,
  jitter: f64,
) -> Duration {
  match (age, consecutive_failures, next_attempt) {
    (Some(age), 0, _) => next_delay(0, true, refresh.saturating_sub(age), jitter),
    (_, _, Some(at)) => at.saturating_duration_since(Instant::now()),
    _ => Duration::ZERO,
  }
}

/// Starts a background refresher for every remote set in the registry.
///
/// Dropping the returned handle stops the refreshers.
pub fn spawn<I: FeedParse>(sets: &FeedSets<I>) -> Result<crate::Refreshers> {
  let remotes: Vec<_> = sets
    .iter()
    .filter_map(|set| Some((Arc::downgrade(set), set.remote()?.clone())))
    .collect();
  if remotes.is_empty() {
    return Ok(crate::Refreshers::default());
  }
  let client = Arc::new(FeedClient::new()?);
  let mut refreshers = crate::Refreshers::default();
  for (set, remote) in remotes {
    let client = client.clone();
    refreshers.tasks.push(tokio::spawn(async move {
      let Some(current) = set.upgrade() else {
        return;
      };
      let saved = current.progress();
      let mut validators = saved.markers;
      let mut failures = saved.consecutive_failures;
      let first_delay = resume_delay(
        current.status().age,
        failures,
        saved.next_attempt,
        remote.refresh,
        fastrand::f64(),
      );
      drop(current);
      tokio::time::sleep(first_delay).await;
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
        let delay = next_delay(failures, loaded, remote.refresh, fastrand::f64());
        set.save_progress(Progress {
          markers: validators.clone(),
          consecutive_failures: failures,
          next_attempt: Some(Instant::now() + delay),
        });
        drop(set);
        tokio::time::sleep(delay).await;
      }
    }));
  }
  Ok(refreshers)
}
