use anyhow::Result;
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Segment {
  Key(String),
  Index(usize),
}

pub(super) type ConfigPath = Vec<Segment>;

pub(super) fn display(path: &[Segment]) -> String {
  let mut text = String::new();
  for segment in path {
    match segment {
      Segment::Key(key) => {
        if !text.is_empty() {
          text.push('.');
        }
        text.push_str(key);
      }
      Segment::Index(index) => text.push_str(&format!("[{index}]")),
    }
  }
  if text.is_empty() { ".".into() } else { text }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Source {
  File(PathBuf),
  Builtin(String),
  Inline,
}

impl std::fmt::Display for Source {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::File(path) => write!(formatter, "{}", path.display()),
      Self::Builtin(path) => write!(formatter, "builtin:{path}"),
      Self::Inline => formatter.write_str("inline configuration"),
    }
  }
}

#[derive(Clone, Debug)]
pub(super) struct SourceDocument {
  pub source: Source,
  pub chain: Vec<Source>,
}

#[derive(Clone, Debug)]
pub(super) struct Origin {
  pub document: Arc<SourceDocument>,
  pub local_path: ConfigPath,
}

impl Origin {
  pub fn describe(&self) -> String {
    let mut text = format!("{} at {}", self.document.source, display(&self.local_path));
    if self.document.chain.len() > 1 {
      text.push_str(&format!(
        " (include chain: {})",
        self
          .document
          .chain
          .iter()
          .map(ToString::to_string)
          .collect::<Vec<_>>()
          .join(" -> ")
      ));
    }
    text
  }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ConfigurationSources(pub(super) Vec<(ConfigPath, Origin)>);

impl ConfigurationSources {
  pub(super) fn schema_context(&self, path: &serde_path_to_error::Path) -> String {
    let segments: ConfigPath = path
      .iter()
      .filter_map(|segment| match segment {
        serde_path_to_error::Segment::Map { key } => Some(Segment::Key(key.clone())),
        serde_path_to_error::Segment::Seq { index } => Some(Segment::Index(*index)),
        _ => None,
      })
      .collect();
    let depth = self
      .0
      .iter()
      .filter(|(candidate, _)| segments.starts_with(candidate))
      .map(|(candidate, _)| candidate.len())
      .max();
    let origins = self
      .0
      .iter()
      .filter(|(candidate, _)| Some(candidate.len()) == depth && segments.starts_with(candidate))
      .map(|(_, origin)| origin.describe())
      .collect::<BTreeSet<_>>();
    if origins.is_empty() {
      String::new()
    } else {
      format!(" (source {})", describe_sources(origins))
    }
  }

  pub(crate) fn annotate(
    &self,
    error: anyhow::Error,
    overrides: &[(String, String)],
  ) -> anyhow::Error {
    if error.is::<AttributedError>() {
      return error;
    }
    let message = format!("{error:#}");
    let paths: Vec<String> = error
      .chain()
      .flat_map(|cause| {
        cause
          .to_string()
          .lines()
          .map(|line| {
            line
              .split_once(':')
              .map_or(line, |(path, _)| path)
              .trim()
              .to_string()
          })
          .collect::<Vec<_>>()
      })
      .collect();
    let override_sources: BTreeSet<_> = overrides
      .iter()
      .filter(|(path, _)| {
        paths
          .iter()
          .any(|error_path| related(path, &error_path.replace('[', ".").replace(']', "")))
      })
      .map(|(_, source)| source.as_str())
      .collect();
    let mut origins = BTreeSet::new();
    for (path, origin) in &self.0 {
      let text = display(path);
      let normalized = text.replace('[', ".").replace(']', "");
      if overrides
        .iter()
        .any(|(path, _)| path_contains(path, &normalized))
      {
        continue;
      }
      if paths.iter().any(|error_path| {
        text == *error_path
          || text
            .strip_prefix(error_path)
            .is_some_and(|suffix| suffix.starts_with('.') || suffix.starts_with('['))
      }) {
        origins.insert(origin.describe());
      }
    }
    if origins.is_empty() && override_sources.is_empty() {
      // Errors from application validators can lack a configuration path.
      let documents: BTreeSet<_> = self
        .0
        .iter()
        .map(|(_, origin)| origin.document.source.to_string())
        .collect();
      if documents.len() > 1 {
        origins.extend(documents);
      }
    }
    let mut contexts = Vec::new();
    if !override_sources.is_empty() {
      contexts.push(format!(
        "from {}",
        override_sources.into_iter().collect::<Vec<_>>().join(", ")
      ));
    }
    if !origins.is_empty() {
      contexts.push(format!(
        "configuration sources: {}",
        describe_sources(origins)
      ));
    }
    if contexts.is_empty() {
      error
    } else {
      attributed(format!("{message} ({})", contexts.join("; ")))
    }
  }

  pub(crate) fn result<T>(&self, result: Result<T>, overrides: &[(String, String)]) -> Result<T> {
    result.map_err(|error| self.annotate(error, overrides))
  }
}

fn describe_sources(origins: BTreeSet<String>) -> String {
  let count = origins.len();
  let mut sources = origins.into_iter().take(16).collect::<Vec<_>>().join(", ");
  if count > 16 {
    sources.push_str(&format!(", and {} more sources", count - 16));
  }
  sources
}

fn path_contains(parent: &str, child: &str) -> bool {
  parent == child
    || child
      .strip_prefix(parent)
      .is_some_and(|suffix| suffix.starts_with('.') || suffix.starts_with('['))
}

fn related(left: &str, right: &str) -> bool {
  path_contains(left, right) || path_contains(right, left)
}

#[derive(Debug)]
struct AttributedError(String);

impl std::fmt::Display for AttributedError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.write_str(&self.0)
  }
}

impl std::error::Error for AttributedError {}

pub(super) fn attributed(message: String) -> anyhow::Error {
  anyhow::Error::new(AttributedError(message))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn pathless_errors_list_only_multiple_distinct_documents() {
    for (files, expected) in [
      (vec!["rilgate.yml", "rilgate.yml"], "validation failed"),
      (
        vec!["rilgate.yml", "policy.yml"],
        "validation failed (configuration sources: policy.yml, rilgate.yml)",
      ),
    ] {
      let sources = ConfigurationSources(
        files
          .into_iter()
          .map(|file| {
            (
              Vec::new(),
              Origin {
                document: Arc::new(SourceDocument {
                  source: Source::File(file.into()),
                  chain: Vec::new(),
                }),
                local_path: Vec::new(),
              },
            )
          })
          .collect(),
      );
      assert_eq!(
        sources
          .annotate(anyhow::anyhow!("validation failed"), &[])
          .to_string(),
        expected
      );
    }
  }
}
