const MAX_TIMER_DELAY = 2147483647;

/**
 * Keeps workers alive across tasks so each worker keeps its state, such as a RandomX cache.
 */
export function createWorkerGroup({ url, options, count, label }) {
  const workers = new Map();
  const startupErrors = [];
  let task = null;
  let lastError = new Error(`${label} worker failed`);

  const fail = (worker, error) => {
    if (worker && !workers.has(worker)) return;
    worker?.terminate();
    workers.delete(worker);
    lastError = error;
    if (task?.failures !== "first") console.warn(`${label} worker failed:`, error.message);
    if (task && (task.failures === "first" || workers.size === 0)) task.reject(error);
  };

  for (let index = 0; index < count; index += 1) {
    try {
      const worker = options ? new Worker(url, options) : new Worker(url);
      const entry = { messaged: false, index };
      workers.set(worker, entry);
      worker.onmessage = ({ data }) => {
        entry.messaged = true;
        if (!task) return;
        try {
          task.onMessage(data, {
            worker,
            index: entry.index,
            resolve: task.resolve,
            reject: task.reject,
            fail: (error) => fail(worker, error),
          });
        } catch (error) {
          task?.reject(error);
        }
      };
      worker.onerror = (event) => {
        const error = new Error(event.message || `${label} worker failed`);
        // A script that fails before its first message was blocked or could not load.
        if (!event.message && !entry.messaged) error.code = "asset_blocked";
        fail(worker, error);
      };
    } catch (error) {
      startupErrors.push(error);
      lastError = error;
    }
  }

  function run({ failures, deadline, start, onMessage }) {
    if (task) throw new Error(`${label} task already running`);
    let timer = null;
    let resolvePromise;
    let rejectPromise;
    const promise = new Promise((resolve, reject) => {
      resolvePromise = resolve;
      rejectPromise = reject;
    });
    const settle = (error, result) => {
      if (task !== current) return;
      task = null;
      if (timer !== null) clearTimeout(timer);
      if (error) rejectPromise(error);
      else resolvePromise(result);
    };
    const current = {
      failures,
      onMessage,
      resolve: (result) => settle(null, result),
      reject: (error) => settle(error),
    };
    task = current;
    const expire = () => {
      const left = deadline - Date.now();
      if (left <= 0) current.reject(new Error(`${label} challenge expired`));
      else timer = setTimeout(expire, Math.min(left, MAX_TIMER_DELAY));
    };
    if (deadline !== undefined) expire();
    if (failures === "first" && startupErrors.length) current.reject(startupErrors[0]);
    else for (const error of startupErrors) console.warn(`${label} worker failed:`, error.message);
    startupErrors.length = 0;
    if (workers.size === 0) current.reject(lastError);
    const liveCount = workers.size;
    let index = 0;
    for (const [worker, entry] of workers) {
      if (task !== current) break;
      entry.index = index++;
      try {
        start(worker, entry.index, liveCount);
      } catch (error) {
        fail(worker, error);
      }
    }
    return { promise, cancel: current.reject };
  }

  return {
    run,
    post(message) {
      for (const worker of workers.keys()) worker.postMessage(message);
    },
    size: () => workers.size,
    terminate() {
      task?.reject(new Error(`${label} workers terminated`));
      for (const worker of workers.keys()) worker.terminate();
      workers.clear();
    },
  };
}

/** Runs one task on several workers and settles once. */
export function runWorkers({ url, options, count, label, ...task }) {
  const group = createWorkerGroup({ url, options, count, label });
  const run = group.run(task);
  return { promise: run.promise.finally(() => group.terminate()), cancel: run.cancel };
}
