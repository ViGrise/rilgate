use rilgate_config::{Setting, SettingValue, loader::parse};

const BASE: &str = "server:\n  upstream:\n    url: http://127.0.0.1:3000\nsecurity:\n  secret: '0123456789abcdef0123456789abcdef'\n";

#[test]
fn unusable_fallbacks_fail_configuration_assembly() {
  for (algorithm, difficulty, verification, message) in [
    (
      "randomx",
      4,
      "local_verify",
      "pow.algorithms.randomx.fallback.algorithm: expected fast or hashwx",
    ),
    (
      "refresh",
      4,
      "local_verify",
      "pow.algorithms.randomx.fallback.algorithm: expected fast or hashwx",
    ),
    (
      "hashwx",
      4,
      "local_verify",
      "pow.algorithms.randomx.fallback.algorithm: hashwx is not enabled",
    ),
    (
      "fast",
      0,
      "local_verify",
      "pow.algorithms.randomx.fallback.difficulty: fast difficulty must be between 1 and 64 leading hexadecimal zeroes",
    ),
    (
      "fast",
      4,
      "stratum_pool_accepted",
      "pow.algorithms.randomx.fallback: requires pow.verification: local_verify",
    ),
  ] {
    let yaml = format!(
      "{BASE}pow:\n  verification: {verification}\n  algorithms:\n    randomx:\n      fallback:\n        algorithm: {algorithm}\n        difficulty: {difficulty}\n  stratum_pool:\n    pools:\n      - name: default\n        address: 127.0.0.1:3333\n"
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
