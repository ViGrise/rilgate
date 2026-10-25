use anyhow::{Context, Result, ensure};
use include_dir::{Dir, include_dir};

static RULES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../config/rules");
static POLICIES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../config/policies");

pub(super) fn normalize(path: &str) -> Result<String> {
  ensure!(
    !path.starts_with('/') && !path.contains(['\\', ':']),
    "builtin include expects a relative catalog path"
  );
  let mut components = Vec::new();
  for component in path.split('/') {
    match component {
      "" | "." => {}
      ".." => {
        ensure!(
          components.pop().is_some(),
          "builtin include escapes the catalog root"
        );
      }
      name => components.push(name),
    }
  }
  let path = components.join("/");
  ensure!(
    path.starts_with("rules/") || path.starts_with("policies/"),
    "builtin include must select rules/ or policies/"
  );
  Ok(path)
}

pub(super) fn get(path: &str) -> Result<&'static str> {
  let file = if let Some(path) = path.strip_prefix("rules/") {
    RULES.get_file(path)
  } else if let Some(path) = path.strip_prefix("policies/") {
    POLICIES.get_file(path)
  } else {
    None
  };
  file
    .with_context(|| format!("builtin:{path}: resource does not exist"))?
    .contents_utf8()
    .with_context(|| format!("builtin:{path}: expected UTF-8"))
}
