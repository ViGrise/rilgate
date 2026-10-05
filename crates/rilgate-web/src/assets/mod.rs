mod metadata;
mod response;
pub use metadata::{AssetMetadata, AssetSource};
use std::collections::BTreeMap;

pub type AssetIndex = BTreeMap<String, AssetMetadata>;

pub fn canonical_asset(name: &str) -> &str {
  match name {
    "challenge.js" => "js/challenge.js",
    "hash-worker.js" | "js/hash-worker.js" => "js/solvers/fast/worker.js",
    "randomx/solver.js" => "js/solvers/randomx/solver.js",
    "randomx/randomx-worker.js" => "js/solvers/randomx/randomx-worker.js",
    "randomx/randomx_wasm.wasm" => "js/solvers/randomx/randomx_wasm.wasm",
    _ => name,
  }
}
