use rilgate_config::{Setting, SettingValue, loader::parse};

const BASE: &str = "server:\n  upstream:\n    url: http://127.0.0.1:3000\nsecurity:\n  secret: '0123456789abcdef0123456789abcdef'\n";

#[test]
fn unusable_fallbacks_fail_configuration_assembly() {
  for (algorithm, difficulty, verification, message) in [
    (
      "randomx",
      4,
      "gateway",
      "pow.algorithms.randomx.fallback.algorithm: expected fast or hashwx",
    ),
    (
      "refresh",
      4,
      "gateway",
      "pow.algorithms.randomx.fallback.algorithm: expected fast or hashwx",
    ),
    (
      "hashwx",
      4,
      "gateway",
      "pow.algorithms.randomx.fallback.algorithm: hashwx is not enabled",
    ),
    (
      "fast",
      0,
      "gateway",
      "pow.algorithms.randomx.fallback.difficulty: fast difficulty must be between 1 and 64 leading hexadecimal zeroes",
    ),
  ] {
    let yaml = format!(
      "{BASE}pow:\n  algorithms:\n    randomx:\n      verify: {{mode: {verification}}}\n      fallback:\n        algorithm: {algorithm}\n        difficulty: {difficulty}\n  stratum_pool:\n    pools:\n      - name: default\n        address: 127.0.0.1:3333\n"
    );
    let error = parse(&yaml).unwrap().resolve().unwrap_err();
    assert_eq!(error.to_string(), message);
  }
}

#[test]
fn a_lone_fallback_flag_requires_the_other_field() {
  for (field, value, missing) in [
    ("algorithm", "fast", "difficulty"),
    ("difficulty", "4", "algorithm"),
  ] {
    let error = rilgate_config::loader::parse_with(
      BASE,
      &[Setting {
        path: format!("pow.algorithms.randomx.fallback.{field}"),
        source: format!("--pow-algorithms-randomx-fallback-{field}"),
        value: SettingValue::Scalar(value.into()),
      }],
    )
    .unwrap_err();
    assert!(
      error
        .to_string()
        .contains(&format!("missing field `{missing}`")),
      "{error}"
    );
  }
}

#[test]
fn pool_share_accepts_fast_fallback() {
  let yaml = format!(
    "{BASE}pow:\n  stratum_pool: {{}}\n  algorithms:\n    randomx:\n      verify: {{mode: pool_share}}\n      fallback: {{algorithm: fast, difficulty: 4}}\n"
  );
  parse(&yaml).unwrap().resolve().unwrap();
}
