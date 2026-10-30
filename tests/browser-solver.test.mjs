import {challengeSource} from "./support/challenge-source.mjs";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const solverUrl = new URL("../web/static/js/solvers/randomx/solver.js", import.meta.url);
const workerUrl = new URL("../web/static/js/solvers/randomx/randomx-worker.js", import.meta.url);
let moduleId = 0;

async function solver(t) {
  const instances = [];
  let postFailure = false;
  class Worker {
    messages = [];
    terminated = false;
    constructor() { instances.push(this); }
    postMessage(message) {
      if (postFailure) throw new Error("post failed");
      this.messages.push(structuredClone(message));
    }
    terminate() { this.terminated = true; }
    emit(message) { this.onmessage({ data: message }); }
  }
  const previousNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { hardwareConcurrency: 2, deviceMemory: 2 } });
  t.after(() => Object.defineProperty(globalThis, "navigator", previousNavigator));
  const previous = globalThis.Worker;
  globalThis.Worker = Worker;
  t.after(() => { globalThis.Worker = previous; });
  const module = await import(`${solverUrl.href}?test=${++moduleId}`);
  return { solve: module.solve, cancel: module.cancel, instances, failPosts: value => { postFailure = value; } };
}

const job = { blob: "00".repeat(64), seed_hash: "ab".repeat(32), difficulty: 1000, nonceOffset: 39 };

function lastRequest(worker) {
  return worker.messages.findLast((message) => message.type === "solve");
}

test("progress callbacks stay on the page while worker payloads are cloneable", async (t) => {
  const { solve, instances } = await solver(t);
  const reports = [];
  const result = solve({ ...job, onProgress: (message) => reports.push(message.attempts) });
  const worker = instances[0];
  const request = lastRequest(worker);
  assert.equal(request.job.onProgress, undefined);
  worker.emit({ type: "progress", requestId: request.requestId, attempts: 4 });
  worker.emit({ type: "solved", requestId: request.requestId, nonce: "01020304", result: "ff".repeat(32) });
  assert.deepEqual(reports, [4]);
  assert.equal((await result).nonce, "01020304");
});

test("job rotation rejects the old solve and ignores its late replies", async (t) => {
  const { solve, instances } = await solver(t);
  const old = solve(job);
  const worker = instances[0];
  const oldId = lastRequest(worker).requestId;
  const rejected = assert.rejects(old, { name: "SupersededError" });
  let settled = false;
  const current = solve(job).then((value) => { settled = true; return value; });
  const currentWorker = instances[1];
  const currentId = lastRequest(currentWorker).requestId;
  worker.emit({ type: "solved", requestId: oldId, nonce: "stale" });
  worker.emit({ type: "error", requestId: oldId, message: "stale" });
  await Promise.resolve();
  assert.equal(settled, false);
  currentWorker.emit({ type: "solved", requestId: currentId, nonce: "current", result: "00".repeat(32) });
  assert.equal((await current).nonce, "current");
  await rejected;
});

test("worker errors allow a later solve to create a fresh worker", async (t) => {
  const { solve, instances } = await solver(t);
  const first = solve(job);
  const rejected = assert.rejects(first, /worker crashed/);
  instances[0].onerror({ message: "worker crashed" });
  await rejected;
  assert.equal(instances[0].terminated, true);
  const next = solve(job);
  assert.equal(instances.length, 2);
  const request = lastRequest(instances[1]);
  instances[1].emit({ type: "solved", requestId: request.requestId, nonce: "fresh" });
  assert.equal((await next).nonce, "fresh");
});

test("a failed postMessage rejects cleanly and does not strand the next solve", async (t) => {
  const { solve, instances, failPosts } = await solver(t);
  failPosts(true);
  await assert.rejects(solve(job), /post failed/);
  assert.equal(instances[0].terminated, true);
  failPosts(false);
  const next = solve(job);
  const worker = instances[1];
  worker.emit({ type: "solved", requestId: lastRequest(worker).requestId, nonce: "next" });
  assert.equal((await next).nonce, "next");
});

async function workerHarness(instantiateStreaming, clock = Date, startNonce = 0) {
  const messages = [];
  const memory = new WebAssembly.Memory({ initial: 1 });
  const exports = {
    memory,
    rx_create: () => 0,
    rx_destroy: () => 0,
    rx_hash: (_handle, _input, _length, output) => {
      new Uint8Array(memory.buffer, output, 32).fill(0);
      return 0;
    },
  };
  const self = { postMessage: (message) => { if (message.type !== "cache-built") messages.push(message); } };
  const context = vm.createContext({
    self, Uint8Array, Date: clock, BigInt, setTimeout, fetch: async () => new Response(new Uint8Array([0])),
    WebAssembly: { instantiateStreaming, instantiate: async () => ({ instance: { exports } }) },
  });
  const source = (await readFile(workerUrl, "utf8")).replace("let nonce = (job.workerIndex || 0) * 3;", `let nonce = ${startNonce};`);
  vm.runInContext(source, context);
  return { self, messages, memory, exports, wasmApi: context.WebAssembly };
}

async function until(predicate) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (predicate()) { return; }
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  assert.fail("worker did not finish");
}

test("WASM fallback can read the response after streaming compilation consumes it", async () => {
  const { self, messages } = await workerHarness(async (response) => {
    await response.arrayBuffer();
    throw new Error("streaming compilation unavailable");
  });
  self.onmessage({ data: { type: "solve", requestId: 10, job: { ...job, seedHash: job.seed_hash, wasmUrl: "test.wasm" } } });
  await until(() => messages.some((message) => message.type === "solved" || message.type === "error"));
  assert.equal(messages[0].type, "solved");
  assert.equal(messages[0].requestId, 10);
});

test("canceling during WASM initialization suppresses the old job result", async () => {
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const { self, messages } = await workerHarness(async () => {
    await gate;
    throw new Error("use buffer path");
  });
  self.onmessage({ data: { type: "solve", requestId: 20, job: { ...job, seedHash: job.seed_hash, wasmUrl: "test.wasm" } } });
  self.onmessage({ data: { type: "cancel" } });
  release();
  await new Promise((resolve) => setTimeout(resolve, 30));
  assert.deepEqual(messages, []);
  self.onmessage({ data: { type: "solve", requestId: 21, job: { ...job, seedHash: job.seed_hash, wasmUrl: "test.wasm" } } });
  await until(() => messages.some((message) => message.type === "solved"));
  assert.equal(messages[0].requestId, 21);
});

test("cancel releases the worker and rejects pending proof work", async (t) => {
  const { solve, cancel, instances } = await solver(t);
  const pending = solve(job);
  const rejected = assert.rejects(pending, { name: "AbortError" });
  cancel();
  await rejected;
  assert.equal(instances[0].terminated, true);
  const next = solve(job);
  assert.equal(instances.length, 2);
  instances[1].emit({ type: "solved", requestId: lastRequest(instances[1]).requestId, nonce: "fresh" });
  assert.equal((await next).nonce, "fresh");
  cancel();
});

test("invalid RandomX job data fails before allocating a WASM cache", async () => {
  let downloads = 0;
  const { self, messages } = await workerHarness(async () => {
    downloads += 1;
    throw new Error("unexpected download");
  });
  const valid = { ...job, seedHash: job.seed_hash, wasmUrl: "test.wasm" };
  const invalid = [
    { blob: "zz" }, { blob: "0" }, { blob: "00".repeat(257) },
    { seedHash: "ab" }, { nonceOffset: -1 }, { nonceOffset: 62 },
    { difficulty: 0 }, { difficulty: Number.MAX_SAFE_INTEGER + 1 },
  ];
  for (let index = 0; index < invalid.length; index += 1) {
    self.onmessage({ data: { type: "solve", requestId: index, job: { ...valid, ...invalid[index] } } });
    await until(() => messages.some((message) => message.requestId === index));
    assert.equal(messages.find((message) => message.requestId === index).type, "error");
  }
  assert.equal(downloads, 0);
});

test("RandomX asks for consent before loading the solver", async () => {
  const elements = new Map();
  const element = (selector) => {
    if (!elements.has(selector)) {
      elements.set(selector, { style: {}, textContent: "", hidden: true, removeAttribute() {}, removeEventListener() {}, addEventListener(type, handler) { this[type] = handler; } });
    }
    return elements.get(selector);
  };
  const english = JSON.parse(await readFile(new URL("../web/i18n/en.json", import.meta.url), "utf8"));
  element("#pow-i18n").textContent = JSON.stringify(english);
  element("#pow-bootstrap").textContent = JSON.stringify({ internal_prefix: "/gateway", algorithm: "randomx", embedded: false, presentation_token: "presentation", solver_module_url: "/solver.js" });
  const calls = [];
  const context = vm.createContext({
    AbortSignal, AbortController, setTimeout, clearTimeout,
    document: { querySelector: element },
    window: {},
    fetch: async (url) => {
      calls.push(url);
      return { status: 200, headers: { get: () => null }, text: async () => JSON.stringify({ enforce: "stratum_pool_accepted" }) };
    },
    console: { error: (error) => assert.fail(error.message) },
  });
  vm.runInContext(challengeSource, context);
  await until(() => !element("#randomx-start").hidden);
  assert.equal(element("#description").textContent, english["challenge.state.consent.description"]);
  assert.equal(element("#status").textContent, english["challenge.state.consent.status"]);
  assert.equal(typeof element("#randomx-start").click, "function");
  assert.deepEqual(calls, ["/gateway/api/client-config"]);
});

test("changing WASM URLs reloads the module even with the same seed", async () => {
  let downloads = 0;
  const { self, messages } = await workerHarness(async () => {
    downloads += 1;
    throw new Error("use buffer path");
  });
  for (let index = 0; index < 2; index += 1) {
    self.onmessage({ data: { type: "solve", requestId: index, job: { ...job, seedHash: job.seed_hash, wasmUrl: `module-${index}.wasm` } } });
    await until(() => messages.some((message) => message.requestId === index));
    assert.equal(messages.find((message) => message.requestId === index).type, "solved");
  }
  assert.equal(downloads, 2);
});

test("seed rotation reuses caller buffers instead of growing memory per job", async () => {
  const { self, messages, memory } = await workerHarness(async () => { throw new Error("use buffer path"); });
  let initialSize;
  for (let index = 0; index < 3; index += 1) {
    self.onmessage({ data: { type: "solve", requestId: index, job: { ...job, seedHash: "ab".repeat(31) + index.toString(16).padStart(2, "0"), wasmUrl: "test.wasm" } } });
    await until(() => messages.some((message) => message.requestId === index));
    assert.equal(messages.find((message) => message.requestId === index).type, "solved");
    initialSize ??= memory.buffer.byteLength;
    assert.equal(memory.buffer.byteLength, initialSize);
  }
});

test("decimal strings preserve the full unsigned 64-bit difficulty", async () => {
  const { self, messages } = await workerHarness(async () => { throw new Error("use buffer path"); });
  self.onmessage({ data: { type: "solve", requestId: 1, job: { ...job, seedHash: job.seed_hash, difficulty: "18446744073709551615", wasmUrl: "test.wasm" } } });
  await until(() => messages.length > 0);
  assert.equal(messages[0].type, "solved");
});

test("expired work is rejected before downloading or allocating the cache", async () => {
  let downloads = 0;
  const { self, messages, memory } = await workerHarness(async () => { downloads += 1; throw new Error("use buffer path"); });
  self.onmessage({ data: { type: "solve", requestId: 1, job: { ...job, seedHash: job.seed_hash, deadline: Date.now(), wasmUrl: "test.wasm" } } });
  await until(() => messages.length > 0);
  assert.equal(messages[0].type, "error");
  assert.match(messages[0].message, /expired/);
  assert.equal(downloads, 0);
  assert.equal(memory.buffer.byteLength, 65536);
});

test("work that expires during initialization cannot return a solution", async () => {
  let wall = Date.now();
  const expiresAt = Math.floor(wall / 1000) + 60;
  const { self, messages } = await workerHarness(async () => {
    wall = expiresAt * 1000;
    throw new Error("use buffer path");
  }, { now: () => wall });
  self.onmessage({ data: { type: "solve", requestId: 1, job: { ...job, seedHash: job.seed_hash, deadline: expiresAt * 1000, wasmUrl: "test.wasm" } } });
  await until(() => messages.length > 0);
  assert.equal(messages[0].type, "error");
  assert.match(messages[0].message, /expired/);
});

test("a hash finishing at the deadline cannot return a solution", async () => {
  let wall = Date.now();
  const expiresAt = Math.floor(wall / 1000) + 60;
  const { self, messages, exports } = await workerHarness(async () => {
    throw new Error("use buffer path");
  }, { now: () => wall });
  const hash = exports.rx_hash;
  exports.rx_hash = (...args) => {
    wall = expiresAt * 1000;
    return hash(...args);
  };
  self.onmessage({ data: { type: "solve", requestId: 1, job: { ...job, seedHash: job.seed_hash, deadline: expiresAt * 1000, wasmUrl: "test.wasm" } } });
  await until(() => messages.length > 0);
  assert.equal(messages[0].type, "error");
  assert.match(messages[0].message, /expired/);
});

function datasetSupport(exports) {
  const updates = [];
  const entries = [];
  let grows = 0;
  exports.rx_dataset_module_ptr = () => 1024;
  exports.rx_dataset_module_len = () => 8;
  exports.rx_set_dataset_function = (handle, slot) => { updates.push([handle, slot]); return 0; };
  exports.dataset_item = () => {};
  exports.__indirect_function_table = {
    grow: () => { grows += 1; return 32; },
    set: (slot, entry) => entries.push([slot, entry]),
  };
  return { updates, entries, grows: () => grows };
}

test("worker installs one dataset kernel per seed and reuses its table slot", async () => {
  const { self, messages, exports } = await workerHarness(async () => { throw new Error("use buffer path"); });
  const support = datasetSupport(exports);
  for (let index = 0; index < 3; index += 1) {
    self.onmessage({ data: { type: "solve", requestId: index, job: { ...job, seedHash: index < 2 ? job.seed_hash : "cd".repeat(32), wasmUrl: "test.wasm" } } });
    await until(() => messages.some((message) => message.requestId === index));
    assert.equal(messages.find((message) => message.requestId === index).type, "solved");
  }
  assert.equal(support.grows(), 1);
  assert.deepEqual(support.updates, [[0, 32], [0, 0], [0, 32]]);
  assert.equal(support.entries[1][1], null);
});

test("dataset compilation failure leaves the interpreter available", async () => {
  const { self, messages, exports, wasmApi } = await workerHarness(async () => { throw new Error("use buffer path"); });
  const support = datasetSupport(exports);
  wasmApi.instantiate = async (_bytes, imports) => {
    if (imports.env) throw new Error("dynamic compilation blocked");
    return { instance: { exports } };
  };
  self.onmessage({ data: { type: "solve", requestId: 1, job: { ...job, seedHash: job.seed_hash, wasmUrl: "test.wasm" } } });
  await until(() => messages.length > 0);
  assert.equal(messages[0].type, "solved");
  assert.deepEqual(support.updates, [[0, 0]]);
  assert.equal(support.grows(), 0);
});

test("failed kernel registration removes its table entry and uses interpretation", async () => {
  const { self, messages, exports } = await workerHarness(async () => { throw new Error("use buffer path"); });
  const support = datasetSupport(exports);
  exports.rx_set_dataset_function = (_handle, slot) => { support.updates.push([0, slot]); return slot ? -1 : 0; };
  self.onmessage({ data: { type: "solve", requestId: 1, job: { ...job, seedHash: job.seed_hash, wasmUrl: "test.wasm" } } });
  await until(() => messages.length > 0);
  assert.equal(messages[0].type, "solved");
  assert.deepEqual(support.updates, [[0, 32], [0, 0]]);
  assert.equal(support.entries.at(-1)[1], null);
});

function pairedSupport(exports) {
  const support = datasetSupport(exports);
  const pairedUpdates = [];
  exports.dataset_pair = () => {};
  exports.rx_set_dataset_pair_function = (handle, slot) => { pairedUpdates.push([handle, slot]); return 0; };
  exports.rx_hash_pair = (_handle, first, second, size, output) => {
    assert.equal(size, 64);
    const view = new DataView(exports.memory.buffer);
    assert.equal(view.getUint32(first + 39, true), 0);
    assert.equal(view.getUint32(second + 39, true), 1);
    new Uint8Array(exports.memory.buffer, output, 32).fill(255);
    new Uint8Array(exports.memory.buffer, output + 32, 32).fill(0);
    return 0;
  };
  return {...support, pairedUpdates};
}

test("paired worker checks both nonce results and reuses two slots across seeds", async () => {
  const {self, messages, exports} = await workerHarness(async () => { throw new Error("use buffer path"); });
  const support = pairedSupport(exports);
  for (let i = 0; i < 3; i += 1) {
    self.onmessage({data:{type:"solve", requestId:i, job:{...job,seedHash:i < 2 ? job.seed_hash : "cd".repeat(32),wasmUrl:"test.wasm"}}});
    await until(() => messages.some(message => message.requestId === i));
    const message = messages.find(message => message.requestId === i);
    assert.equal(message.type, "solved");
    assert.equal(message.nonce, "00000001");
    assert.equal(message.result, "00".repeat(32));
  }
  assert.equal(support.grows(), 1);
  assert.deepEqual(support.pairedUpdates, [[0,33],[0,0],[0,33]]);
});

test("paired installation failure disables both kernels and retains single hashing", async () => {
  const {self, messages, exports} = await workerHarness(async () => { throw new Error("use buffer path"); });
  const support = pairedSupport(exports);
  exports.rx_set_dataset_pair_function = (_handle, slot) => { support.pairedUpdates.push([0,slot]); return slot ? -1 : 0; };
  exports.rx_hash_pair = () => { assert.fail("disabled pair must not run"); };
  self.onmessage({data:{type:"solve", requestId:1, job:{...job,seedHash:job.seed_hash,wasmUrl:"test.wasm"}}});
  await until(() => messages.length > 0);
  assert.equal(messages[0].type, "solved");
  assert.equal(messages[0].nonce, "00000000");
  assert.deepEqual(support.pairedUpdates, [[0,33],[0,0]]);
  assert.equal(support.entries.at(-1)[1], null);
});

test("paired work finishing at the deadline cannot return either solution", async () => {
  let wall = Date.now();
  const expiresAt = Math.floor(wall / 1000) + 60;
  const {self, messages, exports} = await workerHarness(async () => { throw new Error("use buffer path"); }, {now:() => wall});
  pairedSupport(exports);
  const hash = exports.rx_hash_pair;
  exports.rx_hash_pair = (...args) => { wall = expiresAt * 1000; return hash(...args); };
  self.onmessage({data:{type:"solve", requestId:1, job:{...job,seedHash:job.seed_hash,deadline:expiresAt * 1000,wasmUrl:"test.wasm"}}});
  await until(() => messages.length > 0);
  assert.equal(messages[0].type, "error");
  assert.match(messages[0].message, /expired/);
});

function tripleSupport(exports, winningLane = 2, firstNonce = 0) {
  const support = pairedSupport(exports);
  const tripleUpdates = [];
  exports.dataset_triple = () => {};
  exports.rx_set_dataset_triple_function = (handle, slot) => { tripleUpdates.push([handle, slot]); return 0; };
  exports.rx_hash_triple = (_handle, input, size, output) => {
    assert.equal(size, 64);
    const view = new DataView(exports.memory.buffer);
    for (let lane = 0; lane < 3; lane += 1) {
      assert.equal(view.getUint32(input + 256 * lane + 39, true), firstNonce + lane);
      new Uint8Array(exports.memory.buffer, output + 32 * lane, 32).fill(lane === winningLane ? 0 : 255);
    }
    return 0;
  };
  return {...support, tripleUpdates};
}

test("triple worker checks all lanes and reuses three kernel slots across seeds", async () => {
  for (const lane of [0, 1, 2]) {
    const {self, messages, exports} = await workerHarness(async () => { throw new Error("use buffer path"); });
    const support = tripleSupport(exports, lane);
    for (let i = 0; i < 3; i += 1) {
      self.onmessage({data:{type:"solve",requestId:i,job:{...job,seedHash:i < 2 ? job.seed_hash : "cd".repeat(32),wasmUrl:"test.wasm"}}});
      await until(() => messages.some(message => message.requestId === i));
      const message = messages.find(message => message.requestId === i);
      assert.equal(message.type, "solved");
      assert.equal(message.nonce, lane.toString(16).padStart(8,"0"));
      assert.equal(message.result, "00".repeat(32));
    }
    assert.equal(support.grows(), 1);
    assert.deepEqual(support.tripleUpdates, [[0,34],[0,0],[0,34]]);
    assert.deepEqual(support.pairedUpdates, [[0,33],[0,0],[0,33]]);
  }
});

test("triple registration failure clears every callback and falls back to single hashing", async () => {
  const {self, messages, exports} = await workerHarness(async () => { throw new Error("use buffer path"); });
  const support = tripleSupport(exports);
  exports.rx_set_dataset_triple_function = (_handle, slot) => { support.tripleUpdates.push([0,slot]); return slot ? -1 : 0; };
  exports.rx_hash_pair = exports.rx_hash_triple = () => { assert.fail("disabled batches must not run"); };
  self.onmessage({data:{type:"solve",requestId:1,job:{...job,seedHash:job.seed_hash,wasmUrl:"test.wasm"}}});
  await until(() => messages.length > 0);
  assert.equal(messages[0].type, "solved");
  assert.equal(messages[0].nonce, "00000000");
  assert.deepEqual(support.tripleUpdates, [[0,34],[0,0]]);
  assert.deepEqual(support.pairedUpdates, [[0,33],[0,0]]);
  for (const slot of [32,33,34]) assert.equal(support.entries.findLast(entry => entry[0] === slot)[1], null);
});

test("triple work finishing at the deadline cannot return a solution", async () => {
  let wall = Date.now();
  const expiresAt = Math.floor(wall / 1000) + 60;
  const {self, messages, exports} = await workerHarness(async () => { throw new Error("use buffer path"); }, {now:() => wall});
  tripleSupport(exports);
  const hash = exports.rx_hash_triple;
  exports.rx_hash_triple = (...args) => { wall = expiresAt * 1000; return hash(...args); };
  self.onmessage({data:{type:"solve",requestId:1,job:{...job,seedHash:job.seed_hash,deadline:expiresAt * 1000,wasmUrl:"test.wasm"}}});
  await until(() => messages.length > 0);
  assert.equal(messages[0].type, "error");
  assert.match(messages[0].message, /expired/);
});

test("triple search hashes the final 32-bit nonce singly without wrapping", async () => {
  const {self, messages, exports} = await workerHarness(async () => { throw new Error("use buffer path"); }, Date, 0xfffffffc);
  tripleSupport(exports, -1, 0xfffffffc);
  let singleCalls = 0;
  const hash = exports.rx_hash;
  exports.rx_hash = (handle, input, size, output) => {
    singleCalls += 1;
    assert.equal(new DataView(exports.memory.buffer).getUint32(input + 39, true), 0xffffffff);
    return hash(handle, input, size, output);
  };
  self.onmessage({data:{type:"solve",requestId:1,job:{...job,seedHash:job.seed_hash,wasmUrl:"test.wasm"}}});
  await until(() => messages.some(message => message.type === "solved" || message.type === "error"));
  const solved = messages.find(message => message.type === "solved");
  assert.equal(solved?.nonce, "ffffffff");
  assert.equal(singleCalls, 1);
});


test("relative lifetime permits RandomX work with a client clock one day ahead", async t => {
  const serverNow = Date.now(), clientNow = serverNow + 86400000;
  const { solve, cancel, instances } = await solver(t);
  t.after(cancel);
  const { self, messages } = await workerHarness(async () => { throw new Error("use buffer path"); }, { now: () => clientNow });
  const pending = solve({ ...job, expiresAt: Math.floor(serverNow / 1000) + 60, expiresIn: 60, receivedAt: clientNow - 1000 });
  pending.catch(() => {});
  const request = lastRequest(instances[0]);
  assert.equal(request.job.deadline, clientNow + 59000);
  self.onmessage({ data: request });
  await until(() => messages.some(message => message.type !== 'progress'));
  const result = messages.find(message => message.type !== 'progress');
  assert.equal(result.type, 'solved', result.message);
  instances[0].emit(result);
  await pending;
});
