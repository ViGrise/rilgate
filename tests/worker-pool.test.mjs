import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const source = await readFile(
  new URL("../web/static/js/solvers/worker-pool.js", import.meta.url),
  "utf8",
);

test("a worker group keeps live workers across tasks and terminates a pending task", async () => {
  const workers = [];
  const context = vm.createContext({
    console: { warn() {} },
    Worker: class {
      constructor() {
        this.messages = [];
        workers.push(this);
      }
      postMessage(message) {
        this.messages.push(message);
      }
      terminate() {
        this.terminated = true;
      }
    },
  });
  vm.runInContext(source.replaceAll("export ", ""), context);
  const group = context.createWorkerGroup({ url: "worker.js", count: 3, label: "Test" });
  const starts = [];
  const task = {
    failures: "last",
    start: (worker, index, liveCount) => starts.push([worker, index, liveCount]),
    onMessage: (data, { resolve, fail }) => {
      if (data === "fail") fail(new Error("Worker failed"));
      else resolve(data);
    },
  };
  const first = group.run(task);
  group.post("hello");
  assert.ok(workers.every((worker) => worker.messages[0] === "hello"));
  workers[0].onmessage({ data: "fail" });
  assert.equal(workers[0].terminated, true);
  workers[1].onmessage({ data: "first" });
  assert.equal(await first.promise, "first");
  starts.length = 0;
  const second = group.run(task);
  assert.equal(workers.length, 3);
  assert.deepEqual(starts, [
    [workers[1], 0, 2],
    [workers[2], 1, 2],
  ]);
  assert.equal(group.size(), 2);
  group.post("again");
  assert.deepEqual(
    workers.map((worker) => worker.messages),
    [["hello"], ["hello", "again"], ["hello", "again"]],
  );
  workers[2].onmessage({ data: "second" });
  assert.equal(await second.promise, "second");
  const third = group.run(task);
  group.terminate();
  await assert.rejects(third.promise, /Test workers terminated/);
  assert.equal(group.size(), 0);
  assert.ok(workers.every((worker) => worker.terminated));
});
