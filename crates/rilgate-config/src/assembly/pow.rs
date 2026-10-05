use super::defaults::default_thresholds;
use crate::*;
use crate::{Overrides, loader::file::FileConfig, resolution::resolve};
pub(super) fn load(file: &FileConfig, args: &Overrides) -> PowConfig {
  let enforce = resolve(
    file,
    "pow.enforce",
    &[],
    args.pow_enforce,
    PowEnforcement::LocalVerify,
  );

  PowConfig {
    enforce,
    default_algorithm: resolve(
      file,
      "pow.default_algorithm",
      &[],
      None,
      PowAlgorithm::RandomX,
    ),
    signing_key: Secret(resolve(
      file,
      "pow.signing_key",
      &[],
      None,
      "change-me-pow-signing-key".to_string(),
    )),
    auth_signing_key: Secret(resolve(
      file,
      "pow.auth_signing_key",
      &[],
      None,
      "change-me-auth-signing-key".to_string(),
    )),
    challenge_ttl_seconds: resolve(file, "pow.challenge_ttl_seconds", &[], None, 60_u64),
    randomx_challenge_ttl_seconds: resolve(
      file,
      "pow.randomx_challenge_ttl_seconds",
      &[],
      None,
      3600_u64,
    ),
    receipt_ttl_seconds: resolve(file, "pow.receipt_ttl_seconds", &[], None, 30_u64),
    auth_ttl_seconds: resolve(file, "pow.auth_ttl_seconds", &[], None, 21_600_u64),
    auth_cookie_name: resolve(
      file,
      "pow.auth_cookie_name",
      &[],
      None,
      "rilgate_auth".to_string(),
    ),
    nonce_offset: resolve(file, "pow.nonce_offset", &[], None, 39_usize),
    verifier_queue_capacity: resolve(file, "pow.verifier_queue_capacity", &[], None, 8_usize),
    rate_limit: RateLimitConfig {
      enabled: resolve(file, "pow.rate_limit.enabled", &[], None, true),
      client_burst: resolve(file, "pow.rate_limit.client_burst", &[], None, 10_u32),
      client_refill_per_second: resolve(
        file,
        "pow.rate_limit.client_refill_per_second",
        &[],
        None,
        0.1_f64,
      ),
      global_burst: resolve(file, "pow.rate_limit.global_burst", &[], None, 64_u32),
      global_refill_per_second: resolve(
        file,
        "pow.rate_limit.global_refill_per_second",
        &[],
        None,
        16.0_f64,
      ),
      max_clients: resolve(file, "pow.rate_limit.max_clients", &[], None, 4096_usize),
    },
    p2pool: P2PoolConfig {
      host: resolve(file, "pow.p2pool.host", &[], None, "127.0.0.1".to_string()),
      port: resolve(file, "pow.p2pool.port", &[], None, 3333_u16),
      password: Secret(resolve(
        file,
        "pow.p2pool.password",
        &[],
        None,
        "x".to_string(),
      )),
      agent: resolve(
        file,
        "pow.p2pool.agent",
        &[],
        None,
        "rilgate/0.1.0".to_string(),
      ),
      connect_timeout_seconds: resolve(
        file,
        "pow.p2pool.connect_timeout_seconds",
        &[],
        None,
        5_u64,
      ),
      submit_timeout_seconds: resolve(file, "pow.p2pool.submit_timeout_seconds", &[], None, 20_u64),
      max_sessions: resolve(file, "pow.p2pool.max_sessions", &[], None, 64_usize),
      session_ttl_seconds: resolve(file, "pow.p2pool.session_ttl_seconds", &[], None, 900_u64),
    },
    difficulty: DifficultyConfig {
      base: resolve(file, "pow.difficulty.base", &[], None, 1000_u64),
      weight_step: resolve(file, "pow.difficulty.weight_step", &[], None, 10_i32),
      max: resolve(file, "pow.difficulty.max", &[], None, 32_000_u64),
    },
    risk: RiskConfig {
      default_weight: resolve(file, "pow.risk.default_weight", &[], None, 0_i32),
      allow_debug_header: resolve(file, "pow.risk.allow_debug_header", &[], None, false),
      debug_header: resolve(
        file,
        "pow.risk.debug_header",
        &[],
        None,
        "x-pow-weight".to_string(),
      ),
    },
    bots: file.section("pow.bots").unwrap_or_default(),
    thresholds: file
      .section("pow.thresholds")
      .unwrap_or_else(default_thresholds),
    browser: BrowserConfig {
      solver_module_path: resolve(
        file,
        "pow.browser.solver_module_path",
        &[],
        None,
        "/static/js/solvers/randomx/solver.js".to_string(),
      ),
    },
  }
}
