use super::*;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use std::sync::Arc;

pub trait SetDefinition {
  type Index: FeedIndex;
  /// YAML key of the inline values, such as `cidrs`.
  const STATIC_FIELD: &'static str;
  fn static_index(&self, path: &str) -> Option<Result<Self::Index>>;
  fn url(&self) -> Option<&str>;
  fn format(&self) -> Option<&FeedFormat>;
  fn refresh(&self) -> Option<Duration>;
  fn max_stale(&self) -> Option<Duration>;
  /// Rejects a format that cannot produce values of this set kind.
  fn check_format(format: &FeedFormat, path: &str) -> Result<()>;
}

/// The compiled registry of named feed sets referenced by policy conditions.
#[derive(Debug)]
pub struct FeedSets<I: FeedIndex> {
  sets: BTreeMap<String, Arc<FeedSet<I>>>,
}
impl<I: FeedIndex> Default for FeedSets<I> {
  fn default() -> Self {
    Self {
      sets: BTreeMap::new(),
    }
  }
}
impl<I: FeedIndex> Clone for FeedSets<I> {
  fn clone(&self) -> Self {
    Self {
      sets: self.sets.clone(),
    }
  }
}
impl<I: FeedIndex> FeedSets<I> {
  pub fn compile_registry<D: SetDefinition<Index = I>>(
    registry: &str,
    definitions: &BTreeMap<String, D>,
    previous: Option<&Self>,
  ) -> Result<Self> {
    ensure!(
      definitions.len() <= MAX_SETS_PER_REGISTRY,
      "{registry}: at most {MAX_SETS_PER_REGISTRY} sets are supported"
    );
    let mut sets = BTreeMap::new();
    for (name, definition) in definitions {
      let path = format!("{registry}.{name}");
      ensure!(
        valid_name(name),
        "{path}: name must contain 1 to {MAX_SET_NAME_BYTES} bytes of a-z, 0-9, - and _"
      );
      let index = definition.static_index(&path);
      ensure!(
        index.is_some() ^ definition.url().is_some(),
        "{path}: expected exactly one of {}, url",
        D::STATIC_FIELD
      );
      let set = if let Some(index) = index {
        ensure!(
          definition.format().is_none(),
          "{path}.format: only valid with url"
        );
        ensure!(
          definition.refresh().is_none(),
          "{path}.refresh: only valid with url"
        );
        ensure!(
          definition.max_stale().is_none(),
          "{path}.max_stale: only valid with url"
        );
        FeedSet::static_set(name.clone(), index?)
      } else {
        let url = definition.url().expect("url present");
        let uri = url
          .parse::<http::Uri>()
          .with_context(|| format!("{path}.url: expected an absolute http or https URL"))?;
        ensure!(
          matches!(uri.scheme_str(), Some("http" | "https")) && uri.host().is_some(),
          "{path}.url: expected an absolute http or https URL"
        );
        let format = definition
          .format()
          .with_context(|| format!("{path}.format: required with url"))?;
        let format_path = format!("{path}.format");
        format.validate(&format_path)?;
        D::check_format(format, &format_path)?;
        let refresh = definition.refresh().unwrap_or(DEFAULT_REFRESH);
        ensure!(
          (MIN_REFRESH..=MAX_REFRESH).contains(&refresh),
          "{path}.refresh: expected 1h to 7d"
        );
        let max_stale = definition.max_stale().unwrap_or(DEFAULT_MAX_STALE);
        ensure!(
          max_stale >= refresh && max_stale <= MAX_MAX_STALE,
          "{path}.max_stale: expected refresh to 30d"
        );
        let set = FeedSet::remote_set(
          name.clone(),
          RemoteFeed {
            url: url.to_string(),
            format: format.clone(),
            refresh,
            max_stale,
          },
        );
        match previous.and_then(|previous| previous.get(name)) {
          Some(earlier) => set.adopt_state_of(earlier),
          None => set,
        }
      };
      sets.insert(name.clone(), Arc::new(set));
    }
    Ok(Self { sets })
  }
  pub fn get(&self, name: &str) -> Option<&Arc<FeedSet<I>>> {
    self.sets.get(name)
  }
  pub fn iter(&self) -> impl Iterator<Item = &Arc<FeedSet<I>>> {
    self.sets.values()
  }
  pub fn all_remote_loaded(&self) -> bool {
    self
      .iter()
      .filter(|set| set.remote().is_some())
      .all(|set| set.status().loaded)
  }
}
fn valid_name(name: &str) -> bool {
  let bytes = name.as_bytes();
  !bytes.is_empty()
    && bytes.len() <= MAX_SET_NAME_BYTES
    && matches!(bytes[0], b'a'..=b'z' | b'0'..=b'9')
    && bytes
      .iter()
      .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_'))
}
