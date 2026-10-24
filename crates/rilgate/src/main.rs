use clap::Parser;
use rilgate::cli::{Args, Command, ConfigCommand, LogFormat, VerifierCommand};

// Per-thread heaps keep concurrent request handling from contending on one allocator lock.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  // Load the environment file before parsing, because its variables can supply flag values.
  match rilgate::cli::env_file(&std::env::args_os().collect::<Vec<_>>()) {
    Some(path) => {
      dotenvy::from_path(&path)
        .map_err(|error| anyhow::anyhow!("loading environment file {}: {error}", path.display()))?;
    }
    None => {
      dotenvy::dotenv().ok();
    }
  }
  let mut args = Args::parse();
  let command = args.command.take();
  if let Some(Command::Upgrade { version, force }) = command {
    return rilgate::cli::upgrade::run(version, force).await;
  }
  let rilgate_config::loader::Loaded {
    document,
    mut warnings,
  } = rilgate_config::loader::load(&args.overrides())?;
  if let Some(Command::Verifier { command }) = command {
    let config = document.resolve_verifier()?;
    warnings.extend(config.warnings().iter().map(ToString::to_string));
    rilgate::verifier::validate(&config)?;
    return match command {
      VerifierCommand::Serve => {
        rilgate_telemetry::setup_tracing(
          args.log_level.as_deref(),
          args.log_format == LogFormat::Json,
        );
        for warning in warnings {
          tracing::warn!("{warning}");
        }
        rilgate::verifier::run(config).await
      }
      VerifierCommand::Config { command } => {
        for warning in &warnings {
          eprintln!("warning: {warning}");
        }
        match command {
          ConfigCommand::Check { strict } => {
            anyhow::ensure!(
              !strict || warnings.is_empty(),
              "configuration has {} warning(s)",
              warnings.len()
            );
            println!("Configuration is valid");
          }
          ConfigCommand::Show { defaults } => print!("{}", document.display_verifier(defaults)?),
        }
        Ok(())
      }
    };
  }
  let config = document.resolve()?;
  warnings.extend(config.warnings().iter().map(ToString::to_string));
  // Standard output carries command results and serve logs, so command
  // warnings go to standard error and serve warnings go through tracing.
  if !matches!(command, None | Some(Command::Serve)) {
    for warning in &warnings {
      eprintln!("warning: {warning}");
    }
  }
  match command {
    Some(Command::Config { command }) => {
      rilgate::app::validate(&config)?;
      match command {
        ConfigCommand::Check { strict } => {
          anyhow::ensure!(
            !strict || warnings.is_empty(),
            "configuration has {} warning(s); --strict treats warnings as errors",
            warnings.len()
          );
          println!("Configuration is valid");
        }
        ConfigCommand::Show { defaults } => print!("{}", document.display(defaults)?),
      }
      Ok(())
    }
    Some(Command::Web { command }) => rilgate::cli::web::run(&config.web, command),
    Some(Command::Verifier { .. }) => unreachable!("handled before gateway configuration"),
    Some(Command::Upgrade { .. }) => unreachable!("handled before configuration loading"),
    None | Some(Command::Serve) => {
      rilgate_telemetry::setup_tracing(
        args.log_level.as_deref(),
        args.log_format == LogFormat::Json,
      );
      for warning in &warnings {
        tracing::warn!("{warning}");
      }
      rilgate::cli::serve::run(config).await
    }
  }
}
