use serde::de::DeserializeOwned;
use serde_yaml_ng::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use tracing::warn;

pub const DEFAULT_CONFIG_PATH: &str = "config/rilgate.yml";
pub const CONFIG_PATH_VAR: &str = "CONFIG";

#[derive(Debug)]
pub struct FileConfig {
  root: Value,
  values: BTreeMap<String, String>,
  read: RefCell<HashSet<String>>,
}

impl Default for FileConfig {
  fn default() -> Self {
    Self {
      root: Value::Mapping(Default::default()),
      values: BTreeMap::new(),
      read: RefCell::new(HashSet::new()),
    }
  }
}

impl FileConfig {
  pub fn load(explicit_path: Option<&Path>) -> Self {
    let explicit = explicit_path
      .map(PathBuf::from)
      .or_else(|| std::env::var(CONFIG_PATH_VAR).ok().map(PathBuf::from));

    let required = explicit.is_some();
    let path = explicit.unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH));

    let contents = match std::fs::read_to_string(&path) {
      Ok(contents) => contents,
      Err(err) => {
        if required {
          panic!(
            "Unable to read configuration file {}: {}",
            path.display(),
            err
          );
        }
        return Self::default();
      }
    };

    let root: Value = serde_yaml_ng::from_str(&contents).unwrap_or_else(|e| {
      panic!(
        "Unable to parse configuration file {}: {}",
        path.display(),
        e
      )
    });

    let mut values = BTreeMap::new();
    flatten(&root, String::new(), &mut values);

    Self {
      root,
      values,
      read: RefCell::new(HashSet::new()),
    }
  }

  pub fn get(&self, path: &str) -> Option<&str> {
    self.read.borrow_mut().insert(path.to_string());
    self.values.get(path).map(String::as_str)
  }

  pub fn section<T: DeserializeOwned>(&self, path: &str) -> Option<T> {
    self.read.borrow_mut().insert(path.to_string());

    let mut current = &self.root;
    for segment in path.split('.') {
      let mapping = current.as_mapping()?;
      current = mapping.get(Value::String(segment.to_string()))?;
    }

    Some(
      serde_yaml_ng::from_value(current.clone())
        .unwrap_or_else(|error| panic!("Unable to parse configuration section {path}: {error}")),
    )
  }

  fn unknown_keys(&self) -> Vec<&str> {
    let read = self.read.borrow();
    self
      .values
      .keys()
      .map(String::as_str)
      .filter(|key| {
        !read.contains(*key)
          && !read.iter().any(|prefix| {
            key.starts_with(prefix.as_str()) && key.as_bytes().get(prefix.len()) == Some(&b'.')
          })
      })
      .collect()
  }

  pub fn warn_unknown_keys(&self) {
    let unknown = self.unknown_keys();
    if !unknown.is_empty() {
      warn!(keys = ?unknown, "Ignoring unknown configuration file keys");
    }
  }

  pub fn reject_unknown_keys(&self) {
    let unknown = self.unknown_keys();
    assert!(
      unknown.is_empty(),
      "Unknown production configuration keys: {unknown:?}"
    );
  }
}

fn flatten(value: &Value, prefix: String, out: &mut BTreeMap<String, String>) {
  match value {
    Value::Mapping(mapping) => {
      for (key, child) in mapping {
        let Some(key) = key.as_str() else {
          continue;
        };
        let path = if prefix.is_empty() {
          key.to_string()
        } else {
          format!("{}.{}", prefix, key)
        };
        flatten(child, path, out);
      }
    }
    Value::Null => {}
    Value::String(s) => {
      out.insert(prefix, s.clone());
    }
    Value::Bool(b) => {
      out.insert(prefix, b.to_string());
    }
    Value::Number(n) => {
      out.insert(prefix, n.to_string());
    }
    Value::Sequence(_) => {
      out.insert(prefix, String::new());
    }
    Value::Tagged(_) => {}
  }
}
