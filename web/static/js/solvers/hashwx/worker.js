function targetWords(difficulty) {
  const target = [0, 0];
  let remainder = 0;
  for (let bit = 63; bit >= 0; bit--) {
    remainder = remainder * 2 + 1;
    if (remainder >= difficulty) {
      remainder -= difficulty;
      target[bit >= 32 ? 1 : 0] += 2 ** (bit % 32);
    }
  }
  return target;
}

function decimal(lo, hi) {
  let text = '';
  do {
    const quotientHi = Math.floor(hi / 10);
    const dividend = (hi % 10) * 4294967296 + lo;
    const quotientLo = Math.floor(dividend / 10);
    text = String(dividend % 10) + text;
    lo = quotientLo;
    hi = quotientHi;
  } while (lo || hi);
  return text;
}

self.onmessage = async ({ data }) => {
  const { challenge, deadline, wasmUrl, startBatch, batchStride } = data;
  try {
    if (!/^[0-9a-f]{64}$/.test(challenge.challenge) ||
        !Number.isSafeInteger(challenge.difficulty) || challenge.difficulty < 1 || challenge.difficulty > 2 ** 40 ||
        !Number.isInteger(challenge.nonces_per_seed) || challenge.nonces_per_seed < 463 || challenge.nonces_per_seed > 1048576 ||
        !Number.isSafeInteger(deadline) || deadline < 1 ||
        !Number.isInteger(startBatch) || startBatch < 0 || startBatch >= batchStride ||
        !Number.isInteger(batchStride) || batchStride < 1 || batchStride > 8) {
      throw new Error('Invalid HashWX challenge');
    }
    const alive = () => {
      if (Date.now() >= deadline) throw new Error('HashWX challenge expired');
    };
    alive();
    const response = await fetch(wasmUrl);
    if (!response.ok) throw new Error('HashWX WASM download failed');
    const { instance } = await WebAssembly.instantiate(await response.arrayBuffer(), {});
    const api = instance.exports;
    const challengePointer = api.hx_alloc(), seedPointer = api.hx_alloc(), output = api.hx_alloc();
    new Uint8Array(api.memory.buffer, challengePointer, 32).set(challenge.challenge.match(/../g).map((byte) => parseInt(byte, 16)));
    const target = targetWords(challenge.difficulty);
    let batchLo = startBatch, batchHi = 0, attempts = 0, generatedAllowed = true;
    const started = Date.now();
    let reportedAt = started;
    try {
      while (true) {
        alive();
        const productLo = batchLo * challenge.nonces_per_seed;
        const productHi = batchHi * challenge.nonces_per_seed + Math.floor(productLo / 4294967296);
        if (productHi >= 4294967296) throw new Error('HashWX nonce range exhausted');
        const nonceLo = productLo >>> 0, nonceHi = productHi >>> 0;
        api.hx_seed(challengePointer, batchLo, batchHi, seedPointer);
        const handle = api.hx_create(seedPointer, 32);
        if (handle < 0) throw new Error('HashWX instance creation failed');
        let compiled;
        try {
          if (generatedAllowed) {
            try {
              const pointer = api.hx_module_ptr(handle);
              const length = api.hx_module_len(handle);
              const bytes = new Uint8Array(api.memory.buffer, pointer, length).slice();
              compiled = (await WebAssembly.instantiate(bytes, {})).instance.exports;
            } catch {
              generatedAllowed = false;
            }
          }
          // The final seed batch ends at the largest unsigned nonce.
          const count = Math.min(challenge.nonces_per_seed, nonceHi === 0xffffffff ? 4294967296 - nonceLo : challenge.nonces_per_seed);
          for (let offset = 0; offset < count; offset += 256) {
            alive();
            const size = Math.min(256, count - offset);
            const lo = (nonceLo + offset) >>> 0;
            const hi = (nonceHi + Math.floor((nonceLo + offset) / 4294967296)) >>> 0;
            const status = compiled ? compiled.search(lo, hi, size, ...target) : api.hx_search(handle, lo, hi, size, ...target, output);
            if (status === 0) {
              const memory = compiled ? compiled.memory : api.memory;
              const pointer = compiled ? compiled.result_ptr.value : output;
              const view = new DataView(memory.buffer);
              const resultLo = view.getUint32(pointer, true), resultHi = view.getUint32(pointer + 4, true);
              alive();
              self.postMessage({ type: 'solved', nonce: decimal(resultLo, resultHi) });
              return;
            }
            attempts += size;
            if (Date.now() - reportedAt >= 100 || offset + size >= count) {
              reportedAt = Date.now();
              self.postMessage({ type: 'progress', attempts, elapsedMs: reportedAt - started, mode: compiled ? 'generated' : 'interpreter' });
            }
          }
        } finally { api.hx_destroy(handle); }
        const nextLo = batchLo + batchStride;
        batchLo = nextLo >>> 0;
        batchHi += Math.floor(nextLo / 4294967296);
        if (batchHi >= 4294967296) throw new Error('HashWX batch range exhausted');
      }
    } finally {
      api.hx_free(output); api.hx_free(seedPointer); api.hx_free(challengePointer);
    }
  } catch (error) { self.postMessage({ type: 'error', message: error.message }); }
};
