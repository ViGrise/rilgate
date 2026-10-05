use crate::resources::seed;
use anyhow::Result;
use std::path::Path;
pub fn init(root: &Path) -> Result<()> {
  let count = seed::initialize(root)?;
  println!(
    "Initialized {} with {count} embedded web files",
    root.display()
  );
  Ok(())
}
