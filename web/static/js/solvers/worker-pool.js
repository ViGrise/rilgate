const MAX_TIMER_DELAY = 2147483647;

/**
 * Runs one task on several workers and settles once.
 * failures: "first" rejects on the first worker failure; "last" rejects when no worker is left.
 */
export function runWorkers({
  url, // URL of the worker script
  options, // optional Worker constructor options
  count, // number of workers
  label, // name used in messages, for example "HashWX"
  failures, // "first" or "last"
  deadline, // optional epoch milliseconds; passing it rejects with `${label} challenge expired`
  start, // (worker, index) => void, posts the first message
  onMessage, // (data, { worker, index, resolve, reject, fail }) => void; fail(error) reports a failure of this worker
}) {
  const workers = new Set();
  let settled = false;
  let timer = null;
  let remaining = count;
  let rejectPromise;
  let resolvePromise;

  const settle = (error, result) => {
    if (settled) return;
    settled = true;
    if (timer !== null) clearTimeout(timer);
    for (const worker of workers) worker.terminate();
    workers.clear();
    if (error) rejectPromise(error);
    else resolvePromise(result);
  };
  const resolve = (result) => settle(null, result);
  const reject = (error) => settle(error);

  const fail = (worker, error) => {
    // A worker counts as failed once, even when it reports an error and then raises one.
    if (settled || (worker && !workers.has(worker))) return;
    if (failures === "first") {
      reject(error);
      return;
    }
    console.warn(`${label} worker failed:`, error.message);
    worker?.terminate();
    workers.delete(worker);
    remaining -= 1;
    if (remaining === 0) reject(error);
  };

  const promise = new Promise((resolvePending, rejectPending) => {
    resolvePromise = resolvePending;
    rejectPromise = rejectPending;
  });

  const expire = () => {
    const left = deadline - Date.now();
    if (left <= 0) reject(new Error(`${label} challenge expired`));
    else timer = setTimeout(expire, Math.min(left, MAX_TIMER_DELAY));
  };
  if (deadline !== undefined) expire();

  for (let index = 0; index < count && !settled; index += 1) {
    let worker;
    let messaged = false;
    try {
      worker = options ? new Worker(url, options) : new Worker(url);
      workers.add(worker);
      worker.onmessage = ({ data }) => {
        messaged = true;
        if (settled) return;
        try {
          onMessage(data, {
            worker,
            index,
            resolve,
            reject,
            fail: (error) => fail(worker, error),
          });
        } catch (error) {
          reject(error);
        }
      };
      worker.onerror = (event) => {
        const error = new Error(event.message || `${label} worker failed`);
        // A script that fails before its first message was blocked or could not load.
        if (!event.message && !messaged) error.code = "asset_blocked";
        fail(worker, error);
      };
      start(worker, index);
    } catch (error) {
      fail(worker, error);
    }
  }

  return { promise, cancel: reject };
}
