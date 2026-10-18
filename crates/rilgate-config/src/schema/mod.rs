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
  pub environment: Environment,
  pub server: server::Server,
  pub site: site::Site,
  pub security: security::Security,
  pub pow: pow::Pow,
  pub policy: rilgate_policy::policy::schema::Definition,
}
