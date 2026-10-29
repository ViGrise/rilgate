use crate::{
  runtime::state::AppState,
  transport::response::{Response, response},
};
use http::{StatusCode, header::CONTENT_TYPE};
use std::fmt::Write;
use std::sync::atomic::Ordering;

pub fn exposition(state: &AppState) -> Response {
  let mut text = state.metrics.render();
  text.push_str(&state.pool.connection_metrics());
  let (fetch, capacity) = state.pool.failures();
  let web = state.web.diagnostics();
  for (name, value) in [
    (
      "connections_active",
      state.cfg.server.max_connections - state.connections.available_permits(),
    ),
    (
      "requests_active",
      state.cfg.server.max_requests - state.requests.available_permits(),
    ),
    ("pool_sessions_active", state.pool.active_sessions()),
  ] {
    let _ = writeln!(text, "# TYPE rilgate_{name} gauge\nrilgate_{name} {value}");
  }
  for (name, value) in [
    ("pool_fetch_failures", fetch),
    ("pool_capacity_failures", capacity),
    ("web_reloads", web.reload_total),
    ("web_reload_failures", web.reload_failures_total),
    (
      "policy_unresolved_decisions",
      state
        .metrics
        .policy_unresolved_decisions
        .load(Ordering::Relaxed),
    ),
    (
      "auth_binding_mismatches",
      state
        .metrics
        .auth_binding_mismatches
        .load(Ordering::Relaxed),
    ),
  ] {
    let _ = writeln!(
      text,
      "# TYPE rilgate_{name}_total counter\nrilgate_{name}_total {value}"
    );
  }
  let sets: Vec<_> = state
    .pow
    .ip_sets()
    .iter()
    .filter(|set| set.remote().is_some())
    .collect();
  if !sets.is_empty() {
    for (name, kind) in [
      ("prefixes", "gauge"),
      ("stale", "gauge"),
      ("age_seconds", "gauge"),
      ("refresh_failures_total", "counter"),
    ] {
      let _ = writeln!(text, "# TYPE rilgate_ip_set_{name} {kind}");
      for set in &sets {
        let status = set.status();
        let value = match name {
          "prefixes" => status.prefixes as u64,
          "stale" => u64::from(status.stale),
          "age_seconds" => status.age.map_or(0, |age| age.as_secs()),
          _ => status.refresh_failures,
        };
        // The age gauge is meaningful only while a snapshot is loaded.
        if name == "age_seconds" && !status.loaded {
          continue;
        }
        let _ = writeln!(
          text,
          "rilgate_ip_set_{name}{{set=\"{}\"}} {value}",
          set.name()
        );
      }
    }
  }
  let databases: Vec<_> = state.pow.geoip().iter().collect();
  if !databases.is_empty() {
    for (name, kind) in [
      ("loaded", "gauge"),
      ("stale", "gauge"),
      ("refresh_failures_total", "counter"),
    ] {
      let _ = writeln!(text, "# TYPE rilgate_geoip_database_{name} {kind}");
      for database in &databases {
        let status = database.status();
        let value = match name {
          "loaded" => u64::from(status.loaded),
          "stale" => u64::from(status.stale),
          _ => status.refresh_failures,
        };
        let _ = writeln!(
          text,
          "rilgate_geoip_database_{name}{{database=\"{}\"}} {value}",
          database.kind().label()
        );
      }
    }
  }
  if let Some(stats) = state.pow.hash_diagnostics() {
    render_hash_metrics(&mut text, stats);
  }
  if let Some(stats) = state.pow.verifier_diagnostics() {
    for (name, value) in [
      ("queue_depth", stats.queued as u64),
      ("active", u64::from(stats.active)),
      ("completed_total", stats.completed),
      ("rejected_total", stats.rejected),
    ] {
      let kind = if name.ends_with("_total") {
        "counter"
      } else {
        "gauge"
      };
      let _ = writeln!(
        text,
        "# TYPE rilgate_verifier_{name} {kind}\nrilgate_verifier_{name} {value}"
      );
    }
  }
  let mut out = response(StatusCode::OK, text);
  out.headers_mut().insert(
    CONTENT_TYPE,
    "text/plain; version=0.0.4; charset=utf-8".parse().unwrap(),
  );
  out
}

fn render_hash_metrics(text: &mut String, stats: rilgate_pow::algorithm::HashDiagnostics) {
  text.push_str("# TYPE rilgate_randomx_hash_total counter\n");
  for (hasher, name) in ["local", "remote"].iter().enumerate() {
    for (outcome, label) in ["hashed", "unavailable", "invalid"].iter().enumerate() {
      let _ = writeln!(
        text,
        "rilgate_randomx_hash_total{{hasher=\"{name}\",outcome=\"{label}\"}} {}",
        stats.outcomes[hasher][outcome]
      );
    }
  }
  text.push_str("# TYPE rilgate_randomx_hash_fallback_total counter\n");
  for (hasher, name) in ["local", "remote"].iter().enumerate() {
    for (reason, label) in [
      "timeout",
      "connection",
      "overloaded",
      "protocol",
      "deadline",
    ]
    .iter()
    .enumerate()
    {
      let _ = writeln!(
        text,
        "rilgate_randomx_hash_fallback_total{{from=\"{name}\",reason=\"{label}\"}} {}",
        stats.fallbacks[hasher][reason]
      );
    }
  }
  text.push_str("# TYPE rilgate_randomx_remote_duration_seconds histogram\n");
  let mut count = 0;
  for (index, value) in stats.remote_duration.iter().enumerate() {
    count += value;
    let bound = rilgate_pow::algorithm::HashDiagnostics::DURATION_BOUNDS
      .get(index)
      .map_or_else(
        || "+Inf".into(),
        |micros| (*micros as f64 / 1_000_000.0).to_string(),
      );
    let _ = writeln!(
      text,
      "rilgate_randomx_remote_duration_seconds_bucket{{le=\"{bound}\"}} {count}"
    );
  }
  let _ = writeln!(
    text,
    "rilgate_randomx_remote_duration_seconds_count {count}\nrilgate_randomx_remote_duration_seconds_sum {}",
    stats.remote_micros as f64 / 1_000_000.0
  );
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn randomx_metrics_render_bounded_labels_and_cumulative_buckets() {
    let mut text = String::new();
    render_hash_metrics(
      &mut text,
      rilgate_pow::algorithm::HashDiagnostics {
        outcomes: [[1, 2, 3], [4, 5, 6]],
        fallbacks: [[1; 5]; 2],
        remote_duration: [1; 8],
        remote_micros: 2_000_000,
      },
    );
    let mut family = "";
    let mut closed = std::collections::HashSet::new();
    for line in text.lines() {
      let name = if let Some(rest) = line.strip_prefix("# TYPE ") {
        rest.split_whitespace().next().unwrap()
      } else {
        line.split(['{', ' ']).next().unwrap()
      };
      let name = name
        .strip_suffix("_bucket")
        .or_else(|| name.strip_suffix("_count"))
        .or_else(|| name.strip_suffix("_sum"))
        .unwrap_or(name);
      if name != family {
        closed.insert(family);
        assert!(
          !closed.contains(name),
          "metric family {name} is interleaved"
        );
        family = name;
      }
    }
    assert!(
      text.contains("rilgate_randomx_hash_total{hasher=\"remote\",outcome=\"unavailable\"} 5")
    );
    assert!(
      text.contains("rilgate_randomx_hash_fallback_total{from=\"local\",reason=\"overloaded\"} 1")
    );
    assert!(text.contains("rilgate_randomx_remote_duration_seconds_bucket{le=\"+Inf\"} 8"));
    assert!(text.contains("rilgate_randomx_remote_duration_seconds_sum 2"));
  }
}
