// Browser RandomX solver adapter.
//
// Loads the gateway-hosted `randomx_wasm.wasm` (the same `randomx-core` code the
// server verifier runs) inside a dedicated worker and returns the first nonce
// whose RandomX result satisfies the challenge difficulty.

const WORKER_URL = new URL("./randomx-worker.js?v=__WASM_REVISION__", import.meta.url);
const WASM_URL = new URL("./randomx_wasm.wasm?v=__WASM_REVISION__", import.meta.url);

let worker = null;
let inFlight = null;
let nextRequestId = 0;

export async function solve(job) {
  const request = {
    blob: job.blob,
    seedHash: job.seedHash || job.seed_hash,
    difficulty: job.difficulty,
    nonceOffset: job.nonceOffset,
    deadline: job.expiresIn === undefined ? undefined :
      (job.receivedAt ?? Date.now()) + job.expiresIn * 1000,
    wasmUrl: job.wasmUrl || WASM_URL.href,
    onProgress: job.onProgress,
  };

  if (!request.seedHash) {
    throw new Error("RandomX job is missing the seed hash");
  }

  if (!request.blob) {
    throw new Error("RandomX job is missing the hashing blob");
  }

  if (inFlight) {
    worker.postMessage({ type: "cancel" });
    inFlight.reject(superseded());
    inFlight = null;
  }

  const pending = start(request);
  inFlight = pending;

  try {
    return await pending.promise;
  } finally {
    if (inFlight === pending) {
      inFlight = null;
    }
  }
}

function superseded() {
  const error = new Error("RandomX job superseded by a newer challenge");
  error.name = "SupersededError";
  return error;
}

function start(job) {
  const { onProgress, ...payload } = job;
  const requestId = ++nextRequestId;
  if (!worker) {
    worker = new Worker(WORKER_URL, { type: "classic" });
  }

  let reject;
  const promise = new Promise((resolve, fail) => {
    reject = fail;

    worker.onmessage = (event) => {
      const message = event.data;
      if (message.requestId !== requestId) {
        return;
      }

      if (message.type === "solved") {
        resolve({ nonce: message.nonce, result: message.result });
        return;
      }

      if (message.type === "progress" && onProgress) {
        onProgress(message);
        return;
      }

      if (message.type === "error") {
        reject(new Error(message.message));
      }
    };

    worker.onerror = (event) => {
      worker.terminate();
      worker = null;
      reject(new Error(event.message || "RandomX worker failed"));
    };
  });

  try {
    worker.postMessage({ type: "solve", requestId, job: payload });
  } catch (error) {
    reject(error);
  }

  return { promise, reject };
}

export function cancel() {
  if (worker) {
    worker.terminate();
    worker = null;
  }
  if (inFlight) {
    const error = new Error("RandomX solve canceled");
    error.name = "AbortError";
    inFlight.reject(error);
    inFlight = null;
  }
}
