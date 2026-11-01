import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { webcrypto } from "node:crypto";
import test from "node:test";
import vm from "node:vm";

// The solver source is evaluated as a standalone script, so the worker pool import is inlined.
const poolSource = await readFile(
  new URL("../web/static/js/solvers/worker-pool.js", import.meta.url),
  "utf8",
);
const solverSource = (
  await readFile(new URL("../web/static/js/solvers/randomx/solver.js", import.meta.url), "utf8")
).replace('import { runWorkers } from "../worker-pool.js";', poolSource.replace("export ", ""));
const workerSource = await readFile(
  new URL("../web/static/js/solvers/randomx/randomx-worker.js", import.meta.url),
  "utf8",
);
const solver = await import(
  `data:text/javascript,${encodeURIComponent(solverSource.replaceAll("import.meta.url", JSON.stringify("https://example.test/solver.js")))}`
);

test("worker counts respect CPU availability and the cache memory budget", () => {
  for (const [cores, memory, expected] of [
    [12, 16, 8],
    [8, 8, 4],
    [12, undefined, 2],
    [2, 2, 1],
    [4, 0.5, 1],
    [16, 32, 8],
  ]) {
    assert.equal(solver.workerCount(cores, memory), expected);
  }
});

test("two workers hash disjoint groups of three consecutive nonces over 100 groups", async () => {
  const seen = new Set();
  for (let workerIndex = 0; workerIndex < 2; workerIndex += 1) {
    let groups = 0;
    const context = vm.createContext({
      self: { postMessage() {} },
      Date,
      Uint8Array,
      BigInt,
      record(nonce, lanes) {
        assert.equal(lanes, 3);
        assert.equal(nonce, (workerIndex + groups * 2) * 3);
        for (let lane = 0; lane < lanes; lane += 1) {
          assert.ok(!seen.has(nonce + lane));
          seen.add(nonce + lane);
        }
        groups += 1;
      },
      done: () => groups === 100,
    });
    vm.runInContext(
      workerSource +
        `
      ensureInstance = async () => { wasm = { exports: {} }; };
      hash = (_blob, output, _offset, nonce, lanes) => { record(nonce, lanes); output.fill(255); };
      yieldToEventLoop = async () => { if (done()) generation += 1; };
    `,
      context,
    );
    await vm.runInContext(
      `solveJob({blob: "00000000", seedHash: "00".repeat(32), difficulty: 1000000, nonceOffset: 0, workerIndex: ${workerIndex}, workerCount: 2, generation: 0})`,
      context,
    );
  }
  assert.equal(seen.size, 600);
});

test("a corrupt stored cache is deleted and the worker builds the cache", async () => {
  let deleted = false,
    builds = 0;
  const store = {
    get() {
      return { result: { bytes: new ArrayBuffer(32), digest: "bad digest" } };
    },
    delete() {
      deleted = true;
      return {};
    },
  };
  const database = {
    close() {},
    transaction() {
      const transaction = { objectStore: () => store };
      queueMicrotask(() => transaction.oncomplete());
      return transaction;
    },
  };
  const context = vm.createContext({
    self: {},
    Uint8Array,
    indexedDB: {
      open() {
        const request = { result: database };
        queueMicrotask(() => request.onsuccess());
        return request;
      },
    },
    crypto: webcrypto,
    WebAssembly,
    exports: {
      memory: new WebAssembly.Memory({ initial: 1 }),
      rx_cache_size: () => 32,
      rx_create: () => {
        builds += 1;
        return 0;
      },
    },
  });
  vm.runInContext(workerSource + `\ninstantiate = async () => exports;`, context);
  await vm.runInContext(
    'ensureInstance("00".repeat(32), "module.wasm", new Uint8Array(32))',
    context,
  );
  assert.equal(deleted, true);
  assert.equal(builds, 1);
});

async function solverSelection(development, simd, options = {}) {
  const jobs = [];
  const workers = [];
  const context = vm.createContext({
    URL,
    URLSearchParams,
    navigator: { hardwareConcurrency: 8, deviceMemory: 8 },
    options,
    location: { search: "?rx_workers=8&rx_scalar=1" },
    WebAssembly: { validate: () => simd },
    console,
    Worker: class {
      constructor() {
        workers.push(this);
      }
      postMessage(message) {
        jobs.push(message.job);
        queueMicrotask(() =>
          this.onmessage({
            data: { type: "solved", requestId: message.requestId, nonce: "00000000", result: "00" },
          }),
        );
      }
      terminate() {
        this.terminated = true;
      }
    },
  });
  vm.runInContext(
    solverSource
      .replaceAll("import.meta.url", JSON.stringify("https://example.test/solver.js"))
      .replaceAll("__RX_DEVELOPMENT__", String(development))
      .replaceAll("export ", ""),
    context,
  );
  await vm.runInContext(
    'solve({blob: "00000000", seedHash: "00".repeat(32), nonceOffset: 0, difficulty: 1000000}, options)',
    context,
  );
  assert.ok(workers.every((worker) => worker.terminated));
  return jobs;
}

test("production ignores development overrides and selects a module supported by the browser", async () => {
  for (const simd of [true, false]) {
    const jobs = await solverSelection(false, simd);
    assert.equal(jobs.length, 4);
    assert.ok(
      jobs.every((job) =>
        job.wasmUrl.includes(simd ? "randomx_wasm.wasm" : "randomx_wasm_scalar.wasm"),
      ),
    );
  }
});

test("development honors the worker and scalar measurement overrides", async () => {
  const jobs = await solverSelection(true, true);
  assert.equal(jobs.length, 8);
  assert.ok(jobs.every((job) => job.wasmUrl.includes("randomx_wasm_scalar.wasm")));
});

test("surviving workers continue after startup and cache allocation failures", async () => {
  let created = 0;
  const workers = [];
  const warnings = [];
  const context = vm.createContext({
    URL,
    URLSearchParams,
    navigator: { hardwareConcurrency: 8, deviceMemory: 8 },
    WebAssembly,
    console: { warn: (...args) => warnings.push(args) },
    Worker: class {
      constructor() {
        this.index = created++;
        if (this.index === 0) throw new Error("Worker startup failed");
        workers.push(this);
      }
      postMessage(message) {
        if (message.type !== "solve") return;
        queueMicrotask(() =>
          this.onmessage({
            data:
              this.index === 1
                ? {
                    type: "error",
                    requestId: message.requestId,
                    message: "RangeError: cache allocation failed",
                  }
                : { type: "solved", requestId: message.requestId, nonce: "winner", result: "00" },
          }),
        );
      }
      terminate() {
        this.terminated = true;
      }
    },
  });
  vm.runInContext(
    solverSource
      .replaceAll("import.meta.url", JSON.stringify("https://example.test/solver.js"))
      .replaceAll("export ", ""),
    context,
  );
  const result = await vm.runInContext(
    'solve({blob: "00000000", seedHash: "00".repeat(32), nonceOffset: 0, difficulty: 1000000})',
    context,
  );
  assert.equal(result.nonce, "winner");
  assert.equal(warnings.length, 2);
  assert.ok(workers.every((worker) => worker.terminated));
});

test("configured worker caps constrain automatic selection and development overrides", async () => {
  assert.equal(solver.workerCount(12, 16, 2), 2);
  assert.equal(solver.workerCount(12, 16, 8), 8);
  assert.equal(solver.workerCount(2, 2, 8), 1);
  const jobs = await solverSelection(true, true, { max_workers: 2, store_cache: false });
  assert.equal(jobs.length, 2);
  assert.ok(jobs.every((job) => job.storeCache === false));
});

test("disabled cache storage builds the cache without opening IndexedDB", async () => {
  let opens = 0,
    builds = 0;
  const messages = [];
  const context = vm.createContext({
    self: { postMessage: (message) => messages.push(message) },
    Uint8Array,
    BigInt,
    Date,
    WebAssembly,
    setTimeout,
    indexedDB: {
      open() {
        opens += 1;
        throw new Error("Storage disabled");
      },
    },
    exports: {
      memory: new WebAssembly.Memory({ initial: 1 }),
      rx_create: () => {
        builds += 1;
        return 0;
      },
    },
  });
  vm.runInContext(
    workerSource +
      `
    instantiate = async () => exports;
    hash = (_blob, output) => output.fill(0);
  `,
    context,
  );
  await vm.runInContext(
    `self.onmessage({data: {type: "solve", requestId: 1, job: {
    blob: "00000000", seedHash: "00".repeat(32), nonceOffset: 0,
    difficulty: 1, wasmUrl: "module.wasm", storeCache: false,
  }}})`,
    context,
  );
  for (
    let attempt = 0;
    attempt < 100 && !messages.some((message) => message.type === "solved");
    attempt += 1
  ) {
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
  assert.ok(messages.some((message) => message.type === "solved"));
  await vm.runInContext("persistCache()", context);
  assert.equal(builds, 1);
  assert.equal(opens, 0);
});
