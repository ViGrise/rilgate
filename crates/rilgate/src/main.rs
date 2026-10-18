use clap::Parser;
use rilgate::cli::{Args, Command};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  dotenvy::dotenv().ok();
  rilgate_telemetry::setup_tracing();
  let mut args = Args::parse();
  let command = args.command.take();
  let config = rilgate::cli::config::load(args);
  match command {
    Some(Command::CheckConfig) => rilgate::app::validate(&config),
    Some(Command::Web { command }) => rilgate::cli::web::run(&config.web, command),
    None => rilgate::cli::serve::run(config).await,
  }
}
