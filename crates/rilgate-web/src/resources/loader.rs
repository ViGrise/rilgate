use anyhow::{Context, Result, ensure};
use std::{
  collections::BTreeMap,
  fs,
  path::{Component, Path, PathBuf},
};

pub fn logical_path(name: &str) -> Result<()> {
  ensure!(
    !name.is_empty()
      && !name.contains(['\\', '\0', ':'])
      && !name.starts_with('/')
      && name
        .split('/')
        .all(|p| !p.is_empty() && p != "." && p != "..")
      && Path::new(name)
        .components()
        .all(|c| matches!(c, Component::Normal(_))),
    "Invalid logical web path: {name}"
  );
  Ok(())
}

pub fn ignored(path: &Path) -> bool {
  path
    .components()
    .filter_map(|c| c.as_os_str().to_str())
    .any(|name| {
      name.starts_with('.')
        || name == "Thumbs.db"
        || name.ends_with(['~', '#'])
        || name.ends_with(".swp")
        || name.ends_with(".tmp")
    })
}

pub fn canonical_root(root: &Path) -> Result<PathBuf> {
  let root =
    fs::canonicalize(root).with_context(|| format!("Resolving web root {}", root.display()))?;
  ensure!(
    root.is_dir(),
    "Web root is not a directory: {}",
    root.display()
  );
  Ok(root)
}

pub fn files(root: &Path) -> Result<BTreeMap<String, PathBuf>> {
  let root = canonical_root(root)?;
  fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<String, PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("Reading {}", dir.display()))? {
      let entry = entry?;
      let path = entry.path();
      let relative = path.strip_prefix(root)?;
      if ignored(relative) {
        continue;
      }
      let kind = fs::symlink_metadata(&path)?.file_type();
      ensure!(
        !kind.is_symlink(),
        "Symlinks are disabled in web.root: {}",
        relative.display()
      );
      let canonical = fs::canonicalize(&path)?;
      ensure!(
        canonical.starts_with(root),
        "Web path escaped its root: {}",
        relative.display()
      );
      if kind.is_dir() {
        walk(root, &path, files)?;
      } else if kind.is_file() {
        let name = relative
          .to_str()
          .context("Web paths must be UTF-8")?
          .replace('\\', "/");
        logical_path(&name)?;
        files.insert(name, canonical);
      } else {
        anyhow::bail!("Unsupported web file type: {}", relative.display());
      }
    }
    Ok(())
  }
  let mut result = BTreeMap::new();
  walk(&root, &root, &mut result)?;
  Ok(result)
}

pub fn open_checked(root: &Path, path: &Path) -> Result<fs::File> {
  let relative = path
    .strip_prefix(root)
    .context("Web file is outside its root")?;
  let dir = cap_std::fs::Dir::open_ambient_dir(root, cap_std::ambient_authority())?;
  Ok(
    dir
      .open(relative)
      .with_context(|| format!("Opening web file {}", relative.display()))?
      .into_std(),
  )
}

pub fn read_checked(root: &Path, path: &Path) -> Result<Vec<u8>> {
  use std::io::Read;
  let mut bytes = Vec::new();
  open_checked(root, path)?
    .read_to_end(&mut bytes)
    .with_context(|| format!("Reading {}", path.display()))?;
  Ok(bytes)
}
