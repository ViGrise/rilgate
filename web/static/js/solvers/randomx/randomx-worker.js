// RandomX solver Web Worker.
//
// Runs the pure-Rust RandomX Light implementation compiled to WebAssembly and
// keeps the ~256 MiB Argon2d cache alive across jobs that share a seed.

const NONCE_SPACE = 0x100000000;

let wasm = null;
let handle = -1;
let activeSeed = null;
let datasetSlot = -1;
let datasetPairSlot = -1;
let pairReady = false;
let datasetTripleSlot = -1;
let tripleReady = false;
let pending = null;
let pumping = false;
let generation = 0;

// Linear memory offsets for the reusable hash buffers. Browsers cap wasm
// memory, so the search loop must not allocate per candidate.
let inputPointer = 0;
let outputPointer = 0;
let seedPointer = 0;

self.onmessage = (event) => {
  const message = event.data;

  if (message.type === "solve") {
    pending = {
      ...message.job,
      requestId: message.requestId,
      generation: ++generation,
    };
    if (!pumping) {
      pumping = true;
      pump();
    }
    return;
  }

  if (message.type === "cancel") {
    generation += 1;
    pending = null;
  }
};

async function pump() {
  while (pending) {
    const job = pending;
    pending = null;

    try {
      const solution = await solveJob(job);
      if (solution && job.generation === generation) {
        self.postMessage({ type: "solved", requestId: job.requestId, ...solution });
      }
    } catch (error) {
      if (job.generation === generation) {
        self.postMessage({ type: "error", requestId: job.requestId, message: error.message });
      }
    }

    await yieldToEventLoop();
  }

  pumping = false;
}

async function solveJob(job) {
  const started = Date.now();
  checkDeadline(job);
  const blob = hexToBytes(job.blob);
  const seed = hexToBytes(job.seedHash);
  if (blob.length > 256 || seed.length !== 32) {
    throw new Error("RandomX jobs require a blob of at most 256 bytes and a 32-byte seed");
  }
  if (!Number.isInteger(job.nonceOffset) || job.nonceOffset < 0 || job.nonceOffset + 4 > blob.length) {
    throw new Error("RandomX nonce offset is outside the hashing blob");
  }
  const target = targetFor(job.difficulty);
  await ensureInstance(job.seedHash, job.wasmUrl, seed);
  const batchSize = tripleReady && wasm.exports.rx_hash_triple ? 3 : pairReady && wasm.exports.rx_hash_pair ? 2 : 1;
  const result = new Uint8Array(32 * batchSize);
  let attempts = 0;

  for (let nonce = 0; nonce < NONCE_SPACE; nonce += batchSize) {
    if (job.generation !== generation) {
      return null;
    }

    checkDeadline(job);
    const lanes = Math.min(batchSize, NONCE_SPACE - nonce);
    writeNonce(blob, job.nonceOffset, nonce);
    hash(blob, result.subarray(0, 32 * lanes), job.nonceOffset, nonce, lanes);
    attempts += lanes;
    checkDeadline(job);

    for (let lane = 0; lane < lanes; lane += 1) {
      const candidate = result.subarray(lane * 32, (lane + 1) * 32);
      if (bytesToBigInt(candidate) <= target) {
        return {
          nonce: (nonce + lane).toString(16).padStart(8, "0"),
          result: bytesToHex(candidate),
        };
      }
    }

    if ((attempts & 3) === 0) {
      self.postMessage({ type: "progress", requestId: job.requestId, attempts, elapsedMs: Date.now() - started });
    }

    await yieldToEventLoop();
  }

  throw new Error("no RandomX nonce met the difficulty within the nonce space");
}

async function ensureInstance(seedHash, wasmUrl, seed) {
  if (handle >= 0 && activeSeed === seedHash && wasm.url === wasmUrl) {
    return;
  }

  if (!wasm || wasm.url !== wasmUrl) {
    const exports = await instantiate(wasmUrl);
    if (handle >= 0) {
      clearDatasetKernel();
      wasm.exports.rx_destroy(handle);
    }
    handle = -1;
    activeSeed = null;
    wasm = { url: wasmUrl, exports };
    inputPointer = 0;
    outputPointer = 0;
    seedPointer = 0;
    datasetSlot = -1;
    datasetPairSlot = -1;
    datasetTripleSlot = -1;
    pairReady = false;
    tripleReady = false;
  }

  if (handle >= 0) {
    clearDatasetKernel();
    wasm.exports.rx_destroy(handle);
    handle = -1;
  }

  if (!seedPointer) {
    seedPointer = grow(wasm.exports.memory, 32);
  }
  new Uint8Array(wasm.exports.memory.buffer, seedPointer, seed.length).set(seed);
  handle = wasm.exports.rx_create(seedPointer, seed.length);

  if (handle < 0) {
    handle = -1;
    throw new Error("RandomX cache initialization failed in the browser");
  }

  await installDatasetKernel();
  activeSeed = seedHash;
}

function clearDatasetKernel() {
  pairReady = false;
  tripleReady = false;
  wasm.exports.rx_set_dataset_function?.(handle, 0);
  if (datasetSlot >= 0) {
    wasm.exports.__indirect_function_table.set(datasetSlot, null);
  }
  if (datasetPairSlot >= 0) {
    wasm.exports.rx_set_dataset_pair_function(handle, 0);
    wasm.exports.__indirect_function_table.set(datasetPairSlot, null);
  }
  if (datasetTripleSlot >= 0) {
    wasm.exports.rx_set_dataset_triple_function(handle, 0);
    wasm.exports.__indirect_function_table.set(datasetTripleSlot, null);
  }
}

async function installDatasetKernel() {
  const exports = wasm.exports;
  const table = exports.__indirect_function_table;
  if (!table || !exports.rx_dataset_module_ptr || !exports.rx_dataset_module_len || !exports.rx_set_dataset_function) {
    return;
  }
  try {
    const bytes = new Uint8Array(exports.memory.buffer,
      exports.rx_dataset_module_ptr(handle), exports.rx_dataset_module_len(handle)).slice();
    const { instance } = await WebAssembly.instantiate(bytes, { env: { memory: exports.memory } });
    const paired = exports.rx_set_dataset_pair_function && instance.exports.dataset_pair;
    const triple = exports.rx_set_dataset_triple_function && instance.exports.dataset_triple;
    if (datasetSlot < 0) {
      datasetSlot = table.grow(1 + (paired ? 1 : 0) + (triple ? 1 : 0));
      if (paired) datasetPairSlot = datasetSlot + 1;
      if (triple) datasetTripleSlot = datasetSlot + 1 + (paired ? 1 : 0);
    }
    table.set(datasetSlot, instance.exports.dataset_item);
    if (exports.rx_set_dataset_function(handle, datasetSlot) !== 0) {
      throw new Error("RandomX dataset kernel installation failed");
    }
    if (paired && datasetPairSlot >= 0) {
      table.set(datasetPairSlot, instance.exports.dataset_pair);
      if (exports.rx_set_dataset_pair_function(handle, datasetPairSlot) !== 0) {
        throw new Error("RandomX paired dataset kernel installation failed");
      }
      pairReady = true;
    }
    if (triple && datasetTripleSlot >= 0) {
      table.set(datasetTripleSlot, instance.exports.dataset_triple);
      if (exports.rx_set_dataset_triple_function(handle, datasetTripleSlot) !== 0) {
        throw new Error("RandomX triple dataset kernel installation failed");
      }
      tripleReady = true;
    }
  } catch {
    // Interpretation remains available when dynamic compilation is unavailable.
    clearDatasetKernel();
  }
}

async function instantiate(wasmUrl) {
  const compiled = await download(wasmUrl);

  if (typeof WebAssembly.instantiateStreaming === "function") {
    try {
      const { instance } = await WebAssembly.instantiateStreaming(compiled.clone(), {});
      return instance.exports;
    } catch {
      // Fall through to the buffer path for servers that mislabel the MIME type.
    }
  }

  const { instance } = await WebAssembly.instantiate(
    await compiled.arrayBuffer(),
    {},
  );
  return instance.exports;
}

function download(wasmUrl) {
  return fetch(wasmUrl).then((response) => {
    if (!response.ok) {
      throw new Error(
        `unable to download the RandomX module (HTTP ${response.status})`,
      );
    }
    return response;
  });
}

function hash(input, output, nonceOffset, nonce, lanes) {
  const memory = wasm.exports.memory;

  if (!inputPointer) {
    inputPointer = grow(memory, 768);
    outputPointer = grow(memory, 96);
  }

  new Uint8Array(memory.buffer, inputPointer, input.length).set(input);

  for (let lane = 1; lane < lanes; lane += 1) {
    const candidate = new Uint8Array(memory.buffer, inputPointer + lane * 256, input.length);
    candidate.set(input);
    writeNonce(candidate, nonceOffset, nonce + lane);
  }

  let status;
  if (lanes === 3) {
    status = wasm.exports.rx_hash_triple(handle, inputPointer, input.length, outputPointer);
  } else if (lanes === 2) {
    status = wasm.exports.rx_hash_pair(handle, inputPointer, inputPointer + 256, input.length, outputPointer);
  } else {
    status = wasm.exports.rx_hash(handle, inputPointer, input.length, outputPointer);
  }

  if (status !== 0) {
    throw new Error("RandomX hashing failed in the browser");
  }

  output.set(new Uint8Array(memory.buffer, outputPointer, output.length));
}

function grow(memory, length) {
  const pointer = memory.buffer.byteLength;
  memory.grow(Math.ceil(length / 65536) + 1);
  return pointer;
}

function yieldToEventLoop() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

/// Monero compares the PoW result as a little-endian 256-bit integer against
/// `(2^256 - 1) / difficulty`, the same rule the gateway verifier applies.
function targetFor(difficulty) {
  if (typeof difficulty === "number" && !Number.isSafeInteger(difficulty)) {
    throw new Error("RandomX difficulty must be an exact positive integer");
  }
  const value = BigInt(difficulty);
  if (value <= 0n || value > 0xffffffffffffffffn) {
    throw new Error("RandomX difficulty is outside the supported range");
  }
  return (2n ** 256n - 1n) / value;
}

function bytesToBigInt(bytes) {
  let value = 0n;
  for (let index = bytes.length - 1; index >= 0; index -= 1) {
    value = (value << 8n) | BigInt(bytes[index]);
  }
  return value;
}

function writeNonce(blob, offset, nonce) {
  blob[offset] = nonce & 0xff;
  blob[offset + 1] = (nonce >>> 8) & 0xff;
  blob[offset + 2] = (nonce >>> 16) & 0xff;
  blob[offset + 3] = (nonce >>> 24) & 0xff;
}

function hexToBytes(text) {
  if (typeof text !== "string" || !/^(?:[a-f0-9]{2})+$/i.test(text)) {
    throw new Error("RandomX job contains malformed hexadecimal data");
  }
  const bytes = new Uint8Array(text.length / 2);
  for (let index = 0; index < bytes.length; index += 1) {
    bytes[index] = parseInt(text.substr(index * 2, 2), 16);
  }
  return bytes;
}

function bytesToHex(bytes) {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function checkDeadline(job) {
  if (job.deadline === undefined) {
    return;
  }
  if (!Number.isSafeInteger(job.deadline) || job.deadline <= 0) {
    throw new Error("RandomX challenge has an invalid expiration");
  }
  if (Date.now() >= job.deadline) {
    throw new Error("RandomX challenge expired. Retry to request new work.");
  }
}
