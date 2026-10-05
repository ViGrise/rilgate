import {readFile} from "node:fs/promises";
import assert from "node:assert/strict";
const path = process.argv[2] || new URL("../target/wasm32-unknown-unknown/release/randomx_wasm.wasm", import.meta.url);
const {instance:{exports}} = await WebAssembly.instantiate(await readFile(path), {});
assert.equal(typeof exports.rx_validate_float, "function", "Build randomx-wasm with --features validation");
for (const seed of [0x4979706f77581234n, 0x124fa223d928cc31n]) {
  assert.equal(exports.rx_validate_float(seed, 100000), 0, `Arithmetic mismatch for ${seed.toString(16)}`);
}
assert.equal(exports.rx_validate_float(1n, 1000001), -1);
console.log("WASM SIMD arithmetic matches the software oracle for IEEE boundaries and 200000 random pairs in all four rounding modes");
