import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";

async function installDatasetKernel(exports, handle, slot) {
  const bytes = new Uint8Array(exports.memory.buffer, exports.rx_dataset_module_ptr(handle), exports.rx_dataset_module_len(handle)).slice();
  assert.ok(WebAssembly.validate(bytes));
  const { instance } = await WebAssembly.instantiate(bytes, { env: { memory: exports.memory } });
  const table = exports.__indirect_function_table;
  slot ??= table.grow(3);
  table.set(slot, instance.exports.dataset_item);
  assert.equal(exports.rx_set_dataset_function(handle, slot), 0);
  table.set(slot + 1, instance.exports.dataset_pair);
  assert.equal(exports.rx_set_dataset_pair_function(handle, slot + 1), 0);
  table.set(slot + 2, instance.exports.dataset_triple);
  assert.equal(exports.rx_set_dataset_triple_function(handle, slot + 2), 0);
  return { slot, triple: instance.exports.dataset_triple, kernel: instance.exports.dataset_item, pair: instance.exports.dataset_pair };
}

for (const moduleName of ["randomx_wasm.wasm", "randomx_wasm_scalar.wasm"]) {
test(`${moduleName} reproduces reference and nonce vectors with a reused cache`, async () => {
  const bytes = await readFile(new URL(`../web/static/js/solvers/randomx/${moduleName}`, import.meta.url));
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const { memory, rx_create, rx_hash, rx_destroy } = instance.exports;
  const seed = new TextEncoder().encode("test key 000");
  const seedPointer = memory.buffer.byteLength;
  memory.grow(1);
  new Uint8Array(memory.buffer, seedPointer, seed.length).set(seed);
  const handle = rx_create(seedPointer, seed.length);
  assert.ok(handle >= 0);
  const { slot } = await installDatasetKernel(instance.exports, handle);
  try {
    const input = new TextEncoder().encode("This is a test");
    const inputPointer = memory.buffer.byteLength;
    memory.grow(1);
    const outputPointer = inputPointer + 256;
    new Uint8Array(memory.buffer, inputPointer, input.length).set(input);
    for (let iteration = 0; iteration < 2; iteration += 1) {
      assert.equal(rx_hash(handle, inputPointer, input.length, outputPointer), 0);
      assert.equal(
        Buffer.from(memory.buffer, outputPointer, 32).toString("hex"),
        "639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f",
      );
    }
    const candidate = Uint8Array.from({ length: 87 }, (_, index) => (index * 17 + 3) & 255);
    const expectedHashes = [];
    const checksum = createHash("blake2b512");
    for (let nonce = 0; nonce < 8; nonce += 1) {
      new DataView(candidate.buffer).setUint32(39, nonce, true);
      new Uint8Array(memory.buffer, inputPointer, candidate.length).set(candidate);
      assert.equal(rx_hash(handle, inputPointer, candidate.length, outputPointer), 0);
      const result = Buffer.from(new Uint8Array(memory.buffer, outputPointer, 32));
      checksum.update(result);
      expectedHashes.push(result);
    }
    assert.equal(checksum.digest("hex"), "5153f3a41ba4d01b7139f3f155ca3b99153f968f209d7b819446affe57ff82ca9270ea383e65f28e901826927e0634a633e30a57734782ca71a7c162e75a6a16");
    for (let nonce = 0; nonce < 8; nonce += 2) {
      new DataView(candidate.buffer).setUint32(39, nonce, true);
      new Uint8Array(memory.buffer, inputPointer, candidate.length).set(candidate);
      new DataView(candidate.buffer).setUint32(39, nonce + 1, true);
      new Uint8Array(memory.buffer, inputPointer + 512, candidate.length).set(candidate);
      assert.equal(instance.exports.rx_hash_pair(handle, inputPointer, inputPointer + 512, candidate.length, outputPointer), 0);
      assert.deepEqual(Buffer.from(memory.buffer, outputPointer, 64), Buffer.concat(expectedHashes.slice(nonce, nonce + 2)));
    }

  } finally {
    instance.exports.__indirect_function_table.set(slot, null);
    instance.exports.__indirect_function_table.set(slot + 1, null);
    instance.exports.__indirect_function_table.set(slot + 2, null);
    assert.equal(rx_destroy(handle), 0);
  }
});

}

test("Rust-generated dataset kernels match interpretation across seeds and u64 item boundaries", async () => {
  const bytes = await readFile(new URL("../web/static/js/solvers/randomx/randomx_wasm.wasm", import.meta.url));
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const exports = instance.exports;
  const pointer = exports.memory.buffer.byteLength;
  exports.memory.grow(1);
  let slot;
  let state = 0x9e3779b97f4a7c15n;
  const mask = (1n << 64n) - 1n;
  const items = [0n, 1n, 10_000_000n, 30_000_000n, (1n << 32n) - 1n, 1n << 32n, (1n << 63n) - 1n, 1n << 63n, mask];
  for (let index = 0; index < 128; index += 1) {
    state ^= state << 13n; state ^= state >> 7n; state ^= state << 17n; state &= mask;
    items.push(state);
  }
  for (const seed of [new TextEncoder().encode("test key 000"), new TextEncoder().encode("test key 001"), new Uint8Array(32).fill(0xab)]) {
    new Uint8Array(exports.memory.buffer, pointer, seed.length).set(seed);
    const handle = exports.rx_create(pointer, seed.length);
    assert.ok(handle >= 0);
    const installed = await installDatasetKernel(exports, handle, slot);
    slot = installed.slot;
    try {
      for (const item of items) {
        assert.equal(exports.rx_dataset_item(handle, item, pointer + 256), 0);
        installed.kernel(item, pointer + 512);
        assert.deepEqual(Buffer.from(exports.memory.buffer, pointer + 512, 64), Buffer.from(exports.memory.buffer, pointer + 256, 64), `dataset item ${item}`);
      }

      for (let index = 0; index < items.length; index += 1) {
        const first = items[index], second = items[(index + 1) % items.length];
        assert.equal(exports.rx_dataset_item(handle, first, pointer + 256), 0);
        assert.equal(exports.rx_dataset_item(handle, second, pointer + 320), 0);
        installed.pair(first, second, pointer + 512, pointer + 576);
        assert.deepEqual(Buffer.from(exports.memory.buffer, pointer + 512, 128), Buffer.from(exports.memory.buffer, pointer + 256, 128), `paired dataset ${first}, ${second}`);
      }
      for (let index = 0; index < items.length; index += 1) {
        const view = new DataView(exports.memory.buffer);
        for (let lane = 0; lane < 3; lane += 1) {
          const item = items[(index + lane) % items.length];
          view.setBigUint64(pointer + lane * 8, item, true);
          assert.equal(exports.rx_dataset_item(handle, item, pointer + 256 + lane * 64), 0);
        }
        installed.triple(pointer, pointer + 512);
        assert.deepEqual(Buffer.from(exports.memory.buffer, pointer + 512, 192), Buffer.from(exports.memory.buffer, pointer + 256, 192), `triple dataset ${index}`);
      }
      const firstInput = Uint8Array.from({length:87}, (_, i) => (i * 17 + 3) & 255);
      const expected = [];
      for (let lane = 0; lane < 3; lane += 1) {
        new DataView(firstInput.buffer).setUint32(39, 3 + lane, true);
        new Uint8Array(exports.memory.buffer, pointer + 256 * lane, 87).set(firstInput);
        assert.equal(exports.rx_hash(handle, pointer + 256 * lane, 87, pointer + 768), 0);
        expected.push(Buffer.from(new Uint8Array(exports.memory.buffer, pointer + 768, 32)));
      }
      assert.equal(exports.rx_hash_pair(handle, pointer, pointer + 256, 87, pointer + 768), 0);
      assert.deepEqual(Buffer.from(exports.memory.buffer, pointer + 768, 64), Buffer.concat(expected.slice(0,2)));
      assert.equal(exports.rx_hash_triple(handle, pointer, 87, pointer + 768), 0);
      assert.deepEqual(Buffer.from(exports.memory.buffer, pointer + 768, 96), Buffer.concat(expected));
      assert.equal(exports.rx_hash_triple(-1, pointer, 87, pointer + 768), -1);
      assert.equal(exports.rx_hash_triple(handle, pointer, 257, pointer + 768), -1);
      assert.equal(exports.rx_hash_triple(handle, pointer, 0, pointer + 768), -1);
      assert.equal(exports.rx_set_dataset_triple_function(handle, 0), 0);
      exports.__indirect_function_table.set(slot + 2, null);
      assert.equal(exports.rx_set_dataset_pair_function(handle, 0), 0);
      exports.__indirect_function_table.set(slot + 1, null);
      assert.equal(exports.rx_set_dataset_function(handle, 0), 0);
      exports.__indirect_function_table.set(slot, null);
    } finally {
      assert.equal(exports.rx_destroy(handle), 0);
    }
  }
  assert.equal(exports.__indirect_function_table.length, slot + 3);
  assert.equal(exports.rx_set_dataset_triple_function(-1, slot + 2), -1);
  assert.equal(exports.rx_set_dataset_function(-1, slot), -1);
  assert.equal(exports.rx_set_dataset_pair_function(-1, slot + 1), -1);
  assert.equal(exports.rx_dataset_module_len(-1), 0);
  assert.equal(exports.rx_dataset_module_ptr(-1), 0);
});


test("imported cache reproduces the reference hash with regenerated kernels", async () => {
  const bytes = await readFile(new URL("../web/static/js/solvers/randomx/randomx_wasm.wasm", import.meta.url));
  const { instance: { exports } } = await WebAssembly.instantiate(bytes, {});
  const seed = new TextEncoder().encode("test key 000");
  const pointer = exports.memory.buffer.byteLength;
  exports.memory.grow(1);
  new Uint8Array(exports.memory.buffer, pointer, seed.length).set(seed);
  const original = exports.rx_create(pointer, seed.length);
  const length = exports.rx_cache_len(original);
  const cache = new Uint8Array(exports.memory.buffer, exports.rx_cache_ptr(original), length).slice();
  exports.rx_destroy(original);
  const importedPointer = exports.rx_cache_import_ptr();
  new Uint8Array(exports.memory.buffer, importedPointer, length).set(cache);
  const handle = exports.rx_create_from_cache(pointer, seed.length, length);
  assert.ok(handle >= 0);
  await installDatasetKernel(exports, handle);
  const input = new TextEncoder().encode("This is a test");
  new Uint8Array(exports.memory.buffer, pointer, input.length).set(input);
  assert.equal(exports.rx_hash(handle, pointer, input.length, pointer + 256), 0);
  assert.equal(Buffer.from(exports.memory.buffer, pointer + 256, 32).toString("hex"), "639183aae1bf4c9a35884cb46b09cad9175f04efd7684e7262a0ac1c2f0b4e3f");
  exports.rx_destroy(handle);
});
