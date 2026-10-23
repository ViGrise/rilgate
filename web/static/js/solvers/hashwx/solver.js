const WORKER_URL = new URL('./worker.js', import.meta.url);
const WASM_URL = new URL('./hashwx_wasm.wasm', import.meta.url);
let active;

function failure(message, name = 'Error') {
  const error = new Error(message);
  error.name = name;
  return error;
}

export function cancel() {
  active?.finish(failure('HashWX calculation canceled', 'AbortError'));
}

export function solve(challenge, options = {}) {
  active?.finish(failure('HashWX challenge superseded', 'SupersededError'));
  const count = options.workers ?? Math.min(2, Math.max(1, globalThis.navigator?.hardwareConcurrency || 1));
  if (!Number.isInteger(count) || count < 1 || count > 8 ||
      !Number.isSafeInteger(challenge.expires_at) || challenge.expires_at < 1) {
    return Promise.reject(new Error('Invalid HashWX solver parameters'));
  }
  return new Promise((resolve, reject) => {
    const task = { workers: [], attempts: new Array(count).fill(0), timer: null };
    task.finish = (error, result) => {
      if (active !== task) return;
      active = null;
      clearTimeout(task.timer);
      for (const worker of task.workers) worker.terminate();
      if (error) reject(error);
      else resolve(result);
    };
    active = task;
    function expire() {
      const remaining = challenge.expires_at * 1000 - Date.now();
      if (remaining <= 0) task.finish(new Error('HashWX challenge expired'));
      else task.timer = setTimeout(expire, Math.min(remaining, 2147483647));
    }
    expire();
    if (active !== task) return;
    const started = Date.now();
    try {
      for (let index = 0; index < count; index++) {
        const worker = new Worker(WORKER_URL);
        task.workers.push(worker);
        worker.onmessage = ({ data }) => {
          if (active !== task) return;
          try {
            if (data.type === 'solved') {
              if (Date.now() / 1000 >= challenge.expires_at) throw new Error('HashWX challenge expired');
              task.finish(null, { nonce: data.nonce });
            } else if (data.type === 'error') {
              task.finish(new Error(data.message));
            } else if (data.type === 'progress') {
              task.attempts[index] = data.attempts;
              options.onProgress?.({ attempts: task.attempts.reduce((sum, value) => sum + value, 0), elapsedMs: Date.now() - started });
            }
          } catch (error) { task.finish(error); }
        };
        worker.onerror = (event) => task.finish(new Error(event.message || 'HashWX worker failed'));
        worker.postMessage({ challenge, startBatch: index, batchStride: count, wasmUrl: WASM_URL.href });
      }
    } catch (error) { task.finish(error); }
  });
}
