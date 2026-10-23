import { readFile } from "node:fs/promises";

export async function loadHashWx() {
  const bytes = await readFile(new URL("../../web/static/js/solvers/hashwx/hashwx_wasm.wasm", import.meta.url));
  const { instance } = await WebAssembly.instantiate(bytes, {});
  return instance.exports;
}

export function createHashWx(exports, seed) {
  const seedPointer = exports.hx_alloc();
  new Uint8Array(exports.memory.buffer, seedPointer, 32).set(seed);
  const handle = exports.hx_create(seedPointer, 32);
  exports.hx_free(seedPointer);
  if (handle < 0) throw new Error("HashWX instance creation failed");
  const output = exports.hx_alloc();
  return {
    handle,
    output,
    moduleBytes() {
      return new Uint8Array(exports.memory.buffer, exports.hx_module_ptr(handle), exports.hx_module_len(handle)).slice();
    },
    hash(lo, hi) {
      if (exports.hx_hash(handle, lo, hi, output) !== 0) throw new Error("HashWX hash failed");
      return words(exports.memory, output);
    },
    search(lo, hi, count, targetLo, targetHi) {
      return exports.hx_search(handle, lo, hi, count, targetLo, targetHi, output);
    },
    destroy() {
      exports.hx_free(output);
      return exports.hx_destroy(handle);
    },
  };
}

export function words(memory, pointer, count = 2) {
  const view = new DataView(memory.buffer);
  return Array.from({ length: count }, (_, i) => view.getUint32(pointer + i * 4, true));
}
