import assert from "node:assert/strict";
import {challengeSource as source} from "./support/challenge-source.mjs";
import test from "node:test";
import vm from "node:vm";


function page(fetch, failPost = false, translations = null, embedded = false) {
  const elements = new Map();
  const listeners = new Map();
  const workers = [];
  const requests = [];
  let reloads = 0;
  const messages = [];
  const element = (id) => {
    if (!elements.has(id)) {
      elements.set(id, { hidden: true, textContent: "", removeAttribute() {}, addEventListener(type, handler) { this[type] = handler; } });
    }
    return elements.get(id);
  };
  if (translations) { element("#pow-i18n").textContent = JSON.stringify(translations); }
  class Worker {
    constructor() { workers.push(this); }
    terminate() { this.terminated = true; }
    postMessage(message) {
      if (failPost) { throw new Error("post failed"); }
      this.request = structuredClone(message);
    }
  }
  const context = vm.createContext({
    AbortSignal, DOMException, Worker,
    document: { querySelector: element },
    window: {
      POW_EMBEDDED: embedded, parent: { postMessage: (message, origin) => messages.push({ message, origin }) },
      POW_INTERNAL_PREFIX: "/gateway", POW_ALGORITHM: "fast", POW_PRESENTATION_TOKEN: "presentation",
      addEventListener(type, handler) { listeners.set(type, handler); },
      removeEventListener(type) { listeners.delete(type); },
    },
    location: { origin: "https://example.test", reload() { reloads += 1; } },
    console: { error() {} },
    fetch: async (url, options) => {
      requests.push({ url, options });
      return fetch(url, options);
    },
  });
  vm.runInContext(source, context);
  return { element, workers, listeners, requests, messages, reloads: () => reloads };
}

async function until(predicate) {
  for (let index = 0; index < 100; index += 1) {
    if (predicate()) { return; }
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  assert.fail("page did not reach expected state");
}

function response(body) { return { ok: true, json: async () => body }; }
function challengeFetch(url) {
  return response(url.endsWith("client-config") ? { enforce: "local_verify" } : { algorithm: "fast", challenge: "data", difficulty: 1, challenge_token: "challenge" });
}

test("failed API calls expose a retry action and carry a bounded abort signal", async () => {
  const state = page(async () => { throw new Error("network unavailable"); });
  await until(() => !state.element("#retry").hidden);
  assert.match(state.element("#status").textContent, /network unavailable/);
  const options = state.requests[0].options;
  assert.ok(options.signal instanceof AbortSignal);
  assert.equal(options.cache, "no-store");
  state.element("#retry").click();
  assert.equal(state.reloads(), 1);
});

test("leaving the page terminates fast proof work and removes its lifecycle listener", async () => {
  const state = page(async (url) => challengeFetch(url));
  await until(() => state.workers[0]?.request);
  state.listeners.get("pagehide")();
  await until(() => !state.element("#retry").hidden);
  assert.equal(state.workers[0].terminated, true);
  assert.equal(state.listeners.has("pagehide"), false);
  assert.equal(state.workers[0].onmessage, null);
  assert.equal(state.requests.length, 2);
});

test("postMessage failures terminate the worker and allow the user to retry", async () => {
  const state = page(async (url) => challengeFetch(url), true);
  await until(() => !state.element("#retry").hidden);
  assert.equal(state.workers[0].terminated, true);
  assert.equal(state.listeners.has("pagehide"), false);
  assert.match(state.element("#status").textContent, /post failed/);
});

test("completed fast proofs clean up before exchanging the receipt and reloading", async () => {
  const state = page(async (url, options) => {
    if (url.endsWith("verify")) {
      assert.equal(state.workers[0].terminated, true);
      assert.equal(state.listeners.has("pagehide"), false);
      assert.deepEqual(JSON.parse(options.body), { challenge_token: "challenge", nonce: "153" });
      return response({ receipt: "receipt" });
    }
    if (url.endsWith("exchange")) { return response({ status: "ok" }); }
    return challengeFetch(url);
  });
  await until(() => state.workers[0]?.request);
  state.workers[0].onmessage({ data: { type: "solved", nonce: "153" } });
  await until(() => state.reloads() === 1);
  assert.equal(state.element("#retry").hidden, true);
  assert.equal(state.requests.length, 4);
});

test("rate-limit failures display the server's retry delay", async () => {
  const state = page(async () => ({
    ok: false, status: 429, headers: { get: () => "10" },
    json: async () => ({ message: "Too many proof-of-work requests" }),
  }));
  await until(() => !state.element("#retry").hidden);
  assert.match(state.element("#status").textContent, /Try again in 10 seconds/);
  assert.equal(state.reloads(), 0);
});

test("server-rendered translations localize statuses and named placeholders", async () => {
  const state = page(async (url) => {
    if (url.endsWith("verify")) { return response({ receipt: "receipt" }); }
    if (url.endsWith("exchange")) { return response({ status: "ok" }); }
    return challengeFetch(url);
  }, false, {
    "challenge.status.fast_difficulty": "Travail {difficulty}",
    "challenge.success": "Vérification terminée",
  });
  await until(() => state.workers[0]?.request);
  assert.equal(state.element("#status").textContent, "Travail 1");
  state.workers[0].onmessage({ data: { type: "solved", nonce: "153" } });
  await until(() => state.reloads() === 1);
  assert.equal(state.element("#status").textContent, "Vérification terminée");
});

test("embedded completion notifies only the same-origin parent without reloading", async () => {
  const state = page(async url => {
    if (url.endsWith("verify")) return response({ receipt: "receipt" });
    if (url.endsWith("exchange")) return response({ status: "ok" });
    return challengeFetch(url);
  }, false, null, true);
  await until(() => state.workers[0]?.request);
  state.workers[0].onmessage({ data: { type: "solved", nonce: "153" } });
  await until(() => state.messages.length === 1);
  assert.deepEqual(JSON.parse(JSON.stringify(state.messages)), [{ message: { type: "rilgate:verified" }, origin: "https://example.test" }]);
  assert.equal(state.reloads(), 0);
});
