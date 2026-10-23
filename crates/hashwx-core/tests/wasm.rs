use std::{
  io::Write,
  process::{Command, Stdio},
};

use hashwx_core::HashWx;

#[test]
fn deterministic_modules_validate_in_webassembly_engine() {
  let mut child = Command::new("node")
    .args([
      "--input-type=module",
      "--eval",
      r#"
      import { readFileSync } from 'node:fs';
      const bytes = readFileSync(0);
      for (let offset = 0; offset < bytes.length;) {
        const len = bytes.readUInt32LE(offset);
        offset += 4;
        const module = bytes.subarray(offset, offset + len);
        if (!WebAssembly.validate(module)) throw new Error('invalid module');
        const compiled = new WebAssembly.Module(module);
        if (WebAssembly.Module.imports(compiled).length) throw new Error('unexpected import');
        offset += len;
      }
    "#,
    ])
    .stdin(Stdio::piped())
    .spawn()
    .expect("Node is required for generated WebAssembly validation");
  let mut input = child.stdin.take().unwrap();
  for index in 0..32 {
    let seed = core::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(index));
    let mut instance = HashWx::new(&seed);
    let bytes = instance.wasm_module();
    assert!(bytes.len() < 32768);
    instance.hash(u64::MAX);
    assert_eq!(bytes, instance.wasm_module());
    assert_eq!(bytes, HashWx::new(&seed).wasm_module());
    input
      .write_all(&(bytes.len() as u32).to_le_bytes())
      .unwrap();
    input.write_all(&bytes).unwrap();
  }
  drop(input);
  assert!(child.wait().unwrap().success());
}
