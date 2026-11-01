pub use rilgate_policy::policy::schema as policy;
pub mod pow;
pub mod security;
pub mod server;
pub mod site;
pub mod value;

use crate::Environment;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Document {
  #[serde(skip)]
  pub(crate) sources: Vec<(String, String)>,
  #[serde(skip)]
  pub(crate) config_sources: crate::loader::sources::ConfigurationSources,
  pub environment: Environment,
  pub server: server::Server,
  pub site: site::Site,
  pub security: security::Security,
  pub pow: pow::Pow,
  pub signals: rilgate_policy::policy::schema::Signals,
  pub policy: rilgate_policy::policy::schema::Definition,
}
