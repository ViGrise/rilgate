import { readFileSync } from "node:fs";
const english = JSON.parse(readFileSync(new URL("../web/i18n/en.json", import.meta.url), "utf8"));
import assert from "node:assert/strict";
import { challengeSource as source } from "./support/challenge-source.mjs";
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
      elements.set(id, {
        style: {},
        hidden: true,
        textContent: "",
        removeAttribute() {},
        removeEventListener() {},
        addEventListener(type, handler) {
          this[type] = handler;
        },
      });
    }
    return elements.get(id);
  };
  element("#pow-i18n").textContent = JSON.stringify({ ...english, ...translations });
  const bootstrap = {
    internal_prefix: "/gateway",
    algorithm: "fast",
    presentation_token: "presentation",
    embedded,
    solver_module_url: "/gateway/static/js/solvers/fast/solver.js",
  };
  element("#pow-bootstrap").textContent = JSON.stringify(bootstrap);
  class Worker {
    constructor() {
      workers.push(this);
    }
    terminate() {
      this.terminated = true;
    }
    postMessage(message) {
      if (failPost) {
        throw new Error("post failed");
      }
      this.request = structuredClone(message);
    }
  }
  const context = vm.createContext({
    AbortSignal,
    AbortController,
    DOMException,
    Worker,
    setTimeout,
    clearTimeout,
    navigator: {},
    document: { querySelector: element },
    window: {
      parent: { postMessage: (message, origin) => messages.push({ message, origin }) },
      addEventListener(type, handler) {
        listeners.set(type, handler);
      },
      removeEventListener(type) {
        listeners.delete(type);
      },
    },
    location: {
      origin: "https://example.test",
      reload() {
        reloads += 1;
      },
    },
    console: { error() {} },
    fetch: async (url, options) => {
      requests.push({ url, options });
      return fetch(url, options);
    },
  });
  vm.runInContext(source, context);
  for (const [name, value] of Object.entries(bootstrap)) {
    assert.equal(context.window["POW_" + name.toUpperCase()], value);
  }
  return { element, workers, listeners, requests, messages, reloads: () => reloads };
}

async function until(predicate) {
  for (let index = 0; index < 100; index += 1) {
    if (predicate()) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  assert.fail("page did not reach expected state");
}

function response(body) {
  return { status: 200, headers: { get: () => null }, text: async () => JSON.stringify(body) };
}
function challengeFetch(url) {
  if (url.endsWith("session")) return response({ authenticated: true });
  return response(
    url.endsWith("client-config")
      ? { enforce: "local_verify" }
      : { algorithm: "fast", challenge: "data", difficulty: 1, challenge_token: "challenge" },
  );
}

test("failed API calls expose a retry action and carry a bounded abort signal", async () => {
  const state = page(async () => {
    throw new Error("network unavailable");
  });
  await until(() => !state.element("#retry").hidden);
  assert.match(state.element("#status").textContent, /You can try again/);
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
  assert.match(state.element("#status").textContent, /You can try again/);
});

test("completed fast proofs fall back to exchange when cookie_set is missing", async () => {
  const state = page(async (url) => {
    if (url.endsWith("verify")) {
      assert.equal(state.workers[0].terminated, true);
      assert.equal(state.listeners.has("pagehide"), false);
      return response({ receipt: "receipt" });
    }
    if (url.endsWith("exchange")) {
      return response({ status: "ok" });
    }
    return challengeFetch(url);
  });
  await until(() => state.workers[0]?.request);
  state.workers[0].onmessage({ data: { type: "solved", nonce: "153" } });
  await until(() => state.reloads() === 1);
  assert.equal(state.element("#retry").hidden, true);
  assert.equal(state.requests.length, 5);
  assert.deepEqual(
    JSON.parse(state.requests.find(({ url }) => url.endsWith("exchange")).options.body),
    { receipt: "receipt" },
  );
  assert.deepEqual(
    JSON.parse(state.requests.find(({ url }) => url.endsWith("verify")).options.body),
    { challenge_token: "challenge", nonce: "153", set_cookie: true },
  );
});

test("rate-limit failures display the server's retry delay", async () => {
  const state = page(async () => ({
    ok: false,
    status: 429,
    headers: { get: () => "10" },
    text: async () => JSON.stringify({ message: "Too many proof-of-work requests" }),
  }));
  await until(() => state.element("#status").textContent.includes("Try again in 10 seconds"));
  assert.equal(state.element("#retry").hidden, true);
  assert.match(state.element("#status").textContent, /Try again in 10 seconds/);
  assert.equal(state.reloads(), 0);
});

test("server-rendered translations localize state messages", async () => {
  const state = page(
    async (url) => {
      if (url.endsWith("verify")) {
        return response({ receipt: "receipt" });
      }
      if (url.endsWith("exchange")) {
        return response({ status: "ok" });
      }
      return challengeFetch(url);
    },
    false,
    {
      "challenge.state.fast.status": "Travail",
      "challenge.state.success.status": "Vérification terminée",
    },
  );
  await until(() => state.workers[0]?.request);
  assert.equal(state.element("#status").textContent, "Travail");
  state.workers[0].onmessage({ data: { type: "solved", nonce: "153" } });
  await until(() => state.reloads() === 1);
  assert.equal(state.element("#status").textContent, "Vérification terminée");
});

test("embedded completion notifies only the same-origin parent without reloading", async () => {
  const state = page(
    async (url) => {
      if (url.endsWith("verify")) return response({ receipt: "receipt" });
      if (url.endsWith("exchange")) return response({ status: "ok" });
      return challengeFetch(url);
    },
    false,
    null,
    true,
  );
  await until(() => state.workers[0]?.request);
  state.workers[0].onmessage({ data: { type: "solved", nonce: "153" } });
  await until(() => state.messages.length === 1);
  assert.deepEqual(JSON.parse(JSON.stringify(state.messages)), [
    { message: { type: "rilgate:verified" }, origin: "https://example.test" },
  ]);
  assert.equal(state.reloads(), 0);
});

test("completed fast proofs skip exchange when verify sets the cookie", async () => {
  const state = page(async (url) => {
    if (url.endsWith("verify")) {
      return response({ receipt: "receipt", cookie_set: true });
    }
    if (url.endsWith("exchange")) {
      return response({ status: "ok" });
    }
    return challengeFetch(url);
  });
  await until(() => state.workers[0]?.request);
  state.workers[0].onmessage({ data: { type: "solved", nonce: "153" } });
  await until(() => state.reloads() === 1);
  assert.equal(state.requests.filter(({ url }) => url.endsWith("exchange")).length, 0);
});
