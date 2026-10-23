use std::{net::SocketAddr, sync::Arc};

use rilgate_config::schema::policy::{Action, Condition, Definition, Mode, Rule, StringMatch};
use rilgate_config::{
  BrowserConfig, ChallengePolicy, Config, Configuration, DifficultyConfig, Environment,
  P2PoolConfig, PowAlgorithm, PowConfig, PowEnforcement, Secret, UpstreamConfig, WebConfig,
};

pub fn config() -> Config {
  Arc::new(Configuration {
    env: Environment::Development,
    operations_listen_addresses: vec![],
    listen_addresses: vec![SocketAddr::from(([127, 0, 0, 1], 0))],
    app_port: 0,
    server: Default::default(),
    upstream: UpstreamConfig {
      url: "http://127.0.0.1:3000".into(),
      preserve_host: false,
      max_body_bytes: 1024,
      connect_timeout: std::time::Duration::from_secs(1),
      read_timeout: std::time::Duration::from_secs(2),
    },
    web: WebConfig {
      name: "Rilgate".into(),
      website_url: "https://rilgate.com".into(),
      base_path: "/.well-known/test-pow".into(),
      ..Default::default()
    },
    pow: PowConfig {
      enforce: PowEnforcement::LocalVerify,
      randomx_enabled: true,
      default_algorithm: PowAlgorithm::Fast,
      signing_key: Secret("test-shared-signing-key-with-32-bytes".into()),
      auth_signing_key: Secret("test-auth-signing-key-with-32-bytes".into()),
      signing_keys: Default::default(),
      auth_signing_keys: Default::default(),
      challenge_ttl_seconds: 60,
      randomx_challenge_ttl_seconds: 3600,
      receipt_ttl_seconds: 30,
      auth_ttl_seconds: 300,
      auth_binding: Default::default(),
      auth_cookie_name: "rilgate_auth".into(),
      nonce_offset: 39,
      verifier_queue_capacity: 8,
      randomx_cache_idle_timeout: std::time::Duration::from_secs(1800),
      rate_limit: Default::default(),
      p2pool: P2PoolConfig {
        host: "127.0.0.1".into(),
        port: 3333,
        password: Secret("x".into()),
        agent: "test".into(),
        connect_timeout: std::time::Duration::from_secs(1),
        submit_timeout: std::time::Duration::from_secs(1),
        max_sessions: 4,
        session_ttl: std::time::Duration::from_secs(10),
        max_job_sessions: 2,
        job_session_mode: rilgate_pool::JobSessionMode::Pooled,
        job_session_idle: std::time::Duration::from_secs(5),
      },
      difficulty: DifficultyConfig {
        base: 1000,
        weight_step: 10,
        max: 32_000,
      },
      policy: Definition {
        mode: Mode::Custom,
        default_action: Some(Action::Allow),
        rules: Some(vec![Rule {
          name: "protected".into(),
          when: Condition {
            path: Some(StringMatch {
              prefix: Some("/protected".into()),
              ..Default::default()
            }),
            ..Default::default()
          },
          action: Action::Challenge,
          score: None,
          challenge: Some(ChallengePolicy {
            algorithm: Some(PowAlgorithm::Fast),
            difficulty: Some(1),
          }),
        }]),
        score: None,
        ip_sets: std::collections::BTreeMap::new(),
      },
      browser: BrowserConfig {
        solver_module_path: "/static/js/solvers/randomx/solver.js".into(),
      },
    },
  })
}
