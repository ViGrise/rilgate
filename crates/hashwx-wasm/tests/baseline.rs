use std::path::Path;

#[test]
fn browser_artifact_uses_only_webassembly_1_0() {
  let path = Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("../../web/static/js/solvers/hashwx/hashwx_wasm.wasm");
  let artifact = std::fs::read(path).expect("run cargo xtask build-wasm before tests");
  wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::WASM1)
    .validate_all(&artifact)
    .expect("the browser interpreter must run on WebAssembly 1.0 engines");
  for payload in wasmparser::Parser::new(0).parse_all(&artifact) {
    if let wasmparser::Payload::ImportSection(imports) = payload.unwrap() {
      assert_eq!(
        imports.count(),
        0,
        "exports must execute without host callbacks"
      );
    }
  }
}
