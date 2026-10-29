use std::{env, path::Path, process::Command};

fn main() {
  println!("cargo:rerun-if-env-changed=RILGATE_BUILD_COMMIT");
  if Path::new("../../.git/HEAD").is_file() {
    println!("cargo:rerun-if-changed=../../.git/HEAD");
  }

  let commit = env::var("RILGATE_BUILD_COMMIT")
    .ok()
    .filter(|commit| !commit.trim().is_empty())
    .or_else(|| {
      let commit = git(&["rev-parse", "--short=7", "HEAD"])?;
      for reference in [
        Some("HEAD".to_owned()),
        git(&["symbolic-ref", "-q", "HEAD"]),
        Some("packed-refs".to_owned()),
      ]
      .into_iter()
      .flatten()
      {
        if let Some(path) = git(&["rev-parse", "--git-path", &reference]) {
          let path = Path::new(&path);
          if path.is_file() {
            println!("cargo:rerun-if-changed={}", path.display());
          } else if reference.starts_with("refs/")
            && let Some(parent) = path.parent().filter(|parent| parent.is_dir())
          {
            println!("cargo:rerun-if-changed={}", parent.display());
          }
        }
      }
      Some(commit)
    });
  let version = env::var("CARGO_PKG_VERSION").unwrap();
  let version = match commit.filter(|commit| !commit.trim().is_empty()) {
    Some(commit) => format!(
      "{version} ({})",
      commit.trim().chars().take(7).collect::<String>()
    ),
    None => version,
  };
  println!("cargo:rustc-env=RILGATE_VERSION={version}");
}

fn git(args: &[&str]) -> Option<String> {
  let output = Command::new("git").args(args).output().ok()?;
  output
    .status
    .success()
    .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
