use super::settings::Settings;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Proof-of-work reverse proxy gateway.
///
/// Settings come from the YAML file, then environment variables, then flags;
/// each later source overrides the earlier one. Policy rules, IP sets and
/// signing key rotation are configured in YAML only.
#[derive(Parser, Debug)]
#[command(version, about, long_about)]
pub struct Args {
  /// YAML configuration file [default: rilgate.yml, then config/rilgate.yml]
  #[arg(
    short,
    long,
    global = true,
    env = "RILGATE_CONFIG",
    hide_env_values = true,
    value_name = "FILE"
  )]
  pub config: Option<PathBuf>,

  /// Load environment variables from this file instead of ./.env
  #[arg(long, global = true, value_name = "FILE")]
  pub env_file: Option<PathBuf>,

  /// Replace the port of the listen address
  #[arg(
    short,
    long,
    global = true,
    env = "RILGATE_PORT",
    hide_env_values = true
  )]
  pub port: Option<u16>,

  /// Log filter, such as info or rilgate=debug,pingora=error [default: rilgate=info,pingora=error]
  #[arg(
    long,
    global = true,
    env = "RUST_LOG",
    value_name = "FILTER",
    hide_env_values = true
  )]
  pub log_level: Option<String>,

  /// Log output format
  #[arg(long, global = true, env = "RILGATE_LOG_FORMAT", value_enum, default_value_t = LogFormat::Text, hide_env_values = true)]
  pub log_format: LogFormat,

  #[command(flatten)]
  pub settings: Settings,

  #[command(subcommand)]
  pub command: Option<Command>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LogFormat {
  Text,
  Json,
}

#[derive(Debug, Subcommand)]
pub enum Command {
  /// Run the gateway (default when no command is given)
  Serve,
  /// Validate or display the resolved configuration
  Config {
    #[command(subcommand)]
    command: ConfigCommand,
  },
  /// Initialize, validate or compare operator web resources
  Web {
    #[command(subcommand)]
    command: WebCommand,
  },
  /// Replace this binary with a release from GitHub
  Upgrade {
    /// Release tag to install, such as v0.2.0 [default: the latest release]
    #[arg(long, value_name = "TAG")]
    version: Option<String>,
    /// Install even when the release is not newer than this binary
    #[arg(long)]
    force: bool,
  },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
  /// Validate the configuration without opening listeners or contacting dependencies
  Check {
    /// Fail when the configuration produces any warning
    #[arg(long)]
    strict: bool,
  },
  /// Print the resolved configuration with secrets redacted
  Show {
    /// Include settings that still have their default value
    #[arg(long)]
    defaults: bool,
  },
}

#[derive(Debug, Subcommand)]
pub enum WebCommand {
  /// Copy the built-in templates, translations and static files into a directory
  Init { directory: PathBuf },
  /// Check a resource directory against the configured site
  Validate { directory: PathBuf },
  /// List differences between a resource directory and the built-in resources
  Diff { directory: PathBuf },
}

impl Args {
  /// Splits the arguments into configuration overrides and the remaining options.
  pub fn overrides(&mut self) -> rilgate_config::Overrides {
    rilgate_config::Overrides {
      config: self.config.clone(),
      port: self.port,
      settings: std::mem::take(&mut self.settings).into_settings(),
    }
  }
}

/// Finds `--env-file` before full parsing, so its variables can supply flag values.
pub fn env_file(arguments: &[std::ffi::OsString]) -> Option<PathBuf> {
  let mut arguments = arguments.iter().skip(1);
  while let Some(argument) = arguments.next() {
    let argument = argument.to_str()?;
    if argument == "--" {
      return None;
    }
    if argument == "--env-file" {
      return arguments.next().map(PathBuf::from);
    }
    if let Some(path) = argument.strip_prefix("--env-file=") {
      return Some(PathBuf::from(path));
    }
  }
  None
}

#[cfg(test)]
mod tests {
  use super::*;
  use clap::CommandFactory;
  use rilgate_config::SettingValue;

  #[test]
  fn command_definition_is_valid() {
    Args::command().debug_assert();
  }

  #[test]
  fn flags_become_settings_for_their_yaml_paths() {
    let mut args = Args::try_parse_from([
      "rilgate",
      "--server-upstream-url",
      "http://127.0.0.1:3000",
      "--server-metrics-enabled",
      "--site-i18n-strict=false",
      "--server-trusted-proxies",
      "10.0.0.0/8,192.0.2.1/32",
      "config",
      "check",
    ])
    .unwrap();
    let overrides = args.overrides();
    let find = |path: &str| {
      overrides
        .settings
        .iter()
        .find(|setting| setting.path == path)
        .map(|setting| (setting.value.clone(), setting.source.clone()))
    };
    assert_eq!(
      find("server.upstream.url"),
      Some((
        SettingValue::Scalar("http://127.0.0.1:3000".into()),
        "--server-upstream-url".into()
      ))
    );
    assert_eq!(
      find("server.metrics_enabled").unwrap().0,
      SettingValue::Scalar("true".into())
    );
    assert_eq!(
      find("site.i18n.strict").unwrap().0,
      SettingValue::Scalar("false".into())
    );
    assert_eq!(
      find("server.trusted_proxies").unwrap().0,
      SettingValue::List(vec!["10.0.0.0/8".into(), "192.0.2.1/32".into()])
    );
    assert!(find("server.listen").is_none());
    assert!(matches!(
      args.command,
      Some(Command::Config {
        command: ConfigCommand::Check { strict: false }
      })
    ));
  }

  #[test]
  fn env_file_is_found_before_parsing() {
    let args = |values: &[&str]| {
      values
        .iter()
        .map(std::ffi::OsString::from)
        .collect::<Vec<_>>()
    };
    assert_eq!(
      env_file(&args(&["rilgate", "--env-file", "a.env"])),
      Some("a.env".into())
    );
    assert_eq!(
      env_file(&args(&["rilgate", "--env-file=b.env", "serve"])),
      Some("b.env".into())
    );
    assert_eq!(
      env_file(&args(&["rilgate", "--", "--env-file", "c.env"])),
      None
    );
    assert_eq!(env_file(&args(&["rilgate", "serve"])), None);
  }
}
