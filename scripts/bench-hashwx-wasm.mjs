import { performance } from "node:perf_hooks";
import { createHashWx, loadHashWx } from "./lib/hashwx-wasm.mjs";

const exports = await loadHashWx();
const repetitions = 3;
const warmup = createHashWx(exports, new Uint8Array(32));
warmup.search(0, 0, 256, 0, 0);
const { instance } = await WebAssembly.instantiate(warmup.moduleBytes(), {});
instance.exports.search(0, 0, 256, 0, 0);
warmup.destroy();

console.log(
  `Node ${process.version}, ${process.arch}, single thread, ${repetitions} distinct seeds per row`,
);
console.log(
  "Interpreter totals include seed generation and module emission; generated totals also include compilation/instantiation.",
);
console.log(
  "Workload uses one search call per seed with target zero, exhausting the entire range.",
);
console.log(
  "nonces/seed  mode         setup ms/seed  compile ms/seed  hash ms/seed  hash-only H/s  total H/s  module bytes",
);
for (const count of [463, 65536]) {
  for (const mode of ["interpreter", "generated"]) {
    let setupMs = 0,
      compileMs = 0,
      hashMs = 0,
      size = 0;
    for (let i = 0; i < repetitions; i++) {
      const seed = Uint8Array.from(
        { length: 32 },
        (_, j) => (j * 37 + i * 71 + count + (mode === "generated" ? 13 : 0)) & 255,
      );
      const beforeSetup = performance.now();
      const context = createHashWx(exports, seed);
      setupMs += performance.now() - beforeSetup;
      try {
        let search = context.search;
        size += exports.hx_module_len(context.handle);
        if (mode === "generated") {
          const beforeCompile = performance.now();
          const { instance } = await WebAssembly.instantiate(context.moduleBytes(), {});
          compileMs += performance.now() - beforeCompile;
          search = instance.exports.search;
        }
        const beforeHash = performance.now();
        const status = search(0, 0, count, 0, 0);
        hashMs += performance.now() - beforeHash;
        if (status !== -1) throw new Error("zero target must exhaust the range");
      } finally {
        context.destroy();
      }
    }
    const hashes = count * repetitions;
    console.log(
      `${String(count).padEnd(12)} ${mode.padEnd(12)} ${(setupMs / repetitions).toFixed(3).padStart(12)} ${(compileMs / repetitions).toFixed(3).padStart(16)} ${(hashMs / repetitions).toFixed(3).padStart(13)} ${Math.round(
        (hashes * 1000) / hashMs,
      )
        .toString()
        .padStart(14)} ${Math.round((hashes * 1000) / (setupMs + compileMs + hashMs))
        .toString()
        .padStart(10)} ${Math.round(size / repetitions)
        .toString()
        .padStart(13)}`,
    );
  }
}
