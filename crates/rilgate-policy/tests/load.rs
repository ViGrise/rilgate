use http::{HeaderMap, Method};
use rilgate_policy::{
  HostLoad, PolicyAction, PolicyEngine, conditions::Request, policy::schema::Definition,
};

fn engine(text: &str, load: &'static HostLoad) -> anyhow::Result<PolicyEngine> {
  let definition: Definition = serde_yaml_ng::from_str(text)?;
  Ok(PolicyEngine::from_definition(&definition)?.with_host_load(load))
}
fn fresh_load() -> &'static HostLoad {
  Box::leak(Box::new(HostLoad::new()))
}
fn decide(engine: &PolicyEngine) -> (PolicyAction, i32, bool) {
  let headers = HeaderMap::new();
  let decision = engine.evaluate_context(
    &Request {
      host: "example.test",
      headers: &headers,
      path: "/",
      method: &Method::GET,
      client_ip: None,
    },
    0,
  );
  (decision.action, decision.weight, decision.unresolved)
}
fn rule_with(load: &str) -> String {
  format!(
    "mode: custom\ndefault_action: allow\nrules:\n  - name: busy\n    when:\n      load: {load}\n    action: score\n    score: 5\n"
  )
}

#[test]
fn load_condition_matches_the_selected_window() {
  let load = fresh_load();
  load.store([9.0, 5.0, 1.0]);
  let five = engine(&rule_with("{window: 5m, gte: 4.0, lt: 8.0}"), load).unwrap();
  assert_eq!(decide(&five).1, 5);
  let one = engine(&rule_with("{window: 1m, gte: 4.0, lt: 8.0}"), load).unwrap();
  assert_eq!(decide(&one).1, 0);
  let default_window = engine(&rule_with("{gte: 4.0, lt: 8.0}"), load).unwrap();
  assert_eq!(decide(&default_window).1, 5);
}

#[test]
fn load_condition_is_unknown_before_a_sample() {
  let engine = engine(
    "mode: custom\ndefault_action: deny\nrules:\n  - name: calm\n    when:\n      load: {lt: 100.0}\n    action: allow\n",
    fresh_load(),
  )
  .unwrap();
  assert!(engine.uses_load());
  let (action, _, unresolved) = decide(&engine);
  assert_ne!(action, PolicyAction::Allow);
  assert!(unresolved);
}

#[test]
fn load_condition_rejects_invalid_comparisons() {
  for (load, message) in [
    (
      "{gte: 8.0, lt: 4.0}",
      "load: comparisons describe an empty range",
    ),
    (
      "{gt: 4.0, lte: 4.0}",
      "load: comparisons describe an empty range",
    ),
    (
      "{gte: .nan}",
      "load: expected a finite number of at least 0",
    ),
    ("{lt: -1.0}", "load: expected a finite number of at least 0"),
    ("{window: 5m}", "load: expected a numeric comparison"),
  ] {
    let error = engine(&rule_with(load), fresh_load()).unwrap_err();
    assert!(format!("{error:#}").contains(message), "{load}: {error:#}");
  }
}

#[test]
fn load_condition_accepts_a_single_point_range() {
  let load = fresh_load();
  load.store([4.0, 4.0, 4.0]);
  let engine = engine(&rule_with("{gte: 4.0, lte: 4.0}"), load).unwrap();
  assert_eq!(decide(&engine).1, 5);
}
