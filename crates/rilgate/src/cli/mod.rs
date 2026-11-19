mod args;
pub use args::{Args, Command, ConfigCommand, LogFormat, VerifierCommand, WebCommand, env_file};
pub mod config;
mod release_signature;
pub mod reload;
pub mod serve;
mod settings;
pub use settings::Settings;
pub mod upgrade;
pub mod web;

#[cfg(test)]
mod env_example;

use anyhow::Result;
use rilgate_config::{
  Overrides,
  loader::{self, Loaded, Lookup},
};

// Printed before text logs only, so JSON logs stay one object per line.
const BANNER: &str = r"
 ____  ___ _      ____    _  _____ _____
|  _ \|_ _| |    / ___|  / \|_   _| ____|
| |_) || || |   | |  _  / _ \ | | |  _|
|  _ < | || |___| |_| |/ ___ \| | | |___
|_| \_\___|_____|\____/_/   \_\_| |_____|
";

enum Invocation {
  Upgrade {
    version: Option<String>,
    force: bool,
  },
  Verifier(VerifierCommand),
  Gateway(GatewayCommand),
}

enum GatewayCommand {
  Serve,
  Config(ConfigCommand),
  Reload,
  Web(WebCommand),
}

impl From<Option<Command>> for Invocation {
  fn from(command: Option<Command>) -> Self {
    match command {
      None | Some(Command::Serve) => Self::Gateway(GatewayCommand::Serve),
      Some(Command::Upgrade { version, force }) => Self::Upgrade { version, force },
      Some(Command::Verifier { command }) => Self::Verifier(command),
      Some(Command::Config { command }) => Self::Gateway(GatewayCommand::Config(command)),
      Some(Command::Reload) => Self::Gateway(GatewayCommand::Reload),
      Some(Command::Web { command }) => Self::Gateway(GatewayCommand::Web(command)),
    }
  }
}

/// Runs the command selected by the parsed arguments.
pub async fn run(mut args: Args) -> Result<()> {
  let invocation = Invocation::from(args.command.take());
  let overrides = args.overrides();
  match invocation {
    Invocation::Upgrade { version, force } => upgrade::run(version, force).await,
    Invocation::Verifier(command) => run_verifier(command, &args, overrides).await,
    Invocation::Gateway(command) => run_gateway(command, &args, overrides).await,
  }
}

async fn run_verifier(command: VerifierCommand, args: &Args, overrides: Overrides) -> Result<()> {
  let Loaded {
    document,
    mut warnings,
    lookup,
    ..
  } = loader::load(&overrides)?;
  let config = document.resolve_verifier()?;
  warnings.extend(config.warnings().iter().map(ToString::to_string));
  crate::verifier::validate(&config)?;
  match command {
    VerifierCommand::Serve => {
      setup_tracing(args);
      log_lookup(&lookup);
      for warning in warnings {
        tracing::warn!("{warning}");
      }
      crate::verifier::run(config).await
    }
    VerifierCommand::Config { command } => {
      for warning in &warnings {
        eprintln!("warning: {warning}");
      }
      config_command(command, &warnings, |defaults| {
        document.display_verifier(defaults)
      })
    }
  }
}

async fn run_gateway(command: GatewayCommand, args: &Args, overrides: Overrides) -> Result<()> {
  let Loaded {
    document,
    mut warnings,
    lookup,
    files,
  } = loader::load(&overrides)?;
  let config = document.resolve()?;
  warnings.extend(config.warnings().iter().map(ToString::to_string));
  // Standard output carries command results and serve logs, so command
  // warnings go to standard error and serve warnings go through tracing.
  if !matches!(command, GatewayCommand::Serve) {
    for warning in &warnings {
      eprintln!("warning: {warning}");
    }
  }
  match command {
    GatewayCommand::Config(command) => {
      crate::app::validate(&config)?;
      config_command(command, &warnings, |defaults| document.display(defaults))
    }
    GatewayCommand::Reload => reload::run(&config).await,
    GatewayCommand::Web(command) => web::run(&config.web, command),
    GatewayCommand::Serve => {
      setup_tracing(args);
      if args.log_format != LogFormat::Json {
        println!("{BANNER}");
      }
      log_lookup(&lookup);
      tracing::info!(
        "Rilgate {} ({}), configuration {}, upstream {}",
        crate::VERSION,
        config.env,
        lookup.path().map_or_else(
          || "from flags and environment".into(),
          |path| path.display().to_string()
        ),
        config.upstream.url,
      );
      log_settings(&config);
      for warning in &warnings {
        tracing::warn!("{warning}");
      }
      serve::run(config, overrides, files).await
    }
  }
}

/// Logs the protection settings that decide how requests are handled, without secrets.
fn log_settings(config: &rilgate_config::Config) {
  fn name<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
      Ok(serde_json::Value::String(text)) => text,
      _ => "unknown".into(),
    }
  }
  let policy = &config.pow.policy.policy;
  let responses = &config.web.responses;
  tracing::info!(
    rules = policy.rules.as_ref().map_or(0, Vec::len),
    default_action = name(
      &policy
        .default_action
        .unwrap_or(rilgate_config::schema::policy::Action::Allow)
    ),
    default_algorithm = name(&config.pow.default_algorithm),
    hashwx = config.pow.hashwx_enabled,
    randomx = config.pow.randomx_enabled,
    trusted_proxies = config.server.trusted_proxies.len(),
    client_ip_header = config
      .server
      .client_ip_header
      .as_ref()
      .map_or("x-forwarded-for", |header| header.as_str()),
    decision_log = name(&config.server.decision_log),
    metrics = config.server.operations_metrics,
    challenge_status = responses.challenge_status.as_u16(),
    deny_status = responses.deny_status.as_u16(),
    "Gateway settings"
  );
}

fn config_command(
  command: ConfigCommand,
  warnings: &[String],
  show: impl FnOnce(bool) -> Result<String>,
) -> Result<()> {
  match command {
    ConfigCommand::Check { strict } => {
      anyhow::ensure!(
        !strict || warnings.is_empty(),
        "configuration has {} warning(s); --strict treats warnings as errors",
        warnings.len()
      );
      println!("Configuration is valid");
    }
    ConfigCommand::Show { defaults } => print!("{}", show(defaults)?),
  }
  Ok(())
}

fn setup_tracing(args: &Args) {
  rilgate_telemetry::setup_tracing(
    args.log_level.as_deref(),
    args.log_format == LogFormat::Json,
  );
}

fn log_lookup(lookup: &Lookup) {
  match std::env::current_dir() {
    Ok(directory) => tracing::info!("Working directory {}", directory.display()),
    Err(error) => tracing::warn!("Working directory is unavailable: {error}"),
  }
  match lookup {
    Lookup::Selected(path) => tracing::info!(
      "Using configuration file {} from --config or RILGATE_CONFIG",
      path.display()
    ),
    Lookup::Default(tried) => {
      for (path, found) in tried {
        let result = if *found { "found" } else { "not found" };
        tracing::info!("Trying configuration file {}: {result}", path.display());
      }
    }
  }
}
