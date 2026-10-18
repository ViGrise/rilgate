use crate::{
  runtime::state::AppState,
  transport::response::{Response, response},
};
use http::{StatusCode, header::CONTENT_TYPE};
use std::fmt::Write;
use std::sync::atomic::Ordering;

pub fn exposition(state: &AppState) -> Response {
  let mut text = state.metrics.render();
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
