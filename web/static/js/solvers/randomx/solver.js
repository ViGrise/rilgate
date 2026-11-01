// Browser RandomX solver adapter.
import { runWorkers } from "../worker-pool.js";

const WORKER_URL = new URL("./randomx-worker.js?v=__WASM_REVISION__", import.meta.url);
const SIMD_URL = new URL("./randomx_wasm.wasm?v=__WASM_REVISION__", import.meta.url);
const SCALAR_URL = new URL("./randomx_wasm_scalar.wasm?v=__WASM_REVISION__", import.meta.url);
const DEVELOPMENT = "__RX_DEVELOPMENT__" === "true";
let inFlight = null;
let nextRequestId = 0;

export function workerCount(cores, deviceMemory, maxWorkers = 8) {
  const byCores = Math.max(1, (cores || 2) - 1);
  const byMemory = deviceMemory ? Math.floor(deviceMemory / 2) : 2;
  return Math.max(1, Math.min(byCores, byMemory, maxWorkers));
}

export function supportsSimd() {
  return WebAssembly.validate(
    new Uint8Array([
      0, 97, 115, 109, 1, 0, 0, 0, 1, 4, 1, 96, 0, 0, 3, 2, 1, 0, 10, 9, 1, 7, 0, 65, 0, 253, 15,
      26, 11,
    ]),
  );
}

export async function solve(
  job,
  { max_workers: maxWorkers = 8, store_cache: storeCache = true } = {},
) {
  const seedHash = job.seedHash || job.seed_hash;
  if (!seedHash) throw new Error("RandomX job is missing the seed hash");
  if (!job.blob) throw new Error("RandomX job is missing the hashing blob");
  if (inFlight) {
    const error = new Error("RandomX job superseded by a newer challenge");
    error.name = "SupersededError";
    inFlight.cancel(error);
  }
  const params = DEVELOPMENT ? new URLSearchParams(globalThis.location.search) : null;
  const override = params?.get("rx_workers");
  const count = /^[1-8]$/.test(override)
    ? Math.min(Number(override), maxWorkers)
    : workerCount(navigator.hardwareConcurrency, navigator.deviceMemory, maxWorkers);
  const scalar = params?.get("rx_scalar") === "1" || !supportsSimd();
  const requestId = ++nextRequestId;
  const progress = new Map();
  let cacheWriter = false;
  const run = runWorkers({
    url: WORKER_URL,
    options: { type: "classic" },
    count,
    label: "RandomX",
    failures: "last",
    start: (worker, index) =>
      worker.postMessage({
        type: "solve",
        requestId,
        job: {
          blob: job.blob,
          seedHash,
          difficulty: job.difficulty,
          nonceOffset: job.nonceOffset,
          deadline:
            job.expiresIn === undefined
              ? undefined
              : (job.receivedAt ?? Date.now()) + job.expiresIn * 1000,
          wasmUrl: job.wasmUrl || (scalar ? SCALAR_URL : SIMD_URL).href,
          workerIndex: index,
          workerCount: count,
          storeCache,
        },
      }),
    onMessage: (message, { worker, index, resolve, fail }) => {
      if (message.requestId !== requestId) return;
      if (message.type === "cache-built" && storeCache && !cacheWriter) {
        cacheWriter = true;
        worker.postMessage({ type: "persist-cache" });
      } else if (message.type === "solved") {
        resolve({ nonce: message.nonce, result: message.result });
      } else if (message.type === "progress") {
        progress.set(index, message);
        const values = [...progress.values()];
        const attempts = values.reduce((sum, value) => sum + value.attempts, 0);
        const rate = values.reduce(
          (sum, value) => sum + value.attempts / Math.max(value.elapsedMs / 1000, 0.001),
          0,
        );
        job.onProgress?.({ attempts, elapsedMs: (attempts / rate) * 1000 });
      } else if (message.type === "error") {
        fail(new Error(message.message));
      }
    },
  });
  inFlight = run;
  try {
    return await run.promise;
  } finally {
    if (inFlight === run) inFlight = null;
  }
}

export function cancel() {
  if (inFlight) {
    const error = new Error("RandomX solve canceled");
    error.name = "AbortError";
    inFlight.cancel(error);
    inFlight = null;
  }
}
