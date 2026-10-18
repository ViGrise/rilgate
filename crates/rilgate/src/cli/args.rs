use clap::{Parser, Subcommand};
use std::path::PathBuf;

use rilgate_config::{Environment, PowEnforcement};

#[derive(Parser, Debug, Default)]
#[command(version, about = "Rilgate proof-of-work gateway", long_about = None)]
pub struct Args {
  /// Path to the YAML configuration file.
  #[arg(short, long, global = true, value_name = "FILE")]
  pub config: Option<PathBuf>,

  #[command(subcommand)]
  pub command: Option<Command>,

  /// Environment to run in: development or production.
  #[arg(long)]
  pub env: Option<Environment>,

  /// Port to listen on.
  #[arg(short, long)]
  pub port: Option<u16>,

  /// Proof verification backend.
  #[arg(long = "verification")]
  pub pow_enforce: Option<PowEnforcement>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
  /// Validate or display the resolved YAML configuration.
  Config {
    #[command(subcommand)]
    command: ConfigCommand,
  },
  /// Initialize, validate, or compare operator web resources.
  Web {
    #[command(subcommand)]
    command: WebCommand,
  },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
  Check,
  Show {
    #[arg(long)]
    defaults: bool,
  },
}

#[derive(Debug, Subcommand)]
pub enum WebCommand {
  Init { directory: PathBuf },
  Validate { directory: PathBuf },
  Diff { directory: PathBuf },
}

impl From<Args> for rilgate_config::Overrides {
  fn from(args: Args) -> Self {
    Self {
      config: args.config,
      env: args.env,
      port: args.port,
      pow_enforce: args.pow_enforce,
    }
  }
}
