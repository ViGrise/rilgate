use super::Args;
use rilgate_config::{Config, Configuration};

pub fn load(mut args: Args) -> anyhow::Result<Config> {
  Configuration::load(args.overrides())
}
