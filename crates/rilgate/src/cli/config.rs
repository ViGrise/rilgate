use super::Args;
use rilgate_config::{Config, Configuration};

pub fn load(args: Args) -> Config {
  Configuration::load(args.into())
}
