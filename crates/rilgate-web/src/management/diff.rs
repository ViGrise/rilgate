use crate::resources::{embedded::EmbeddedWeb, loader};
use anyhow::Result;
use std::path::Path;
pub fn diff(root: &Path) -> Result<()> {
  let root = loader::canonical_root(root)?;
  let files = loader::files(&root)?;
  let mut embedded = EmbeddedWeb::entries();
  for (name, path) in files {
    let custom = loader::read_checked(&root, &path)?;
    match embedded.remove(&name) {
      Some(bytes) if custom == bytes => {}
      Some(_) => println!("modified {name}"),
      None => println!("custom {name}"),
    }
  }
  for name in embedded.keys() {
    println!("embedded fallback {name}");
  }
  Ok(())
}
