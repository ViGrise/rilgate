mod value;
use crate::{Overrides, schema::Document};
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
  let mut document = parse(&text)?;
  if let Some(value) = std::env::var_os("RILGATE_SECRET") {
    ensure!(
      document.security.keys.is_none(),
      "security: RILGATE_SECRET conflicts with security.keys"
    );
    document.security.secret =
      Some(rilgate_crypto::Secret(value.into_string().map_err(
        |_| anyhow::anyhow!("RILGATE_SECRET must be UTF-8"),
      )?));
  }
  if let Ok(value) = std::env::var("RILGATE_UPSTREAM_URL") {
    document.server.upstream.url = value;
  }
  if let Ok(value) = std::env::var("RILGATE_LISTEN") {
    document.server.listen = value;
  }
  if let Some(value) = args.env {
    document.environment = value;
  }
  if let Some(port) = args.port {
    let mut address = super::assembly::listen_address(&document.server.listen)?;
    address.set_port(port);
    document.server.listen = address.to_string();
  }
  if let Some(value) = args.pow_enforce {
    document.pow.verification = value;
  }
  Ok(document)
}

pub fn parse(text: &str) -> Result<Document> {
  ensure!(
    text.len() <= 1_048_576,
    "configuration must not exceed 1 MiB"
  );
  let value: Value = if text.trim().is_empty() {
    Value::Mapping(Default::default())
  } else {
    serde_yaml_ng::from_str(text).context("configuration YAML")?
  };
  let value = expand(value, "", 0)?;
  serde_path_to_error::deserialize(value)
    .map_err(|error| anyhow::anyhow!("Configuration error at {}: {}", error.path(), error.inner()))
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
