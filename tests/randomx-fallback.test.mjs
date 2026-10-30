import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

test("RandomX reloads once per URL and clears the guard after challenge success", async () => {
  const source = await readFile(new URL("../web/static/js/solvers/randomx/adapter.js", import.meta.url), "utf8");
  const values = new Map();
  let reloads = 0;
  let adapter;
  let failure = true;
  const unavailable = Object.assign(new Error("HTTP 503"), { status: 503, code: "work_unavailable" });
  const window = {
    location: { href: "https://example.test/protected", reload() { reloads++; } },
    POW_SOLVER_MODULE_URL: "/solver.js",
    PowClient: {
      t: (_key, text) => text,
      status() {},
      progressEl: { removeAttribute() {} },
      once: (_target, event, listener) => { if (event === "click") listener(); return () => {}; },
      loadSolver: async () => ({ solve: async () => ({ nonce: "1" }) }),
      requestChallenge: async () => { if (failure) throw unavailable; return { algorithm: "randomx" }; },
      verify: async () => {},
      register: (_id, value) => { adapter = value; },
    },
  };
  vm.runInNewContext(source, {
    window, Worker() {}, WebAssembly: { validate: () => true }, BigInt,
    document: { querySelector: () => ({}) },
    sessionStorage: { getItem: key => values.get(key), setItem: (key, value) => values.set(key, value), removeItem: key => values.delete(key) },
  });
  await adapter.run({ enforce: "local_verify", algorithms: [] });
  assert.equal(reloads, 1);
  await assert.rejects(adapter.run({ enforce: "local_verify", algorithms: [] }), error => error === unavailable);
  assert.equal(reloads, 1);
  failure = false;
  await adapter.run({ enforce: "local_verify", algorithms: [] });
  assert.equal(values.has("rilgate-work-unavailable-reload"), false);
  failure = true;
  await adapter.run({ enforce: "local_verify", algorithms: [] });
  assert.equal(reloads, 2);
});
