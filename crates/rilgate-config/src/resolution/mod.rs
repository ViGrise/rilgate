pub(crate) mod compatibility;
use crate::loader::file::FileConfig;
use std::fmt;
use std::str::FromStr;
fn env_name(path: &str) -> String {
  path.to_uppercase().replace('.', "_")
}

pub(crate) fn resolve<T>(
  file: &FileConfig,
  path: &str,
  aliases: &[&str],
  arg: Option<T>,
  default: T,
) -> T
where
  T: FromStr,
  <T as FromStr>::Err: fmt::Display,
{
  lookup(file, path, aliases, arg).unwrap_or(default)
}

pub(crate) fn lookup<T>(
  file: &FileConfig,
  path: &str,
  aliases: &[&str],
  arg: Option<T>,
) -> Option<T>
where
  T: FromStr,
  <T as FromStr>::Err: fmt::Display,
{
  let from_file = file.get(path);

  if arg.is_some() {
    return arg;
  }

  let from_env = std::iter::once(env_name(path))
    .chain(aliases.iter().map(|alias| alias.to_string()))
    .find_map(|name| env_value(&name, path));

  from_env.or_else(|| from_file.map(|raw| parse(raw, path)))
}

fn env_value<T>(name: &str, path: &str) -> Option<T>
where
  T: FromStr,
  <T as FromStr>::Err: fmt::Display,
{
  let raw = std::env::var(name).ok()?;
  if raw.is_empty() {
    return None;
  }

  Some(parse(&raw, path))
}

fn parse<T>(raw: &str, path: &str) -> T
where
  T: FromStr,
  <T as FromStr>::Err: fmt::Display,
{
  raw
    .parse()
    .unwrap_or_else(|e| panic!("Unable to parse the value of {path}: {e}"))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn command_line_values_override_invalid_file_scalars() {
    let path = std::env::temp_dir().join(format!("pow-config-{}.yml", uuid::Uuid::new_v4()));
    std::fs::write(&path, "serve:\n  port: invalid\n").unwrap();
    let file = FileConfig::load(Some(&path));
    std::fs::remove_file(path).unwrap();
    assert_eq!(
      lookup::<u16>(&file, "serve.port", &[], Some(9090)),
      Some(9090)
    );
  }
}
