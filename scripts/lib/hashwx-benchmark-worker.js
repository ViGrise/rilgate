function targetWords(difficulty) {
  const words = [0, 0];
  let remainder = 0;
  for (let bit = 63; bit >= 0; bit--) {
    remainder = remainder * 2 + 1;
    if (remainder >= difficulty) {
      remainder -= difficulty;
      words[bit >= 32 ? 1 : 0] += 2 ** (bit % 32);
    }
  }
  return words;
}

self.onmessage = async ({ data: parameters }) => {
  try {
    const { difficulties, trials, repetitions, counts } = parameters;
    if (!Array.isArray(difficulties) || !difficulties.length || difficulties.length > 8 ||
        difficulties.some((value) => !Number.isInteger(value) || value < 1 || value > 5000000) ||
        !Number.isInteger(trials) || trials < 1 || trials > 10 ||
        !Number.isInteger(repetitions) || repetitions < 1 || repetitions > 10 ||
        !Array.isArray(counts) || counts.some((value) => ![463, 65536].includes(value))) {
      throw new Error('Invalid benchmark parameters');
    }
    const startedAt = performance.now();
    const progress = (message) => self.postMessage({ type: 'progress', message });
    const checkDeadline = () => {
      if (performance.now() - startedAt > 600000) throw new Error('Benchmark reached its ten-minute limit');
    };
    progress('Loading the interpreter');
    const loadStart = performance.now();
    const response = await fetch('/hashwx.wasm');
    if (!response.ok) throw new Error('WASM download failed');
    const bytes = await response.arrayBuffer();
    const { instance } = await WebAssembly.instantiate(bytes, {});
    const loadMs = performance.now() - loadStart;
    const api = instance.exports;
    const seedPointer = api.hx_alloc();
    const output = api.hx_alloc();
    const makeInstance = (salt) => {
      let state = salt >>> 0 || 1;
      const seed = Uint8Array.from({ length: 32 }, () => {
        state ^= state << 13; state ^= state >>> 17; state ^= state << 5;
        return state & 255;
      });
      new Uint8Array(api.memory.buffer, seedPointer, 32).set(seed);
      const handle = api.hx_create(seedPointer, 32);
      if (handle < 0) throw new Error('Instance creation failed');
      return handle;
    };
    const engine = async (handle, mode) => {
      if (mode === 'interpreter') {
        return {
          search: (start, count, target) => api.hx_search(handle, start >>> 0, Math.floor(start / 4294967296), count, ...target, output),
          nonce: () => {
            const view = new DataView(api.memory.buffer);
            return view.getUint32(output, true) + view.getUint32(output + 4, true) * 4294967296;
          },
        };
      }
      const module = new Uint8Array(api.memory.buffer, api.hx_module_ptr(handle), api.hx_module_len(handle)).slice();
      const { instance } = await WebAssembly.instantiate(module, {});
      const compiled = instance.exports;
      return {
        search: (start, count, target) => compiled.search(start >>> 0, Math.floor(start / 4294967296), count, ...target),
        nonce: () => {
          const view = new DataView(compiled.memory.buffer);
          return view.getUint32(16408, true) + view.getUint32(16412, true) * 4294967296;
        },
      };
    };
    const rows = [];
    const solves = [];
    for (const count of counts) {
      for (const mode of ['interpreter', 'generated']) {
        let setupMs = 0, compileMs = 0, hashMs = 0;
        for (let i = -1; i < repetitions; i++) {
          checkDeadline();
          progress(`${mode}, ${count} nonces per seed: throughput ${i < 0 ? 'warmup' : i + 1}`);
          const setup = performance.now();
          const handle = makeInstance(count + (i + 2) * 71);
          try {
            const setupTime = performance.now() - setup;
            const compile = performance.now();
            const runner = await engine(handle, mode);
            const compileTime = performance.now() - compile;
            const start = performance.now();
            if (runner.search(0, count, [0, 0]) !== -1) throw new Error('Zero target accepted a proof');
            const hashTime = performance.now() - start;
            if (i >= 0) { setupMs += setupTime; compileMs += compileTime; hashMs += hashTime; }
          } finally { api.hx_destroy(handle); }
        }
        rows.push({ count, mode, setupMs: setupMs / repetitions, compileMs: compileMs / repetitions, hashMs: hashMs / repetitions, hashHps: count * repetitions * 1000 / hashMs, totalHps: count * repetitions * 1000 / (setupMs + compileMs + hashMs) });
        for (const difficulty of difficulties) {
          const target = targetWords(difficulty);
          for (let trial = 0; trial < trials; trial++) {
            const start = performance.now();
            let attempts = 0, batch = 0, found = false;
            while (!found) {
              checkDeadline();
              progress(`${mode}, ${count} nonces per seed, difficulty ${difficulty}, trial ${trial + 1}/${trials}: ${attempts} hashes`);
              const handle = makeInstance((difficulty ^ (trial + 1) * 104729 ^ batch * 15485863) >>> 0);
              try {
                const runner = await engine(handle, mode);
                for (let offset = 0; offset < count; offset += 256) {
                  checkDeadline();
                  const size = Math.min(256, count - offset);
                  const nonce = batch * count + offset;
                  if (!Number.isSafeInteger(nonce)) throw new Error('Prototype nonce range exhausted');
                  if (runner.search(nonce, size, target) === 0) {
                    attempts += runner.nonce() - nonce + 1;
                    found = true;
                    break;
                  }
                  attempts += size;
                }
              } finally { api.hx_destroy(handle); }
              batch++;
            }
            solves.push({ count, mode, difficulty, trial: trial + 1, attempts, batches: batch, elapsedMs: performance.now() - start });
          }
        }
      }
    }
    api.hx_free(output);
    api.hx_free(seedPointer);
    progress('Measuring the Fast SHA-256 baseline');
    importScripts('/fast-worker.js');
    let fastChecksum = 0;
    for (let i = 0; i < 1000; i++) fastChecksum ^= sha256(`benchmark${i}`)[0];
    const fastStart = performance.now();
    for (let i = 0; i < 65536; i++) fastChecksum ^= sha256(`benchmark${i}`)[0];
    self.postMessage({ type: 'complete', result: {
      schemaVersion: 1,
      recordedAt: new Date().toISOString(),
      userAgent: navigator.userAgent,
      hardwareConcurrency: navigator.hardwareConcurrency,
      parameters: { ...parameters, modes: ['interpreter', 'generated'], warmupSeeds: 1, searchChunk: 256, seedScheme: 'xorshift32 deterministic synthetic seeds; SHA-256 challenge derivation excluded', timing: 'one worker; setup and compilation included in solves; initial artifact load excluded' },
      loadMs, wasmBytes: bytes.byteLength, rows, solves,
      fastHps: 65536000 / (performance.now() - fastStart), fastChecksum,
      totalMs: performance.now() - startedAt,
    } });
  } catch (error) { self.postMessage({ type: 'error', message: error.message }); }
};
