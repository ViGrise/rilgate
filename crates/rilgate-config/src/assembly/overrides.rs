use crate::{Environment, PowEnforcement};
use std::path::PathBuf;
#[derive(Debug, Default)]
pub struct Overrides {
  pub config: Option<PathBuf>,
  pub env: Option<Environment>,
  pub port: Option<u16>,
  pub pow_enforce: Option<PowEnforcement>,
}
