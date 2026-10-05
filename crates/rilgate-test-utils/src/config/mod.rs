use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};

use rilgate_config::{
  BotRuleConfig, BrowserConfig, ChallengePolicy, Config, Configuration, DifficultyConfig,
  Environment, P2PoolConfig, PolicyAction, PowAlgorithm, PowConfig, PowEnforcement, RiskConfig,
  Secret, UpstreamConfig, WebConfig,
};

pub fn config() -> Config {
  Arc::new(Configuration {
    env: Environment::Development,
    listen_address: SocketAddr::from(([127, 0, 0, 1], 0)),
    app_port: 0,
    upstream: UpstreamConfig {
      url: "http://127.0.0.1:3000".into(),
      preserve_host: false,
      max_body_bytes: 1024,
      connect_timeout_seconds: 1,
      read_timeout_seconds: 2,
    },
    web: WebConfig {
      name: "Rilgate".into(),
      website_url: "https://rilgate.com".into(),
      base_path: "/.well-known/test-pow".into(),
      ..Default::default()
    },
    pow: PowConfig {
      enforce: PowEnforcement::LocalVerify,
      default_algorithm: PowAlgorithm::Fast,
      signing_key: Secret("test-shared-signing-key".into()),
      auth_signing_key: Secret("test-auth-signing-key".into()),
      challenge_ttl_seconds: 60,
      randomx_challenge_ttl_seconds: 3600,
      receipt_ttl_seconds: 30,
      auth_ttl_seconds: 300,
      auth_cookie_name: "rilgate_auth".into(),
      nonce_offset: 39,
      verifier_queue_capacity: 8,
      rate_limit: Default::default(),
      p2pool: P2PoolConfig {
        host: "127.0.0.1".into(),
        port: 3333,
        password: Secret("x".into()),
        agent: "test".into(),
        connect_timeout_seconds: 1,
        submit_timeout_seconds: 1,
        max_sessions: 4,
        session_ttl_seconds: 10,
      },
      difficulty: DifficultyConfig {
        base: 1000,
        weight_step: 10,
        max: 32_000,
      },
      risk: RiskConfig {
        default_weight: 0,
        allow_debug_header: false,
        debug_header: "x-pow-weight".into(),
      },
      bots: vec![BotRuleConfig {
        name: "protected".into(),
        path_regex: Some("^/protected".into()),
        user_agent_regex: None,
        headers_regex: BTreeMap::new(),
        action: PolicyAction::Challenge,
        weight: Default::default(),
        challenge: ChallengePolicy {
          algorithm: Some(PowAlgorithm::Fast),
          difficulty: Some(1),
        },
      }],
      thresholds: vec![],
      browser: BrowserConfig {
        solver_module_path: "/static/js/solvers/randomx/solver.js".into(),
      },
    },
  })
}
