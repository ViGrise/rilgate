//! Generates `.env.example` from the settings table, the schema defaults and the
//! setting descriptions in `config/rilgate.example.yml`, and checks the
//! committed file matches. Set `RILGATE_UPDATE_ENV_EXAMPLE=1` when running the
//! test to rewrite the file.
use super::settings::{Kind, SPECS};
use std::collections::HashMap;
use std::fmt::Write;

const HEADER: &str = "\
# Rilgate environment configuration
#
# Each variable sets the YAML setting named in its comment and overrides the
# configuration file; command-line flags override variables. A variable that is
# unset keeps the YAML value or its default. Variable names follow the YAML path:
# server.upstream.url is RILGATE_SERVER_UPSTREAM_URL and --server-upstream-url.
#
# Values use the same syntax as YAML values. Durations use units such as 300ms,
# 5s, 1m and 6h; sizes use 1024B, 1KiB, 16MiB and 1GiB; booleans are true or
# false; lists are comma-separated and replace the YAML list, and an empty value
# clears it. Invalid values fail validation and name their variable.
#
# Copy this file to .env in the working directory, or pass --env-file FILE.
# Variables already set in the process environment take precedence over the
# file. Policy rules, IP sets and signing key rotation are configured in YAML.
# Optional variables are commented out; values shown are defaults.
#
# Check the result: rilgate config check
# Inspect resolved values: rilgate config show --defaults
";

/// Process options without a YAML setting, with their descriptions.
const PROCESS: &[(&str, &str, &str)] = &[
  (
    "RILGATE_CONFIG",
    "rilgate.yml",
    "YAML configuration file. Without this, Rilgate reads the first of\n\
     rilgate.yml and config/rilgate.yml that exists and warns when both exist.\n\
     An explicitly selected missing file fails; missing default files are\n\
     allowed when variables supply the required values. Flag: --config",
  ),
  (
    "RILGATE_PORT",
    "",
    "Replaces the ports of server.listen addresses; preserves operations addresses. Flag: --port",
  ),
  (
    "RUST_LOG",
    "rilgate=info,pingora=error",
    "Log filter. Per-request diagnostics are DEBUG; raise the filter when needed.\n\
     Flag: --log-level",
  ),
  (
    "RILGATE_LOG_FORMAT",
    "text",
    "Log output: text or json. Flag: --log-format",
  ),
];

/// Values set in the example because the gateway needs them to start.
const REQUIRED: &[(&str, &str)] = &[
  ("server.upstream.url", "http://127.0.0.1:3000"),
  ("security.secret", ""),
];

pub(crate) fn render(yaml_example: &str) -> String {
  let descriptions = descriptions(yaml_example);
  let defaults = defaults();
  let mut out = String::from(HEADER);
  section(&mut out, "PROCESS");
  for (name, default, help) in PROCESS {
    out.push('\n');
    for line in help.lines() {
      let _ = writeln!(out, "# {}", line.trim());
    }
    let _ = writeln!(out, "#\n# {name}={default}");
  }
  let mut heading = "";
  for spec in SPECS {
    if spec.heading != heading {
      heading = spec.heading;
      section(&mut out, &heading.to_uppercase());
    }
    out.push('\n');
    let lines = descriptions
      .get(spec.path)
      .cloned()
      .unwrap_or_else(|| vec![format!("{}.", spec.help)]);
    for line in lines {
      if line.is_empty() {
        out.push_str("#\n");
      } else {
        let _ = writeln!(out, "# {line}");
      }
    }
    let _ = writeln!(out, "# YAML: {}. Flag: --{}\n#", spec.path, spec.flag);
    match REQUIRED.iter().find(|(path, _)| *path == spec.path) {
      Some((_, value)) => {
        let _ = writeln!(out, "{}={value}", spec.env);
      }
      None => {
        let value = match spec.kind {
          Kind::Secret => String::new(),
          _ => defaults.get(spec.path).cloned().unwrap_or_default(),
        };
        let _ = writeln!(out, "# {}={value}", spec.env);
      }
    }
  }
  out
}

fn section(out: &mut String, title: &str) {
  let title = format!(" {title} ");
  let left = (79 - title.len()) / 2;
  let right = 79 - title.len() - left;
  let _ = write!(out, "\n{}{title}{}\n", "#".repeat(left), "#".repeat(right));
}

fn defaults() -> HashMap<&'static str, String> {
  let root = super::settings::defaults().expect("schema defaults");
  SPECS
    .iter()
    .filter_map(|spec| Some((spec.path, super::settings::default_text(&root, spec.path)?)))
    .collect()
}

/// Maps each YAML path to the comment block above its key in the example.
///
/// The example nests keys by indentation, two columns per level, whether a key
/// is commented out or not. A description is the comment block that ends with
/// a bare `#` line directly above the key.
fn descriptions(yaml: &str) -> HashMap<String, Vec<String>> {
  let mut found = HashMap::new();
  let mut stack: Vec<(usize, String)> = Vec::new();
  let mut block: Vec<String> = Vec::new();
  let mut separated = false;
  for line in yaml.lines() {
    let indent = line.len() - line.trim_start().len();
    let trimmed = line.trim_start();
    let (commented, body) = match trimmed.strip_prefix('#') {
      Some(rest) => (true, rest.strip_prefix(' ').unwrap_or(rest)),
      None => (false, trimmed),
    };
    let key = body
      .split_once(':')
      .filter(|(key, _)| {
        !key.is_empty()
          && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'_' || b.is_ascii_digit())
      })
      .map(|(key, value)| (key.to_string(), value.trim().is_empty()));
    match key {
      Some((key, parent)) if !commented || separated || parent => {
        stack.retain(|(level, _)| *level < indent);
        let path = stack
          .iter()
          .map(|(_, key)| key.as_str())
          .chain(std::iter::once(key.as_str()))
          .collect::<Vec<_>>()
          .join(".");
        if separated && !block.is_empty() {
          found.insert(path, std::mem::take(&mut block));
        }
        if parent {
          stack.push((indent, key));
        }
        block.clear();
        separated = false;
      }
      _ if commented && body.trim().is_empty() => separated = !block.is_empty(),
      _ if commented && !body.starts_with("##") => {
        if separated {
          block.push(String::new());
          separated = false;
        }
        block.push(body.trim_end().to_string());
      }
      _ => {
        block.clear();
        separated = false;
      }
    }
  }
  found
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn env_example_matches_the_settings_and_yaml_example() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let yaml = std::fs::read_to_string(format!("{root}/config/rilgate.example.yml")).unwrap();
    let expected = render(&yaml);
    let path = format!("{root}/.env.example");
    if std::env::var_os("RILGATE_UPDATE_ENV_EXAMPLE").is_some() {
      std::fs::write(&path, &expected).unwrap();
    }
    let actual = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
      actual == expected,
      ".env.example is out of date; rerun with RILGATE_UPDATE_ENV_EXAMPLE=1"
    );
  }

  #[test]
  fn descriptions_follow_key_nesting() {
    let yaml = "server:\n  # Bind address.\n  # Second line.\n  #\n  # listen: \":8080\"\n\n  upstream:\n    # Upstream URL.\n    #\n    url: x\n";
    let found = descriptions(yaml);
    assert_eq!(
      found["server.listen"],
      vec!["Bind address.", "Second line."]
    );
    assert_eq!(found["server.upstream.url"], vec!["Upstream URL."]);
  }
}
