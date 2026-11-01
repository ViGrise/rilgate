import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";
import { challengeSource } from "./support/challenge-source.mjs";

for (const verification of ["gateway", "pool_share"]) {
  test(`RandomX ${verification} selects its proof path and reloads once per URL`, async () => {
    const source = await readFile(
      new URL("../web/static/js/solvers/randomx/adapter.js", import.meta.url),
      "utf8",
    );
    const values = new Map();
    let reloads = 0;
    let adapter;
    let failure = true;
    const unavailable = Object.assign(new Error("HTTP 503"), {
      status: 503,
      code: "work_unavailable",
    });
    const paths = [];
    const window = {
      location: {
        href: "https://example.test/protected",
        reload() {
          reloads++;
        },
      },
      POW_SOLVER_MODULE_URL: "/solver.js",
      PowClient: {
        t: (_key, text) => text,
        supportsWasm: () => true,
        status() {},
        state() {},
        stats() {},
        progressEl: { removeAttribute() {} },
        once: (_target, event, listener) => {
          if (event === "click") listener();
          return () => {};
        },
        loadSolver: async () => ({ solve: async () => ({ nonce: "1" }) }),
        stratumPoolAccepted: async () => {
          paths.push("pool_share");
          if (failure) throw unavailable;
        },
        requestChallenge: async () => {
          paths.push("gateway");
          if (failure) throw unavailable;
          return { algorithm: "randomx" };
        },
        verify: async () => {},
        register: (_id, value) => {
          adapter = value;
        },
      },
    };
    vm.runInNewContext(source, {
      window,
      Worker() {},
      WebAssembly: { validate: () => true },
      BigInt,
      document: { querySelector: () => ({}) },
      sessionStorage: {
        getItem: (key) => values.get(key),
        setItem: (key, value) => values.set(key, value),
        removeItem: (key) => values.delete(key),
      },
    });
    await adapter.run({ algorithms: [{ id: "randomx", verification }] });
    assert.equal(reloads, 1);
    await assert.rejects(
      adapter.run({ algorithms: [{ id: "randomx", verification }] }),
      (error) => error === unavailable,
    );
    assert.equal(reloads, 1);
    failure = false;
    await adapter.run({ algorithms: [{ id: "randomx", verification }] });
    assert.equal(values.has("rilgate-work-unavailable-reload"), false);
    failure = true;
    await adapter.run({ algorithms: [{ id: "randomx", verification }] });
    assert.equal(reloads, 2);
    assert.deepEqual(paths, Array(4).fill(verification));
  });
}

test("pool_share shows preparing until a job arrives and pool only after a share", async () => {
  const source = await readFile(
    new URL("../web/static/js/challenge/stratum-pool.js", import.meta.url),
    "utf8",
  );
  const events = [];
  const states = [];
  class WebSocket {
    addEventListener(type, listener) {
      if (type === "open") queueMicrotask(listener);
      if (type === "message") {
        this.onmessage = listener;
        queueMicrotask(() =>
          listener({
            data: JSON.stringify({
              type: "job",
              job: { job_id: "job", blob: "00000000", seed_hash: "00".repeat(32) },
              difficulty: 1,
              nonce_offset: 0,
            }),
          }),
        );
      }
    }
    removeEventListener() {}
    send(data) {
      const message = JSON.parse(data);
      assert.equal(message.type, "share");
      events.push(message.type);
      queueMicrotask(() =>
        this.onmessage({ data: JSON.stringify({ type: "accepted", receipt: "receipt" }) }),
      );
    }
    close() {}
  }
  const window = {
    POW_PRESENTATION_TOKEN: "presentation",
    PowClient: {
      state(name) {
        states.push(name);
        events.push(name);
      },
      endpoint: (path) => path,
      exchange: async (receipt) => assert.equal(receipt, "receipt"),
      once: (target, type, listener) => target.addEventListener(type, listener),
      randomxJob: (job) => job,
      solveRandomX: (solver, job) => solver.solve(job),
    },
  };
  vm.runInNewContext(source, {
    window,
    location: { protocol: "https:", host: "example.test" },
    WebSocket,
  });
  await window.PowClient.stratumPoolAccepted({
    solve: async () => ({ nonce: "00000000", result: "00".repeat(32) }),
  });
  assert.deepEqual(states.slice(0, 2), ["preparing", "initializing"]);
  assert.ok(events.includes("share"));
  assert.ok(events.indexOf("pool") > events.indexOf("share"));
});

test("a pool work_unavailable message reloads once and then shows unavailable", async () => {
  const values = new Map();
  let reloads = 0;
  let poolConnections = 0;
  async function openPage() {
    const elements = new Map();
    const element = (id) => {
      if (!elements.has(id)) {
        elements.set(id, {
          style: {},
          textContent: "",
          hidden: true,
          removeAttribute() {},
          removeEventListener() {},
          addEventListener(type, listener) {
            if (id === "#randomx-start" && type === "click") queueMicrotask(listener);
          },
        });
      }
      return elements.get(id);
    };
    element("#pow-bootstrap").textContent = JSON.stringify({
      algorithm: "randomx",
      internal_prefix: "/gateway",
      presentation_token: "presentation",
      solver_module_url: "/solver.js",
    });
    const location = {
      protocol: "https:",
      host: "example.test",
      href: "https://example.test/protected",
      reload() {
        reloads++;
      },
    };
    class WebSocket {
      constructor() {
        poolConnections++;
      }
      addEventListener(type, listener) {
        if (type === "open") queueMicrotask(listener);
        if (type === "message")
          queueMicrotask(() =>
            listener({
              data: JSON.stringify({
                type: "error",
                message: "Stratum pool connection unavailable",
                code: "work_unavailable",
              }),
            }),
          );
      }
      removeEventListener() {}
      close() {}
    }
    const window = { location, addEventListener() {}, removeEventListener() {} };
    const context = vm.createContext({
      window,
      location,
      WebSocket,
      Worker() {},
      BigInt,
      AbortController,
      AbortSignal,
      document: { querySelector: element },
      navigator: {},
      setTimeout,
      clearTimeout,
      console: { error() {} },
      sessionStorage: {
        getItem: (key) => values.get(key),
        setItem: (key, value) => values.set(key, value),
        removeItem: (key) => values.delete(key),
      },
      fetch: async (url) => {
        assert.ok(url.endsWith("client-config") || url.endsWith("session"));
        return {
          status: 200,
          headers: { get: () => null },
          text: async () =>
            JSON.stringify(
              url.endsWith("client-config")
                ? { algorithms: [{ id: "randomx", verification: "pool_share" }] }
                : { authenticated: false },
            ),
        };
      },
    });
    vm.runInContext(challengeSource, context);
    window.PowClient.supportsWasm = () => true;
    window.PowClient.loadSolver = async () => ({
      solve: async () => assert.fail("pool supplied no job"),
    });
    return window;
  }
  const first = await openPage();
  for (let attempt = 0; attempt < 100 && reloads === 0; attempt++) {
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  assert.equal(reloads, 1);
  assert.equal(poolConnections, 1);
  assert.notEqual(first.PowClient.currentState(), "unavailable");
  const second = await openPage();
  for (
    let attempt = 0;
    attempt < 100 && second.PowClient.currentState() !== "unavailable";
    attempt++
  ) {
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  assert.equal(second.PowClient.currentState(), "unavailable");
  assert.equal(reloads, 1);
  assert.equal(poolConnections, 2);
});
