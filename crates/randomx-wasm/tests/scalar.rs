use std::path::Path;

#[test]
fn scalar_artifact_runs_without_simd() {
  let path = Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("../../web/static/js/solvers/randomx/randomx_wasm_scalar.wasm");
  let artifact = std::fs::read(path).expect("run cargo xtask build-wasm before tests");
  let mut features = wasmparser::WasmFeatures::default();
  features.remove(wasmparser::WasmFeatures::SIMD | wasmparser::WasmFeatures::RELAXED_SIMD);
  wasmparser::Validator::new_with_features(features)
    .validate_all(&artifact)
    .expect("the scalar artifact must run on engines without SIMD");
}
