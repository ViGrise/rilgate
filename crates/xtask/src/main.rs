use std::{
  env, fs,
  path::{Path, PathBuf},
  process::{Command, ExitCode},
};

mod web;

const WASM_TARGET: &str = "wasm32-unknown-unknown";
const ARTIFACTS: [(&str, &str, bool); 2] = [
  (
    "randomx-wasm",
    "web/static/js/solvers/randomx/randomx_wasm.wasm",
    false,
  ),
  (
    "hashwx-wasm",
    "web/static/js/solvers/hashwx/hashwx_wasm.wasm",
    true,
  ),
];

fn main() -> ExitCode {
  match env::args().nth(1).as_deref() {
    Some("build-wasm") => build_wasm(),
    Some("build-web") => match web::build(&workspace_root()) {
      Ok(()) => ExitCode::SUCCESS,
      Err(error) => {
        eprintln!("{error:#}");
        ExitCode::FAILURE
      }
    },
    Some("help" | "h" | "-h" | "--help") => {
      eprintln!("cargo xtask build-wasm\ncargo xtask build-web");
      ExitCode::SUCCESS
    }
    other => {
      eprintln!("unknown xtask command: {other:?}");
      eprintln!("usage: cargo xtask build-wasm | build-web");
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

/// Builds and stages browser WASM artifacts.
///
/// The release profile settings are passed as environment overrides so the
/// native server build keeps its own panic and codegen behaviour.
fn build_wasm() -> ExitCode {
  let root = workspace_root();
  for (package, output, mvp_only) in ARTIFACTS {
    if !build_artifact(&root, package, output, mvp_only) {
      return ExitCode::FAILURE;
    }
  }
  ExitCode::SUCCESS
}

fn build_artifact(root: &Path, package: &str, output: &str, mvp_only: bool) -> bool {
  let output = root.join(output);
  let mut command = Command::new(cargo());
  if mvp_only {
    command.env("CARGO_ENCODED_RUSTFLAGS", "-C\x1ftarget-cpu=mvp\x1f-C\x1ftarget-feature=-simd128,-bulk-memory,-reference-types,-multivalue,-nontrapping-fptoint,-sign-ext");
  }

  let status = command
    .current_dir(root)
    .env("CARGO_PROFILE_RELEASE_LTO", "fat")
    .env("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "1")
    .env("CARGO_PROFILE_RELEASE_OPT_LEVEL", "3")
    .env("CARGO_PROFILE_RELEASE_STRIP", "symbols")
    .args([
      "rustc",
      "--package",
      package,
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
    return false;
  };

  if !status.success() {
    return false;
  }

  let Some(target_dir) = target_directory(root) else {
    eprintln!("failed to read the target directory from cargo metadata");
    return false;
  };
  let built = target_dir
    .join(WASM_TARGET)
    .join("release")
    .join(format!("{}.wasm", package.replace('-', "_")));

  if let Some(parent) = output.parent()
    && !parent.exists()
    && let Err(error) = fs::create_dir_all(parent)
  {
    eprintln!("failed to create {}: {error}", parent.display());
    return false;
  }

  if let Err(error) = fs::copy(&built, &output) {
    eprintln!(
      "failed to copy {} to {}: {error}",
      built.display(),
      output.display()
    );
    return false;
  }

  let size = fs::metadata(&output).map(|meta| meta.len()).unwrap_or(0);
  println!("wrote {} ({size} bytes)", output.display());

  true
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
