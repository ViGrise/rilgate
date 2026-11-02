use rilgate_config::loader::parse;

const BASE: &str = "server:\n  upstream:\n    url: http://127.0.0.1:3000\nsecurity:\n  secret: '0123456789abcdef0123456789abcdef'\n";

fn resolve(pow: &str, delay: u64) -> anyhow::Result<rilgate_config::Config> {
  let yaml = format!(
    "{BASE}pow:\n{pow}policy:\n  default_action: allow\n  rules:\n    - name: slow\n      when: {{path: {{prefix: /slow}}}}\n      action: challenge\n      challenge: {{algorithm: refresh, difficulty: {delay}}}\n"
  );
  parse(&yaml)?.resolve()
}

#[test]
fn refresh_max_bounds_rule_delays_within_the_challenge_ttl() {
  let error = resolve("  algorithms: {}\n", 30).unwrap_err().to_string();
  assert!(error.contains("refresh requires 1..10 seconds"), "{error}");

  resolve("  algorithms:\n    refresh: {max: 30}\n", 30).unwrap();
  resolve(
    "  challenge: {ttl: 2m}\n  algorithms:\n    refresh: {max: 110}\n",
    110,
  )
  .unwrap();

  let error = resolve("  algorithms:\n    refresh: {max: 51}\n", 5)
    .unwrap_err()
    .to_string();
  assert_eq!(
    error,
    "pow.algorithms.refresh.max: expected 1 to 50 seconds, 10 seconds less than pow.challenge.ttl (60s)"
  );
}
