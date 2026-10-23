import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import vm from 'node:vm';
import { loadHashWx, createHashWx } from '../scripts/lib/hashwx-wasm.mjs';

const directory = new URL('../web/static/js/solvers/hashwx/', import.meta.url);
const source = await readFile(new URL('worker.js', directory), 'utf8');
const bytes = await readFile(new URL('hashwx_wasm.wasm', directory));
const challenge = { challenge: 'ab'.repeat(32), difficulty: 1000, nonces_per_seed: 463, expires_in: 60, expires_at: Math.floor(Date.now() / 1000) + 60 };

for (const fallback of [false, true]) {
  test(`worker solves signed seed batches without BigInt, fallback=${fallback}`, async () => {
    const messages = [];
    let compilations = 0;
    const context = vm.createContext({
      self: { postMessage: message => messages.push(message) },
      fetch: async () => ({ ok: true, arrayBuffer: async () => bytes }),
      WebAssembly: { instantiate: async (...args) => {
        if (++compilations > 1 && fallback) throw new Error('Generated compilation unavailable');
        return WebAssembly.instantiate(...args);
      } },
      BigInt: undefined,
    });
    vm.runInContext(source, context);
    await context.self.onmessage({ data: { challenge, deadline: Date.now() + 60000, startBatch: 1, batchStride: 2, wasmUrl: 'local' } });
    const solved = messages.at(-1);
    assert.equal(solved.type, 'solved', solved.message);
    const nonce = BigInt(solved.nonce), batch = nonce / 463n;
    assert.equal(batch % 2n, 1n);
    const batchBytes = Buffer.alloc(8); batchBytes.writeBigUInt64LE(batch);
    const seed = createHash('sha256').update(Buffer.from(challenge.challenge, 'hex')).update(batchBytes).digest();
    const api = await loadHashWx(), hasher = createHashWx(api, seed);
    try {
      const words = hasher.hash(Number(nonce & 0xffffffffn), Number(nonce >> 32n));
      assert.ok((BigInt(words[0]) | BigInt(words[1]) << 32n) < ((1n << 64n) - 1n) / 1000n);
    } finally { hasher.destroy(); }
    if (fallback) {
      assert.equal(compilations, 2);
      assert.ok(messages.some(message => message.mode === 'interpreter'));
    }
  });
}

test('decimal nonce conversion preserves carries and the u64 limit without BigInt', () => {
  const context = vm.createContext({ self: {}, BigInt: undefined });
  vm.runInContext(source, context);
  for (const value of [0n, 4294967295n, 4294967296n, (1n << 53n) + 1n, (1n << 64n) - 1n]) {
    const lo = Number(value & 0xffffffffn), hi = Number(value >> 32n);
    assert.equal(vm.runInContext(`decimal(${lo}, ${hi})`, context), String(value));
  }
});

test('expired and malformed work fails before downloading WASM', async () => {
  const messages = [];
  const context = vm.createContext({ self: { postMessage: message => messages.push(message) }, fetch: () => { throw new Error('Unexpected download'); } });
  vm.runInContext(source, context);
  for (const work of [{ ...challenge, expires_at: 1 }, { ...challenge, nonces_per_seed: 0 }]) {
    await context.self.onmessage({ data: { challenge: work, deadline: work.expires_at === 1 ? 1 : Date.now() + 60000, startBatch: 0, batchStride: 1 } });
    assert.equal(messages.at(-1).type, 'error');
    assert.match(messages.at(-1).message, /expired|Invalid/);
  }
});

let moduleId = 0;
async function solver(t) {
  const instances = [];
  const previous = globalThis.Worker;
  globalThis.Worker = class {
    constructor() { instances.push(this); }
    postMessage(message) { this.request = message; }
    terminate() { this.terminated = true; }
    emit(data) { this.onmessage({ data }); }
  };
  t.after(() => { globalThis.Worker = previous; });
  return { module: await import(`${new URL('solver.js', directory).href}?test=${++moduleId}`), instances };
}

test('parallel workers own distinct batches and all terminate when one solves', async t => {
  const { module, instances } = await solver(t);
  const progress = [];
  const pending = module.solve(challenge, { workers: 2, onProgress: message => progress.push(message.attempts) });
  assert.deepEqual(instances.map(worker => [worker.request.startBatch, worker.request.batchStride]), [[0, 2], [1, 2]]);
  instances[0].emit({ type: 'progress', attempts: 10 });
  instances[1].emit({ type: 'progress', attempts: 20 });
  assert.deepEqual(progress, [10, 30]);
  instances[1].emit({ type: 'solved', nonce: '123' });
  assert.deepEqual(await pending, { nonce: '123' });
  assert.ok(instances.every(worker => worker.terminated));
});

test('superseded and canceled work rejects and late messages cannot settle its replacement', async t => {
  const { module, instances } = await solver(t);
  const old = module.solve(challenge, { workers: 1 });
  const oldRejected = assert.rejects(old, { name: 'SupersededError' });
  const next = module.solve(challenge, { workers: 1 });
  const nextRejected = assert.rejects(next, { name: 'AbortError' });
  instances[0].emit({ type: 'solved', nonce: 'stale' });
  module.cancel();
  await oldRejected; await nextRejected;
  assert.ok(instances.every(worker => worker.terminated));
});

test('worker errors and progress callback errors release the whole worker group', async t => {
  const { module, instances } = await solver(t);
  const first = module.solve(challenge, { workers: 2 });
  const firstRejected = assert.rejects(first, /crash/);
  instances[0].onerror({ message: 'crash' });
  await firstRejected;
  const next = module.solve(challenge, { workers: 2, onProgress: () => { throw new Error('callback'); } });
  const nextRejected = assert.rejects(next, /callback/);
  instances[2].emit({ type: 'progress', attempts: 1 });
  await nextRejected;
  assert.ok(instances.every(worker => worker.terminated));
});

test('worker reads the generated module result through its exported location', async () => {
  const messages = [];
  const memory = new WebAssembly.Memory({ initial: 1 });
  new DataView(memory.buffer).setUint32(128, 987, true);
  let calls = 0;
  const context = vm.createContext({
    self: { postMessage: message => messages.push(message) },
    fetch: async () => ({ ok: true, arrayBuffer: async () => bytes }),
    WebAssembly: { instantiate: async (...args) => ++calls === 1 ? WebAssembly.instantiate(...args) :
      { instance: { exports: { memory, result_ptr: { value: 128 }, search: () => 0 } } } },
  });
  vm.runInContext(source, context);
  await context.self.onmessage({ data: { challenge, deadline: Date.now() + 60000, startBatch: 0, batchStride: 1 } });
  assert.equal(messages.at(-1).nonce, '987');
});


test('relative lifetime permits a HashWX solve with a client clock one day ahead', async t => {
  const now = Date.now() + 86400000;
  const original = Date.now;
  Date.now = () => now;
  t.after(() => { Date.now = original; });
  const { module, instances } = await solver(t);
  t.after(() => module.cancel());
  const work = { ...challenge, received_at: now - 1000, expires_in: 60 };
  const pending = module.solve(work, { workers: 1 });
  pending.catch(() => {});
  if (!instances.length) await pending;
  assert.equal(instances[0].request.deadline, now + 59000);
  const messages = [];
  const context = vm.createContext({
    Date: { now: () => now },
    self: { postMessage: message => messages.push(message) },
    fetch: async () => ({ ok: true, arrayBuffer: async () => bytes }),
    WebAssembly,
  });
  vm.runInContext(source, context);
  await context.self.onmessage({ data: instances[0].request });
  assert.equal(messages.at(-1).type, 'solved', messages.at(-1).message);
  instances[0].emit(messages.at(-1));
  assert.match((await pending).nonce, /^\d+$/);
});
