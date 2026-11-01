import { runWorkers } from "../worker-pool.js";

const WORKER_URL = new URL("./worker.js", import.meta.url);
const WASM_URL = new URL("./hashwx_wasm.wasm", import.meta.url);
let active;

function failure(message, name = "Error") {
  const error = new Error(message);
  error.name = name;
  return error;
}

export function cancel() {
  active?.cancel(failure("HashWX calculation canceled", "AbortError"));
}

export function solve(challenge, options = {}) {
  active?.cancel(failure("HashWX challenge superseded", "SupersededError"));
  const count =
    options.workers ?? Math.min(2, Math.max(1, globalThis.navigator?.hardwareConcurrency || 1));
  if (
    !Number.isInteger(count) ||
    count < 1 ||
    count > 8 ||
    !Number.isSafeInteger(challenge.expires_in) ||
    challenge.expires_in < 0
  ) {
    return Promise.reject(new Error("Invalid HashWX solver parameters"));
  }
  const deadline = (challenge.received_at ?? Date.now()) + challenge.expires_in * 1000;
  if (!Number.isSafeInteger(deadline)) return Promise.reject(new Error("Invalid HashWX deadline"));
  const attempts = new Array(count).fill(0);
  const started = Date.now();
  const run = runWorkers({
    url: WORKER_URL,
    count,
    label: "HashWX",
    failures: "first",
    deadline,
    start: (worker, index) =>
      worker.postMessage({
        challenge,
        deadline,
        startBatch: index,
        batchStride: count,
        wasmUrl: WASM_URL.href,
      }),
    onMessage: (data, { index, resolve, reject }) => {
      if (data.type === "solved") {
        if (Date.now() >= deadline) throw new Error("HashWX challenge expired");
        resolve({ nonce: data.nonce });
      } else if (data.type === "error") {
        reject(new Error(data.message));
      } else if (data.type === "progress") {
        attempts[index] = data.attempts;
        options.onProgress?.({
          attempts: attempts.reduce((sum, value) => sum + value, 0),
          elapsedMs: Date.now() - started,
        });
      }
    },
  });
  active = run;
  return run.promise.finally(() => {
    if (active === run) active = null;
  });
}
