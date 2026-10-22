use std::{
  env, fs,
  path::{Path, PathBuf},
  process::{Command, ExitCode},
};

mod web;

const WASM_TARGET: &str = "wasm32-unknown-unknown";
const OUTPUT: &str = "web/static/js/solvers/randomx/randomx_wasm.wasm";

fn main() -> ExitCode {
  match env::args().nth(1).as_deref() {
    Some("build-wasm") => build_wasm(),
    Some("build-web") => {
      let args: Vec<_> = env::args().skip(2).collect();
      if !args.is_empty() && args != ["--check"] {
        eprintln!("usage: cargo xtask build-web [--check]");
        return ExitCode::FAILURE;
      }
      match web::build(&workspace_root(), args == ["--check"]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
          eprintln!("{error:#}");
          ExitCode::FAILURE
        }
      }
    }
    Some("help" | "h" | "-h" | "--help") => {
      eprintln!("cargo xtask build-wasm\ncargo xtask build-web [--check]");
      ExitCode::SUCCESS
    }
    other => {
      eprintln!("unknown xtask command: {other:?}");
      eprintln!("usage: cargo xtask build-wasm | build-web [--check]");
      ExitCode::FAILURE
    }
  }
}

/// The directory that holds final build artifacts, as Cargo resolves it from
/// the environment and configuration.
fn target_directory(root: &Path) -> Option<PathBuf> {
  let output = Command::new(cargo())
    .current_dir(root)
    .args(["metadata", "--format-version", "1", "--no-deps"])
    .output()
    .ok()?;
  if !output.status.success() {
    return None;
  }
  let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
  metadata["target_directory"].as_str().map(PathBuf::from)
}

/// Builds `randomx-wasm` for wasm32 and stages the artifact where the server
/// embeds it.
///
/// The release profile settings are passed as environment overrides so the
/// native server build keeps its own panic and codegen behaviour.
fn build_wasm() -> ExitCode {
  let root = workspace_root();
  let output = root.join(OUTPUT);

  let status = Command::new(cargo())
    .current_dir(&root)
    .env("CARGO_PROFILE_RELEASE_LTO", "fat")
    .env("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "1")
    .env("CARGO_PROFILE_RELEASE_OPT_LEVEL", "3")
    .env("CARGO_PROFILE_RELEASE_STRIP", "symbols")
    .args([
      "rustc",
      "--package",
      "randomx-wasm",
      "--release",
      "--target",
      WASM_TARGET,
      "--",
      "-C",
      "link-arg=--export-table",
      "-C",
      "link-arg=--growable-table",
    ])
    .status();

  let Ok(status) = status else {
    eprintln!("failed to run cargo");
    return ExitCode::FAILURE;
  };

  if !status.success() {
    return ExitCode::from(1);
  }

  let Some(target_dir) = target_directory(&root) else {
    eprintln!("failed to read the target directory from cargo metadata");
    return ExitCode::FAILURE;
  };
  let built = target_dir
    .join(WASM_TARGET)
    .join("release")
    .join("randomx_wasm.wasm");

  if let Some(parent) = output.parent()
    && !parent.exists()
    && let Err(error) = fs::create_dir_all(parent)
  {
    eprintln!("failed to create {}: {error}", parent.display());
    return ExitCode::FAILURE;
  }

  if let Err(error) = fs::copy(&built, &output) {
    eprintln!(
      "failed to copy {} to {}: {error}",
      built.display(),
      output.display()
    );
    return ExitCode::FAILURE;
  }

  let size = fs::metadata(&output).map(|meta| meta.len()).unwrap_or(0);
  println!("wrote {} ({size} bytes)", output.display());

  ExitCode::SUCCESS
}

fn cargo() -> PathBuf {
  env::var_os("CARGO").map_or_else(|| PathBuf::from("cargo"), PathBuf::from)
}

fn workspace_root() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("../..")
    .canonicalize()
    .expect("workspace root")
}
