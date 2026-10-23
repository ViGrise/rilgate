use super::value::{IdleTimeout, Time};
use rilgate_crypto::Secret;
use rilgate_protocol::{PowAlgorithm, PowEnforcement};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pow {
  pub verification: PowEnforcement,
  pub default_algorithm: PowAlgorithm,
  pub challenge: Challenge,
  pub rate_limit: RateLimit,
  pub algorithms: Algorithms,
  pub stratum_pool: Option<StratumPool>,
}
impl Default for Pow {
  fn default() -> Self {
    Self {
      verification: PowEnforcement::LocalVerify,
      default_algorithm: PowAlgorithm::Fast,
      challenge: Challenge::default(),
      rate_limit: RateLimit::default(),
      algorithms: Algorithms::default(),
      stratum_pool: None,
    }
  }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Challenge {
  pub ttl: Time,
  pub receipt_ttl: Time,
}
impl Default for Challenge {
  fn default() -> Self {
    Self {
      ttl: Time::seconds(60),
      receipt_ttl: Time::seconds(30),
    }
  }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RateLimit {
  pub enabled: bool,
  pub max_clients: usize,
  pub client: ClientQuota,
  pub global: GlobalQuota,
}
impl Default for RateLimit {
  fn default() -> Self {
    Self {
      enabled: true,
      max_clients: 4096,
      client: ClientQuota {
        burst: 10,
        refill_per_second: 0.1,
      },
      global: GlobalQuota {
        burst: 64,
        refill_per_second: 16.0,
      },
    }
  }
}
macro_rules! quota {
  ($name:ident, $burst:expr, $refill:expr) => {
    #[derive(Debug, Clone, Deserialize, Serialize)]
    #[serde(default, deny_unknown_fields)]
    pub struct $name {
      pub burst: u32,
      pub refill_per_second: f64,
    }
    impl Default for $name {
      fn default() -> Self {
        Self {
          burst: $burst,
          refill_per_second: $refill,
        }
      }
    }
  };
}
quota!(ClientQuota, 10, 0.1);
quota!(GlobalQuota, 64, 16.0);
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Algorithms {
  pub hashwx: HashWX,
  pub randomx: RandomX,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RandomX {
  pub challenge_ttl: Time,
  pub nonce_offset: usize,
  pub verifier_queue: usize,
  pub cache: Cache,
  pub verify: Verify,
  pub serve: Option<Serve>,
  pub difficulty: Difficulty,
  pub browser: Browser,
}
impl Default for RandomX {
  fn default() -> Self {
    Self {
      challenge_ttl: Time::seconds(3600),
      nonce_offset: 39,
      verifier_queue: 8,
      cache: Cache::default(),
      verify: Verify::default(),
      serve: None,
      difficulty: Difficulty::default(),
      browser: Browser::default(),
    }
  }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Cache {
  pub idle_timeout: IdleTimeout,
}
impl Default for Cache {
  fn default() -> Self {
    Self {
      idle_timeout: IdleTimeout(std::time::Duration::from_secs(1800)),
    }
  }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Difficulty {
  pub base: u64,
  pub score_step: i32,
  pub max: u64,
}
impl Default for Difficulty {
  fn default() -> Self {
    Self {
      base: 1000,
      score_step: 10,
      max: 32_000,
    }
  }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Browser {
  pub module: String,
}
impl Default for Browser {
  fn default() -> Self {
    Self {
      module: "/static/js/solvers/randomx/solver.js".into(),
    }
  }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct StratumPool {
  pub pools: Vec<Pool>,
  pub proxies: Vec<rilgate_pool::Proxy>,
  pub failover: Failover,
  pub agent: String,
  pub timeout: PoolTimeout,
  pub sessions: Sessions,
  pub job_sessions: JobSessions,
}
impl Default for StratumPool {
  fn default() -> Self {
    Self {
      proxies: Vec::new(),
      pools: vec![Pool {
        name: "default".into(),
        address: "127.0.0.1:3333".into(),
        login: Login::default(),
        tls: Default::default(),
        proxies: None,
      }],
      failover: Failover::default(),
      agent: concat!("rilgate/", env!("CARGO_PKG_VERSION")).into(),
      timeout: PoolTimeout::default(),
      sessions: Sessions::default(),
      job_sessions: JobSessions::default(),
    }
  }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pool {
  pub name: String,
  pub address: String,
  #[serde(default)]
  pub login: Login,
  #[serde(default)]
  pub tls: rilgate_pool::TlsConfig,
  #[serde(default)]
  pub proxies: Option<Vec<rilgate_pool::Proxy>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Login {
  pub user: Secret,
  pub password: Secret,
}
impl Default for Login {
  fn default() -> Self {
    Self {
      user: Secret("x+{difficulty}".into()),
      password: Secret("x".into()),
    }
  }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Failover {
  pub timeout: Time,
  pub cooldown: Time,
}
impl Default for Failover {
  fn default() -> Self {
    Self {
      timeout: Time::seconds(15),
      cooldown: Time::seconds(30),
    }
  }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PoolTimeout {
  pub connect: Time,
  pub submit: Time,
}
impl Default for PoolTimeout {
  fn default() -> Self {
    Self {
      connect: Time::seconds(5),
      submit: Time::seconds(20),
    }
  }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sessions {
  pub max: usize,
  pub ttl: Time,
}
impl Default for Sessions {
  fn default() -> Self {
    Self {
      max: 64,
      ttl: Time::seconds(900),
    }
  }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct JobSessions {
  pub mode: rilgate_pool::JobSessionMode,
  pub max: usize,
  pub idle: Time,
}
impl Default for JobSessions {
  fn default() -> Self {
    Self {
      mode: rilgate_pool::JobSessionMode::Pooled,
      max: 16,
      idle: Time::seconds(60),
    }
  }
}

pub use rilgate_pow_randomx::{DeadlinePolicy, HasherKind as Hasher};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Verify {
  pub order: Vec<Hasher>,
  pub remote: Remote,
}
impl Default for Verify {
  fn default() -> Self {
    Self {
      order: vec![Hasher::Local],
      remote: Remote::default(),
    }
  }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Remote {
  pub on_deadline_rejected: DeadlinePolicy,
  pub endpoint: Option<String>,
  pub server_name: Option<String>,
  pub timeout: Time,
  pub tls: RemoteTls,
}
impl Default for Remote {
  fn default() -> Self {
    Self {
      on_deadline_rejected: DeadlinePolicy::Fallback,
      endpoint: None,
      server_name: None,
      timeout: Time::seconds(2),
      tls: RemoteTls::default(),
    }
  }
}

impl Remote {
  /// The certificate name to verify: `server_name`, or else the endpoint host.
  pub fn effective_server_name(&self) -> Option<String> {
    self.server_name.clone().or_else(|| {
      let endpoint = url::Url::parse(self.endpoint.as_deref()?).ok()?;
      Some(endpoint.host_str()?.trim_matches(['[', ']']).to_owned())
    })
  }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RemoteTls {
  pub ca: Option<std::path::PathBuf>,
  pub client_cert: Option<std::path::PathBuf>,
  pub client_key: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Serve {
  pub listen: String,
  pub workers: usize,
  pub queue: usize,
  pub max_connections: usize,
  pub tls: ServeTls,
}
impl Default for Serve {
  fn default() -> Self {
    Self {
      listen: String::new(),
      workers: 1,
      queue: 64,
      max_connections: 256,
      tls: ServeTls::default(),
    }
  }
}
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServeTls {
  pub cert: Option<std::path::PathBuf>,
  pub key: Option<std::path::PathBuf>,
  pub client_ca: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HashWX {
  pub nonces_per_seed: u32,
  pub difficulty: HashWxDifficulty,
}
impl Default for HashWX {
  fn default() -> Self {
    Self {
      nonces_per_seed: 65536,
      difficulty: HashWxDifficulty::default(),
    }
  }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HashWxDifficulty {
  pub base: u64,
  pub score_step: i32,
  pub max: u64,
}
impl Default for HashWxDifficulty {
  fn default() -> Self {
    Self {
      base: 50000,
      score_step: 10,
      max: 5000000,
    }
  }
}
