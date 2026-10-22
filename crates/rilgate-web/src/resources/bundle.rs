use crate::{
  assets::{AssetIndex, AssetMetadata},
  resources::embedded::EmbeddedWeb,
};
use anyhow::{Context, Result, ensure};
use include_dir::{Dir, include_dir};
use serde::Deserialize;
use sha2::{Digest, Sha256};

static GENERATED: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/generated");
pub const BUNDLE_PATH: &str = "js/challenge.bundle.js";

#[derive(Deserialize)]
struct Manifest {
  startup: Vec<Member>,
  worker: String,
}

#[derive(Deserialize)]
struct Member {
  source: String,
  output: String,
  sha256: String,
}

pub fn assemble(assets: &mut AssetIndex) -> Result<()> {
  let manifest: Manifest = serde_json::from_slice(
    GENERATED
      .get_file("manifest.json")
      .context("Missing generated web manifest")?
      .contents(),
  )?;
  let mut bundle = Vec::new();
  let mut worker = None;
  for member in &manifest.startup {
    let source = EmbeddedWeb::get(&member.source).context("Missing embedded bundle source")?;
    ensure!(
      hex::encode(Sha256::digest(source)) == member.sha256,
      "Stale generated web source: {}",
      member.source
    );
    let path = member
      .source
      .strip_prefix("static/")
      .context("Invalid bundle source path")?;
    let asset = assets.get(path).context("Missing bundle source asset")?;
    let bytes = if asset.etag.trim_matches('"') == member.sha256 {
      GENERATED
        .get_file(&member.output)
        .context("Missing generated web member")?
        .contents()
        .to_vec()
    } else {
      asset.bytes()?
    };
    bundle.extend_from_slice(&bytes);
    bundle.extend_from_slice(b"\n;\n");
    if member.output == manifest.worker {
      worker = Some((path.to_string(), bytes));
    }
  }
  let (path, bytes) = worker.context("Missing generated fast worker")?;
  assets.insert(path.clone(), AssetMetadata::memory(&path, bytes));
  assets.insert(
    BUNDLE_PATH.into(),
    AssetMetadata::memory(BUNDLE_PATH, bundle),
  );
  Ok(())
}
