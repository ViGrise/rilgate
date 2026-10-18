mod value;
use crate::{Overrides, Setting, SettingValue, schema::Document};
use anyhow::{Context, Result, ensure};
use serde_yaml_ng::Value;
use std::path::PathBuf;

pub fn load(args: &Overrides) -> Result<Document> {
  let explicit = args
    .config
    .clone()
    .or_else(|| std::env::var_os("RILGATE_CONFIG").map(PathBuf::from));
  let path = explicit
    .clone()
    .unwrap_or_else(|| PathBuf::from("config/rilgate.yml"));
  let text = match std::fs::read_to_string(&path) {
    Ok(text) => text,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound && explicit.is_none() => {
      String::new()
    }
    Err(error) => {
      return Err(error).with_context(|| format!("configuration file {}", path.display()));
    }
  };
  let mut document = parse_with(&text, &args.settings)?;
  if let Some(port) = args.port {
    let addresses = super::assembly::parse_listen(&document.server.listen)?;
    document.server.listen.0 = addresses
      .into_iter()
      .map(|mut address| {
        address.set_port(port);
        address.to_string()
      })
      .collect();
  }
  Ok(document)
}

pub fn parse(text: &str) -> Result<Document> {
  parse_with(text, &[])
}

/// Parses a YAML document and applies settings over it before deserialization,
/// so overridden values pass the same schema validation as YAML values.
pub fn parse_with(text: &str, settings: &[Setting]) -> Result<Document> {
  ensure!(
    text.len() <= 1_048_576,
    "configuration must not exceed 1 MiB"
  );
  let value: Value = if text.trim().is_empty() {
    Value::Mapping(Default::default())
  } else {
    serde_yaml_ng::from_str(text).context("configuration YAML")?
  };
  let mut value = expand(value, "", 0)?;
  for setting in settings {
    assign(&mut value, setting)
      .with_context(|| format!("{} ({})", setting.path, setting.source))?;
  }
  serde_path_to_error::deserialize(value).map_err(|error| {
    let path = error.path().to_string();
    let source = settings
      .iter()
      .rev()
      .find(|setting| path == setting.path || path.starts_with(&format!("{}[", setting.path)))
      .map(|setting| format!(" (from {})", setting.source))
      .unwrap_or_default();
    anyhow::anyhow!("Configuration error at {path}{source}: {}", error.inner())
  })
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
    // An absent or null section becomes a mapping, so a setting can enable an
    // optional section such as `pow.p2pool`.
    if matches!(node, Expanded::Scalar(Value::Null)) {
      *node = Expanded::Mapping(Vec::new());
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
    depth <= 64,
    "{path}: YAML nesting must not exceed 64 levels"
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
