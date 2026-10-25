use super::{
  builtin,
  sources::{ConfigPath, ConfigurationSources, Origin, Segment, Source, SourceDocument, display},
};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_yaml_ng::Value;
use std::{collections::HashMap, fs::File, io::Read, path::Path, sync::Arc};

pub(super) const MAX_BYTES: usize = 1_048_576;
const MAX_SOURCE_READS: usize = 256;
const MAX_INCLUDE_DEPTH: usize = 16;
pub(super) const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 65_536;

#[derive(Default)]
pub(super) struct Resolver {
  bytes: usize,
  reads: usize,
  nodes: usize,
  active: Vec<Source>,
}

// Origins follow nodes until sequence insertion assigns their final indices.
struct Node {
  origin: Origin,
  contributors: Vec<Origin>,
  value: Contents,
}

enum Contents {
  Scalar(Value),
  Mapping(Vec<(String, Node)>),
  Sequence(Vec<Node>),
}

impl Node {
  fn overlay(&mut self, higher: Node) {
    match (&mut self.value, higher.value) {
      (Contents::Mapping(lower), Contents::Mapping(higher_entries)) => {
        merge_entries(lower, higher_entries);
      }
      (lower, higher_value) => *lower = higher_value,
    }
    self.origin = higher.origin;
    self.contributors = higher.contributors;
  }

  fn finish(self, path: &mut ConfigPath, sources: &mut ConfigurationSources) -> Value {
    if self.origin.document.source != Source::Inline {
      sources.0.push((path.clone(), self.origin));
    }
    sources.0.extend(
      self
        .contributors
        .into_iter()
        .map(|origin| (path.clone(), origin)),
    );
    match self.value {
      Contents::Scalar(value) => value,
      Contents::Mapping(entries) => {
        let mut map = serde_yaml_ng::Mapping::new();
        for (key, node) in entries {
          path.push(Segment::Key(key.clone()));
          map.insert(Value::String(key), node.finish(path, sources));
          path.pop();
        }
        Value::Mapping(map)
      }
      Contents::Sequence(items) => Value::Sequence(
        items
          .into_iter()
          .enumerate()
          .map(|(index, node)| {
            path.push(Segment::Index(index));
            let value = node.finish(path, sources);
            path.pop();
            value
          })
          .collect(),
      ),
    }
  }
}

fn merge_entries(lower: &mut Vec<(String, Node)>, higher: Vec<(String, Node)>) {
  let mut positions: HashMap<String, usize> = lower
    .iter()
    .enumerate()
    .map(|(index, (key, _))| (key.clone(), index))
    .collect();
  for (key, node) in higher {
    if let Some(&index) = positions.get(&key) {
      lower[index].1.overlay(node);
    } else {
      positions.insert(key.clone(), lower.len());
      lower.push((key, node));
    }
  }
}

impl Resolver {
  pub fn read_optional(&mut self, path: &Path) -> Result<Option<String>> {
    let file = match File::open(path) {
      Ok(file) => file,
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
      Err(error) => {
        return Err(error).with_context(|| format!("configuration file {}", path.display()));
      }
    };
    ensure!(
      self.reads < MAX_SOURCE_READS,
      "configuration file {}: configuration exceeds {MAX_SOURCE_READS} source reads",
      path.display()
    );
    self.reads += 1;
    let remaining = MAX_BYTES.saturating_sub(self.bytes);
    let mut bytes = Vec::new();
    file
      .take((remaining + 1) as u64)
      .read_to_end(&mut bytes)
      .with_context(|| format!("configuration file {}", path.display()))?;
    ensure!(
      bytes.len() <= remaining,
      "configuration file {}: configuration must not exceed 1 MiB across all sources",
      path.display()
    );
    self.bytes += bytes.len();
    String::from_utf8(bytes)
      .map(Some)
      .with_context(|| format!("configuration file {}: expected UTF-8", path.display()))
  }

  pub fn read(&mut self, path: &Path) -> Result<String> {
    self
      .read_optional(path)?
      .with_context(|| format!("configuration file {}: file does not exist", path.display()))
  }

  pub fn resolve(&mut self, text: &str, path: &Path) -> Result<(Value, ConfigurationSources)> {
    let node = self.document(text, Source::File(path.to_path_buf()), Vec::new(), 0, false)?;
    let mut sources = ConfigurationSources::default();
    let value = node.finish(&mut Vec::new(), &mut sources);
    Ok((value, sources))
  }

  pub fn resolve_inline(&mut self, text: &str) -> Result<(Value, ConfigurationSources)> {
    self.charge(text.len(), &Source::Inline)?;
    let node = self.document(text, Source::Inline, Vec::new(), 0, false)?;
    let mut sources = ConfigurationSources::default();
    let value = node.finish(&mut Vec::new(), &mut sources);
    Ok((value, sources))
  }

  fn charge(&mut self, bytes: usize, source: &Source) -> Result<()> {
    ensure!(
      self.reads < MAX_SOURCE_READS,
      "configuration source {source}: configuration exceeds {MAX_SOURCE_READS} source reads"
    );
    ensure!(
      bytes <= MAX_BYTES.saturating_sub(self.bytes),
      "configuration source {source}: configuration must not exceed 1 MiB across all sources"
    );
    self.reads += 1;
    self.bytes += bytes;
    Ok(())
  }

  fn document(
    &mut self,
    text: &str,
    source: Source,
    mut chain: Vec<Source>,
    depth: usize,
    fragment: bool,
  ) -> Result<Node> {
    ensure!(
      self.active.len() < MAX_INCLUDE_DEPTH,
      "include depth must not exceed {MAX_INCLUDE_DEPTH} sources"
    );
    let identity = match &source {
      Source::File(path) => Source::File(
        path
          .canonicalize()
          .with_context(|| format!("configuration file {}", path.display()))?,
      ),
      other => other.clone(),
    };
    ensure!(
      !self.active.contains(&identity),
      "include cycle at {source}"
    );
    chain.push(source.clone());
    let source = Arc::new(SourceDocument { source, chain });
    self.active.push(identity);
    let result = (|| {
      let mut documents = serde_yaml_ng::Deserializer::from_str(text);
      let document = documents.next();
      let value = match document {
        Some(document) => Value::deserialize(document).context("configuration YAML")?,
        None if fragment => anyhow::bail!("included file must contain a YAML document"),
        None => Value::Mapping(Default::default()),
      };
      ensure!(
        documents.next().is_none(),
        "configuration file must contain exactly one YAML document"
      );
      if fragment && value.is_null() {
        ensure!(
          has_document_value(text),
          "included file must contain a YAML document"
        );
      }
      let value = if !fragment && text.trim().is_empty() {
        Value::Mapping(Default::default())
      } else {
        value
      };
      self.node(value, source.clone(), Vec::new(), depth)
    })()
    .with_context(|| {
      format!(
        "configuration source {}",
        Origin {
          document: source,
          local_path: Vec::new()
        }
        .describe()
      )
    });
    self.active.pop();
    result
  }

  fn node(
    &mut self,
    value: Value,
    document: Arc<SourceDocument>,
    local_path: ConfigPath,
    depth: usize,
  ) -> Result<Node> {
    ensure!(
      depth <= MAX_DEPTH,
      "{}: YAML nesting must not exceed {MAX_DEPTH} levels",
      display(&local_path)
    );
    ensure!(
      self.nodes < MAX_NODES,
      "expanded configuration exceeds {MAX_NODES} nodes"
    );
    self.nodes += 1;
    let origin = Origin {
      document: document.clone(),
      local_path: local_path.clone(),
    };
    let mut contributors = Vec::new();
    let value = match value {
      Value::Tagged(tagged) => {
        ensure!(
          tagged.tag == "include",
          "{}: YAML tag {} is unsupported",
          display(&local_path),
          tagged.tag
        );
        let path = match tagged.value {
          Value::String(path) if !path.trim().is_empty() => path,
          _ => anyhow::bail!(
            "{}: !include expects a nonempty literal path string",
            display(&local_path)
          ),
        };
        ensure!(
          !path.contains("${"),
          "{}: !include paths do not support environment interpolation",
          display(&local_path)
        );
        return (|| {
          let target = match path.strip_prefix("builtin:") {
            Some(path) => Source::Builtin(builtin::normalize(path)?),
            None => match &document.source {
              Source::File(parent) => {
                Source::File(parent.parent().unwrap_or(Path::new(".")).join(path))
              }
              Source::Builtin(parent) => {
                ensure!(
                  !path.starts_with('/'),
                  "builtin documents require relative catalog paths"
                );
                let directory = parent
                  .rsplit_once('/')
                  .map_or("", |(directory, _)| directory);
                Source::Builtin(builtin::normalize(&format!("{directory}/{path}"))?)
              }
              Source::Inline => {
                anyhow::bail!("!include requires configuration file context for local paths")
              }
            },
          };
          let text = match &target {
            Source::File(path) => self.read(path)?,
            Source::Builtin(path) => {
              let text = builtin::get(path)?;
              self.charge(text.len(), &target)?;
              text.to_string()
            }
            Source::Inline => unreachable!(),
          };
          self.document(&text, target, document.chain.clone(), depth, true)
        })()
        .with_context(|| format!("!include at {}", origin.describe()));
      }
      Value::Mapping(entries) => {
        let mut children = Vec::with_capacity(entries.len());
        let mut bases = Vec::new();
        for (key, value) in entries {
          let key = match key {
            Value::String(key) => key,
            _ => anyhow::bail!(
              "{}: configuration keys must be untagged strings",
              origin.describe()
            ),
          };
          let mut path = local_path.clone();
          path.push(Segment::Key(key.clone()));
          let merge_origin = Origin {
            document: document.clone(),
            local_path: path.clone(),
          };
          let child = self.node(value, document.clone(), path, depth + 1)?;
          if key == "<<" {
            let items = match child.value {
              Contents::Mapping(entries) => {
                merge_entries(&mut bases, entries);
                continue;
              }
              Contents::Sequence(items) => items,
              _ => anyhow::bail!(
                "{}: `<<` must be a mapping or a list of mappings (source {})",
                merge_origin.describe(),
                child.origin.describe()
              ),
            };
            for item in items {
              match item.value {
                Contents::Mapping(entries) => merge_entries(&mut bases, entries),
                _ => anyhow::bail!(
                  "{}: `<<` must be a mapping or a list of mappings (source {})",
                  merge_origin.describe(),
                  item.origin.describe()
                ),
              }
            }
          } else {
            children.push((key, child));
          }
        }
        merge_entries(&mut bases, children);
        Contents::Mapping(bases)
      }
      Value::Sequence(values) => {
        let mut children = Vec::new();
        for (index, value) in values.into_iter().enumerate() {
          let splice = matches!(&value, Value::Tagged(tagged) if tagged.tag == "include");
          let mut path = local_path.clone();
          path.push(Segment::Index(index));
          let child = self.node(value, document.clone(), path, depth + 1)?;
          match child {
            Node {
              origin,
              contributors: included_contributors,
              value: Contents::Sequence(items),
            } if splice => {
              // Sequence origins contribute to the container even when they insert no items.
              contributors.push(origin);
              contributors.extend(included_contributors);
              children.extend(items);
            }
            child => children.push(child),
          }
        }
        Contents::Sequence(children)
      }
      scalar => Contents::Scalar(scalar),
    };
    Ok(Node {
      origin,
      contributors,
      value,
    })
  }
}

// The value tree represents omitted document content and explicit null as Null.
fn has_document_value(text: &str) -> bool {
  text.lines().any(|line| {
    let line = line.trim().trim_start_matches('\u{feff}').trim_start();
    if line.is_empty() || line.starts_with('#') || line.starts_with('%') {
      return false;
    }
    for marker in ["---", "..."] {
      if let Some(rest) = line.strip_prefix(marker)
        && (rest.is_empty() || rest.starts_with(char::is_whitespace))
      {
        let rest = rest.trim_start();
        return !rest.is_empty() && !rest.starts_with('#');
      }
    }
    true
  })
}
