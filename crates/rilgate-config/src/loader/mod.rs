mod builtin;
mod includes;
pub(crate) mod sources;
mod value;
use crate::{Overrides, Setting, SettingValue, schema::Document};
use anyhow::{Context, Result, ensure};
use includes::MAX_DEPTH;
use serde_yaml_ng::Value;
use std::path::{Path, PathBuf};

/// Configuration files tried in order, relative to the working directory, when
/// neither `--config` nor `RILGATE_CONFIG` selects one.
const DEFAULT_PATHS: &[&str] = &["rilgate.yml", "config/rilgate.yml"];

/// Configuration sections with a new name. An old name fails with the new one,
/// so an operator does not lose a setting without notice.
const RENAMED_SECTIONS: &[(&str, &str)] = &[("pow.p2pool", "pow.stratum_pool")];
/// Setting values with a new name, as (setting path, old value, new value).
const RENAMED_VALUES: &[(&str, &str, &str)] = &[(
  "pow.verification",
  "p2pool_accepted",
  "stratum_pool_accepted",
)];

/// A loaded document with warnings for the caller to report once logging is
/// available.
pub struct Loaded {
  pub document: Document,
  pub warnings: Vec<String>,
}

pub fn load(args: &Overrides) -> Result<Loaded> {
  reject_renamed_environment(std::env::vars_os().filter_map(|(name, _)| name.into_string().ok()))?;
  let explicit = args
    .config
    .clone()
    .or_else(|| std::env::var_os("RILGATE_CONFIG").map(PathBuf::from));
  let mut warnings = Vec::new();
  let mut resolver = includes::Resolver::default();
  let input = match explicit {
    Some(path) => Some((resolver.read(&path)?, path)),
    None => read_default(&mut warnings, &mut resolver)?,
  };
  let (value, config_sources) = match input {
    Some((text, path)) => resolver.resolve(&text, &path)?,
    None => (
      Value::Mapping(Default::default()),
      sources::ConfigurationSources::default(),
    ),
  };
  let mut document = document_from_value(value, &args.settings, config_sources)?;
  if let Some(port) = args.port {
    let addresses = document.config_sources.result(
      super::assembly::parse_listen(&document.server.listen, "server.listen"),
      &document.sources,
    )?;
    document.server.listen.0 = addresses
      .into_iter()
      .map(|mut address| {
        address.set_port(port);
        address.to_string()
      })
      .collect();
  }
  Ok(Loaded { document, warnings })
}

/// Reads the first default file that exists and warns about each later one,
/// which is ignored. No default file yields an empty document, so environment
/// variables can supply the required values.
fn read_default(
  warnings: &mut Vec<String>,
  resolver: &mut includes::Resolver,
) -> Result<Option<(String, PathBuf)>> {
  let mut paths = DEFAULT_PATHS.iter().map(Path::new);
  while let Some(used) = paths.next() {
    if let Some(text) = resolver.read_optional(used)? {
      for ignored in paths.filter(|path| path.is_file()) {
        warnings.push(format!(
          "ignoring configuration file {}; using {}. Remove one, or select one with --config",
          ignored.display(),
          used.display()
        ));
      }
      return Ok(Some((text, used.to_path_buf())));
    }
  }
  Ok(None)
}

pub fn parse(text: &str) -> Result<Document> {
  parse_with(text, &[])
}

/// Parses a YAML document and applies settings over it before deserialization,
/// so overridden values pass the same schema validation as YAML values.
pub fn parse_with(text: &str, settings: &[Setting]) -> Result<Document> {
  let (value, config_sources) = includes::Resolver::default()
    .resolve_inline(text)
    .map_err(|error| anyhow::anyhow!("{error:#}"))?;
  document_from_value(value, settings, config_sources)
}

fn document_from_value(
  value: Value,
  settings: &[Setting],
  config_sources: sources::ConfigurationSources,
) -> Result<Document> {
  let overrides: Vec<_> = settings
    .iter()
    .map(|setting| (setting.path.clone(), setting.source.clone()))
    .collect();
  let finish = || -> Result<Document> {
    reject_renamed(&value, settings)?;
    let mut value = expand(value, "", 0)?;
    for setting in settings {
      assign(&mut value, setting)
        .with_context(|| format!("{} ({})", setting.path, setting.source))?;
    }
    let mut document: Document = serde_path_to_error::deserialize(value).map_err(|error| {
      let path = error.path().to_string();
      let dotted = path.replace('[', ".").replace(']', "");
      let source = settings
        .iter()
        .rev()
        .find(|setting| {
          dotted == setting.path
            || dotted.starts_with(&format!("{}.", setting.path))
            || setting.path.starts_with(&format!("{dotted}."))
        })
        .map(|setting| format!(" (from {})", setting.source))
        .unwrap_or_default();
      let document_source = if source.is_empty() {
        config_sources.schema_context(error.path())
      } else {
        String::new()
      };
      sources::attributed(format!(
        "Configuration error at {path}{source}{document_source}: {}",
        error.inner()
      ))
    })?;
    document.sources = overrides.clone();
    Ok(document)
  };
  match finish() {
    Ok(mut document) => {
      document.config_sources = config_sources;
      Ok(document)
    }
    Err(error) => Err(config_sources.annotate(error, &overrides)),
  }
}

fn reject_renamed(document: &Value, settings: &[Setting]) -> Result<()> {
  let lookup = |path: &str| {
    path
      .split('.')
      .try_fold(document, |node, key| node.get(key))
  };
  for (old, new) in RENAMED_SECTIONS {
    ensure!(lookup(old).is_none(), "{old}: renamed to {new}");
  }
  for (path, old, new) in RENAMED_VALUES {
    let in_document = lookup(path).and_then(Value::as_str) == Some(*old);
    let in_settings = settings.iter().any(|setting| {
      setting.path == *path && matches!(&setting.value, SettingValue::Scalar(text) if text == old)
    });
    ensure!(
      !in_document && !in_settings,
      "{path}: {old} is renamed to {new}"
    );
  }
  Ok(())
}

/// Fails for an environment variable of a renamed section. Without this check,
/// the variable has no effect.
fn reject_renamed_environment(names: impl Iterator<Item = String>) -> Result<()> {
  let variable = |path: &str| format!("RILGATE_{}_", path.replace('.', "_").to_uppercase());
  for name in names {
    let indexed = match name.as_str() {
      "RILGATE_POW_P2POOL_ADDRESS" | "RILGATE_POW_STRATUM_POOL_ADDRESS" => {
        Some("RILGATE_POW_STRATUM_POOL_POOLS_0_ADDRESS")
      }
      "RILGATE_POW_P2POOL_PASSWORD" | "RILGATE_POW_STRATUM_POOL_PASSWORD" => {
        Some("RILGATE_POW_STRATUM_POOL_POOLS_0_LOGIN_PASSWORD")
      }
      _ => None,
    };
    if let Some(indexed) = indexed {
      anyhow::bail!("{name}: renamed to {indexed}");
    }
    for (old, new) in RENAMED_SECTIONS {
      let prefix = variable(old);
      if let Some(rest) = name.strip_prefix(&prefix) {
        anyhow::bail!("{name}: renamed to {}{rest}", variable(new));
      }
    }
  }
  Ok(())
}

fn assign(root: &mut value::Expanded, setting: &Setting) -> Result<()> {
  use value::Expanded;
  let scalar = |text: &String| -> Result<Expanded> {
    ensure!(text.len() <= 65_536, "value exceeds 64 KiB");
    Ok(Expanded::Environment(text.clone()))
  };
  let replacement = match &setting.value {
    SettingValue::Scalar(text) => scalar(text)?,
    SettingValue::List(items) => {
      Expanded::Sequence(items.iter().map(scalar).collect::<Result<_>>()?)
    }
  };
  let mut node = root;
  let mut keys = setting.path.split('.').peekable();
  while let Some(key) = keys.next() {
    let numeric = key.parse::<usize>().ok();
    if matches!(node, Expanded::Scalar(Value::Null)) {
      *node = if numeric.is_some() {
        Expanded::Sequence(Vec::new())
      } else {
        Expanded::Mapping(Vec::new())
      };
    }
    if let Some(index) = numeric {
      ensure!(index < 8, "list index must be less than 8");
      let Expanded::Sequence(items) = node else {
        anyhow::bail!("expected a YAML list above {key}");
      };
      ensure!(
        index <= items.len(),
        "list index {index} leaves a gap; next index is {}",
        items.len()
      );
      if index == items.len() {
        items.push(Expanded::Scalar(Value::Null));
      }
      if keys.peek().is_none() {
        items[index] = replacement;
        return Ok(());
      }
      node = &mut items[index];
      continue;
    }
    let Expanded::Mapping(entries) = node else {
      anyhow::bail!("expected a YAML mapping above {key}");
    };
    let index = match entries.iter().position(|(name, _)| name == key) {
      Some(index) => index,
      None => {
        entries.push((key.to_string(), Expanded::Scalar(Value::Null)));
        entries.len() - 1
      }
    };
    if keys.peek().is_none() {
      entries[index].1 = replacement;
      return Ok(());
    }
    node = &mut entries[index].1;
  }
  anyhow::bail!("empty setting path")
}

fn expand(value: Value, path: &str, depth: usize) -> Result<value::Expanded> {
  use value::Expanded;
  ensure!(
    depth <= MAX_DEPTH,
    "{path}: YAML nesting must not exceed {MAX_DEPTH} levels"
  );
  Ok(match value {
    Value::String(text) if text.contains("${") => Expanded::Environment(interpolate(&text, path)?),
    Value::Mapping(map) => {
      let mut values = Vec::with_capacity(map.len());
      for (key, value) in map {
        let key = key
          .as_str()
          .with_context(|| format!("{path}: configuration keys must be strings"))?
          .to_string();
        let child = if path.is_empty() {
          key.clone()
        } else {
          format!("{path}.{key}")
        };
        values.push((key, expand(value, &child, depth + 1)?));
      }
      Expanded::Mapping(values)
    }
    Value::Sequence(values) => Expanded::Sequence(
      values
        .into_iter()
        .enumerate()
        .map(|(index, value)| expand(value, &format!("{path}[{index}]"), depth + 1))
        .collect::<Result<_>>()?,
    ),
    Value::Tagged(_) => anyhow::bail!("{path}: YAML tags are unsupported"),
    Value::String(text) => {
      ensure!(text.len() <= 65_536, "{path}: value exceeds 64 KiB");
      Expanded::Scalar(Value::String(text))
    }
    scalar => Expanded::Scalar(scalar),
  })
}

fn interpolate(text: &str, path: &str) -> Result<String> {
  let mut remaining = text;
  let mut output = String::new();
  while let Some(index) = remaining.find("${") {
    output.push_str(&remaining[..index]);
    remaining = &remaining[index + 2..];
    let end = remaining
      .find('}')
      .with_context(|| format!("{path}: unterminated environment variable reference"))?;
    let name = &remaining[..end];
    ensure!(
      !name.is_empty()
        && name
          .bytes()
          .enumerate()
          .all(|(i, c)| c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())),
      "{path}: expected ${{VARIABLE_NAME}}"
    );
    let value = std::env::var(name).with_context(|| {
      format!("{path}: required environment variable {name} is missing or not UTF-8")
    })?;
    ensure!(
      !value.is_empty(),
      "{path}: required environment variable {name} is empty"
    );
    output.push_str(&value);
    ensure!(
      output.len() <= 65_536,
      "{path}: expanded value exceeds 64 KiB"
    );
    remaining = &remaining[end + 1..];
  }
  output.push_str(remaining);
  ensure!(output.len() <= 65_536, "{path}: value exceeds 64 KiB");
  Ok(output)
}

#[cfg(test)]
mod tests {
  #[test]
  fn environment_variables_of_renamed_sections_fail_with_the_new_name() {
    let error =
      super::reject_renamed_environment(["RILGATE_POW_P2POOL_ADDRESS".to_string()].into_iter())
        .unwrap_err();
    assert_eq!(
      error.to_string(),
      "RILGATE_POW_P2POOL_ADDRESS: renamed to RILGATE_POW_STRATUM_POOL_POOLS_0_ADDRESS"
    );
    assert!(
      super::reject_renamed_environment(
        ["RILGATE_POW_STRATUM_POOL_POOLS_0_ADDRESS".to_string()].into_iter()
      )
      .is_ok()
    );
  }
}

#[cfg(test)]
mod indexed_tests {
  use super::*;

  #[test]
  fn removed_single_pool_variables_fail_with_indexed_migration_paths() {
    for (name, field) in [
      ("RILGATE_POW_STRATUM_POOL_ADDRESS", "ADDRESS"),
      ("RILGATE_POW_STRATUM_POOL_PASSWORD", "LOGIN_PASSWORD"),
    ] {
      let error = reject_renamed_environment([name.to_string()].into_iter()).unwrap_err();
      assert_eq!(
        error.to_string(),
        format!("{name}: renamed to RILGATE_POW_STRATUM_POOL_POOLS_0_{field}")
      );
    }
  }

  #[test]
  fn indexed_overrides_update_yaml_items_append_contiguously_and_name_invalid_sources() {
    let base =
      "pow:\n  stratum_pool:\n    pools:\n      - {name: primary, address: localhost:3333}\n";
    let setting = |path: &str, text: &str| Setting {
      path: path.into(),
      source: "RILGATE_POW_STRATUM_POOL_POOLS_1_ADDRESS".into(),
      value: SettingValue::Scalar(text.into()),
    };
    let document = parse_with(
      base,
      &[
        setting("pow.stratum_pool.pools.0.address", "localhost:4444"),
        setting("pow.stratum_pool.pools.1.name", "backup"),
        setting("pow.stratum_pool.pools.1.address", "localhost:5555"),
      ],
    )
    .unwrap();
    let pools = document.pow.stratum_pool.unwrap().pools;
    assert_eq!(pools[0].address, "localhost:4444");
    assert_eq!(pools[1].address, "localhost:5555");
    let error = format!(
      "{:#}",
      parse_with(
        base,
        &[setting("pow.stratum_pool.pools.2.address", "localhost:1")]
      )
      .unwrap_err()
    );
    assert!(error.contains("leaves a gap"), "{error}");
    assert!(error.contains("RILGATE_POW_STRATUM_POOL_POOLS_1_ADDRESS"));
  }
}
