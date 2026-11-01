import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";
import { challengeSource } from "./support/challenge-source.mjs";

const runtime = await readFile(
  new URL("../web/static/js/challenge/runtime.js", import.meta.url),
  "utf8",
);

function loadReloadAllowed(context) {
  const client = context.window.PowClient;
  const document = context.document;
  context.document = {
    querySelector: (selector) => (selector === "#pow-bootstrap" ? { textContent: "{}" } : null),
  };
  vm.runInContext(runtime, context);
  client.reloadAllowed = context.window.PowClient.reloadAllowed;
  client.reloadPage = context.window.PowClient.reloadPage;
  context.window.PowClient = client;
  context.document = document;
}

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
          values.delete("rilgate-work-unavailable-reload");
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
    const context = vm.createContext({
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
    loadReloadAllowed(context);
    vm.runInContext(source, context);
    const reload = { name: "PageReload" };
    await assert.rejects(adapter.run({ algorithms: [{ id: "randomx", verification }] }), reload);
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
    await assert.rejects(adapter.run({ algorithms: [{ id: "randomx", verification }] }), reload);
    assert.equal(reloads, 2);
    assert.deepEqual(paths, Array(4).fill(verification));
  });
}

test("pool_share shows connecting until a job arrives and pool only after a share", async () => {
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
  assert.deepEqual(states.slice(0, 2), ["connecting", "initializing"]);
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

async function resumePage({ closeBeforeJob = false, reloadUsed = false } = {}) {
  const source = await readFile(
    new URL("../web/static/js/challenge/stratum-pool.js", import.meta.url),
    "utf8",
  );
  const sockets = [];
  const states = [];
  const delays = [];
  const values = new Map(
    reloadUsed ? [["rilgate-pool-resume-reload", "https://example.test/"]] : [],
  );
  let reloads = 0;
  let cancels = 0;
  const jobs = [];
  class WebSocket {
    constructor(url) {
      this.url = url;
      this.listeners = new Map();
      sockets.push(this);
      queueMicrotask(() => this.emit("open"));
    }
    emit(type, event) {
      for (const listener of [...(this.listeners.get(type) || [])]) listener(event);
    }
    addEventListener(type, listener) {
      if (!this.listeners.has(type)) this.listeners.set(type, new Set());
      this.listeners.get(type).add(listener);
      if (type === "message")
        queueMicrotask(() => {
          if (closeBeforeJob) this.close();
          else
            this.emit("message", { data: JSON.stringify({ type: "job", job: { job_id: "job" } }) });
        });
    }
    removeEventListener(type, listener) {
      this.listeners.get(type)?.delete(listener);
    }
    send() {
      if (sockets.length === 1) this.close();
      else
        queueMicrotask(() =>
          this.emit("message", {
            data: JSON.stringify({ type: "accepted", receipt: "receipt" }),
          }),
        );
    }
    close() {
      this.emit("close");
    }
  }
  const window = {
    location: { href: "https://example.test/", reload: () => reloads++ },
    POW_PRESENTATION_TOKEN: "presentation",
    PowClient: {
      state: (name) => states.push(name),
      endpoint: (path) => path,
      exchange: async (receipt) => assert.equal(receipt, "receipt"),
      once(target, type, listener) {
        const handle = (event) => {
          target.removeEventListener(type, handle);
          listener(event);
        };
        target.addEventListener(type, handle);
      },
      randomxJob: (job) => job,
      solveRandomX: (solver, job) => solver.solve(job),
    },
  };
  const context = vm.createContext({
    window,
    location: { protocol: "https:", host: "example.test" },
    WebSocket,
    sessionStorage: {
      getItem: (key) => values.get(key),
      setItem: (key, value) => values.set(key, value),
      removeItem: (key) => values.delete(key),
    },
    setTimeout: (callback, delay) => {
      delays.push(delay);
      queueMicrotask(callback);
    },
  });
  loadReloadAllowed(context);
  vm.runInContext(source, context);
  const solver = {
    async solve(job) {
      jobs.push(job);
      return { nonce: "00000000", result: "00".repeat(32) };
    },
    cancel() {
      cancels++;
    },
  };
  const promise = window.PowClient.stratumPoolAccepted(solver);
  return {
    promise,
    sockets,
    states,
    delays,
    jobs,
    values,
    reloads: () => reloads,
    cancels: () => cancels,
  };
}

test("a closed pool session resumes with the same solver and shows connecting again", async () => {
  const page = await resumePage();
  page.values.set("rilgate-work-unavailable-reload", "https://example.test/");
  page.values.set("rilgate-pool-resume-reload", "https://example.test/");
  await page.promise;
  assert.equal(page.sockets.length, 2);
  assert.equal(page.sockets[0].url, page.sockets[1].url);
  assert.equal(page.jobs.length, 2);
  assert.equal(page.states.filter((state) => state === "connecting").length, 2);
  assert.deepEqual(page.delays, [1000]);
  assert.equal(page.cancels(), 1);
  assert.equal(page.values.size, 0);
});

test("five sessions without a job reload once and then report a resume failure", async () => {
  for (const reloadUsed of [false, true]) {
    const page = await resumePage({ closeBeforeJob: true, reloadUsed });
    if (reloadUsed) await assert.rejects(page.promise, /RandomX pool session could not resume/);
    else await assert.rejects(page.promise, { name: "PageReload" });
    assert.equal(page.sockets.length, 5);
    assert.equal(page.reloads(), reloadUsed ? 0 : 1);
    assert.deepEqual(page.delays, [1000, 2000, 4000, 8000]);
    assert.equal(page.cancels(), 1);
  }
});
