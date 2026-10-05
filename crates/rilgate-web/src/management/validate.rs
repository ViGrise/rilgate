use crate::{WebConfig, resources::loader, runtime::snapshot::WebSnapshot};
use anyhow::Result;
use std::path::Path;
pub fn validate(config: &WebConfig, root: &Path) -> Result<()> {
  let root = loader::canonical_root(root)?;
  let mut config = config.clone();
  config.root = Some(root);
  let snapshot = WebSnapshot::build(&config, 1, true)?;
  println!(
    "Valid web bundle: {} templates, {} locales, {} assets",
    snapshot.templates.templates().count(),
    snapshot.translations.locales.len(),
    snapshot.assets.len()
  );
  Ok(())
}
