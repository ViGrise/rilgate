use clap::Parser;
use rilgate::cli::{Args, Command, ConfigCommand};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  dotenvy::dotenv().ok();
  let mut args = Args::parse();
  let command = args.command.take();
  let document = rilgate_config::loader::load(&args.into())?;
  let config = document.resolve()?;
  match command {
    Some(Command::Config { command }) => {
      rilgate::app::validate(&config)?;
      match command {
        ConfigCommand::Check => println!("Configuration is valid"),
        ConfigCommand::Show { defaults } => print!("{}", document.display(defaults)?),
      }
      Ok(())
    }
    Some(Command::Web { command }) => rilgate::cli::web::run(&config.web, command),
    None => {
      rilgate_telemetry::setup_tracing();
      rilgate::cli::serve::run(config).await
    }
  }
}
