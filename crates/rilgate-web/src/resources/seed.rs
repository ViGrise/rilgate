use crate::resources::{embedded::EmbeddedWeb, loader};
use anyhow::{Context, Result, ensure};
use std::{fs, io::Write, path::Path};

pub fn has_user_files(root: &Path) -> Result<bool> {
  for entry in fs::read_dir(root)? {
    let entry = entry?;
    let path = entry.path();
    let kind = fs::symlink_metadata(&path)?.file_type();
    ensure!(
      !kind.is_symlink(),
      "Symlinks are disabled in web.root: {}",
      path.display()
    );
    if kind.is_dir() {
      if has_user_files(&path)? {
        return Ok(true);
      }
    } else if !matches!(
      entry.file_name().to_str(),
      Some(".DS_Store" | "Thumbs.db" | ".rilgate-web-version")
    ) {
      return Ok(true);
    }
  }
  Ok(false)
}

pub fn initialize(root: &Path) -> Result<usize> {
  fs::create_dir_all(root).with_context(|| format!("Creating web root {}", root.display()))?;
  let root = loader::canonical_root(root)?;
  ensure!(
    !has_user_files(&root)?,
    "Web root already contains operator files: {}",
    root.display()
  );
  let dir = cap_std::fs::Dir::open_ambient_dir(&root, cap_std::ambient_authority())?;
  dir.create_dir_all("static/images")?;
  let mut count = 0;
  for (name, bytes) in EmbeddedWeb::entries().into_iter().chain(std::iter::once((
    ".rilgate-web-version".into(),
    env!("CARGO_PKG_VERSION").as_bytes(),
  ))) {
    let target = Path::new(&name);
    let parent = target.parent().context("Web file has no parent")?;
    if !parent.as_os_str().is_empty() {
      dir.create_dir_all(parent)?;
    }
    let temporary = tempfile::Builder::new()
      .prefix(".rilgate-seed-")
      .tempfile()?;
    let temporary_name = parent.join(
      temporary
        .path()
        .file_name()
        .context("Temporary file name")?,
    );
    let mut options = cap_std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut output = dir.open_with(&temporary_name, &options)?;
    let result = (|| -> std::io::Result<()> {
      output.write_all(bytes)?;
      output.sync_all()?;
      // A hard link publishes a complete file with exclusive destination semantics.
      dir.hard_link(&temporary_name, &dir, target)
    })();
    drop(output);
    let cleanup = dir.remove_file(&temporary_name);
    match result {
      Ok(_) => count += 1,
      Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
      Err(error) => return Err(error).with_context(|| format!("Seeding {}", target.display())),
    }
    cleanup?;
  }
  Ok(count)
}

pub fn prepare(root: &Path, seed: bool) -> Result<()> {
  if !root.exists()
    && let Err(error) = fs::create_dir_all(root)
  {
    tracing::warn!(root = %root.display(), %error, "Web root unavailable; embedded resources remain active");
    return Ok(());
  }
  let canonical = loader::canonical_root(root)?;
  if seed && !has_user_files(&canonical)? {
    match initialize(&canonical) {
      Ok(count) => tracing::info!(count, root = %canonical.display(), "Web root initialized"),
      Err(error) => tracing::warn!(%error, "Unable to seed web root; existing files are preserved"),
    }
  }
  Ok(())
}
