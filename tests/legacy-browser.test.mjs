import { readFileSync } from "node:fs";
const english = JSON.parse(readFileSync(new URL("../web/i18n/en.json", import.meta.url), "utf8"));
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";
import { challengeSource } from "./support/challenge-source.mjs";

const promiseSource = await readFile(
  new URL("../web/static/js/challenge/promise.js", import.meta.url),
  "utf8",
);
const workerSource = await readFile(
  new URL("../web/static/js/solvers/fast/worker.js", import.meta.url),
  "utf8",
);

function polyfill() {
  const context = vm.createContext({ Promise: undefined, setTimeout, clearTimeout });
  context.window = context;
  vm.runInContext(promiseSource, context);
  return context.Promise;
}

test("Promise fallback assimilates thenables once, chains errors and schedules handlers asynchronously", async () => {
  // biome-ignore lint/suspicious/noShadowRestrictedNames: the test exercises the polyfill under the global name
  const Promise = polyfill();
  let synchronous = true;
  const result = Promise.resolve({
    // biome-ignore lint/suspicious/noThenProperty: the test needs a thenable
    then(resolve, reject) {
      resolve(7);
      reject(new Error("ignored"));
    },
  })
    .then((value) => {
      assert.equal(synchronous, false);
      return value + 1;
    })
    .then(() => {
      throw new Error("chained");
    })
    .catch((error) => error.message);
  synchronous = false;
  assert.equal(await result, "chained");
  assert.deepEqual(Array.from(await Promise.all([Promise.resolve(1), 2])), [1, 2]);
  assert.deepEqual(Array.from(await Promise.all([])), []);
  assert.equal(await Promise.race([Promise.resolve("first"), new Promise(() => {})]), "first");
  let resolve;
  const circular = new Promise((done) => {
    resolve = done;
  });
  resolve(circular);
  await assert.rejects(circular, /cannot resolve itself/);
  await assert.rejects(
    Promise.resolve(
      // biome-ignore lint/suspicious/noThenProperty: the test needs a thenable with a throwing getter
      Object.defineProperty({}, "then", {
        get() {
          throw new Error("getter");
        },
      }),
    ),
    /getter/,
  );
  const context = vm.createContext({ Promise: globalThis.Promise });
  context.window = context;
  vm.runInContext(promiseSource, context);
  assert.equal(context.Promise, globalThis.Promise);
});

test("fast UTF-8 fallback matches SHA-256 vectors including surrogate replacement", () => {
  const context = vm.createContext({ TextEncoder: undefined, self: {}, setTimeout, clearTimeout });
  vm.runInContext(`${workerSource}\nself.hash = sha256;`, context);
  for (const value of [
    "",
    "abc",
    "Rilgate:123",
    "Tiếng Việt",
    "😀",
    "\ud800",
    "\udc00",
    "a\ud800b",
    "\ud800\ud800\udc00",
  ]) {
    const actual = Buffer.from(context.self.hash(value)).toString("hex");
    assert.equal(actual, createHash("sha256").update(value).digest("hex"), JSON.stringify(value));
  }
});

test("main-thread fast work yields progress and cancellation clears the next slice", () => {
  const pending = new Map();
  let id = 0;
  let now = 0;
  const context = vm.createContext({
    TextEncoder: undefined,
    PowClient: {},
    Date: { now: () => (now += 10) },
    setTimeout(callback) {
      pending.set(++id, callback);
      return id;
    },
    clearTimeout(id) {
      pending.delete(id);
    },
  });
  context.window = context;
  vm.runInContext(workerSource, context);
  const reports = [];
  const cancel = context.PowClient.startFastWork(
    { challenge: "yielding", difficulty: 64 },
    (message) => reports.push(message),
  );
  const [key, run] = pending.entries().next().value;
  pending.delete(key);
  run();
  assert.equal(reports[0].type, "progress");
  assert.ok(reports[0].attempts >= 128);
  assert.equal(pending.size, 1);
  cancel();
  assert.equal(pending.size, 0);
});

async function until(predicate) {
  for (let attempt = 0; attempt < 200; attempt++) {
    if (predicate()) return;
    await new globalThis.Promise((resolve) => setTimeout(resolve, 5));
  }
  assert.fail("challenge did not settle");
}

function legacyPage(failureStatus = 200, randomx = null, algorithm = randomx ? "randomx" : "fast") {
  const elements = new Map();
  const requests = [];
  const errors = [];
  let reloads = 0;
  function element(id) {
    if (!elements.has(id))
      elements.set(id, {
        style: {},
        hidden: true,
        textContent: "",
        removeAttribute() {},
        addEventListener(type, handler) {
          this[type] = handler;
        },
        removeEventListener(type) {
          delete this[type];
        },
      });
    return elements.get(id);
  }
  element("#pow-i18n").textContent = JSON.stringify(english);
  element("#pow-bootstrap").textContent = JSON.stringify({
    algorithm,
    internal_prefix: "/gate",
    presentation_token: "presentation",
    embedded: false,
    solver_module_url: "/gate/static/js/solvers/randomx/solver.js",
  });
  class XMLHttpRequest {
    headers = {};
    open(method, url) {
      this.method = method;
      this.url = url;
    }
    setRequestHeader(name, value) {
      this.headers[name] = value;
    }
    getResponseHeader(name) {
      return name === "retry-after" ? "30" : null;
    }
    send(body) {
      requests.push({
        url: this.url,
        method: this.method,
        body,
        timeout: this.timeout,
        headers: this.headers,
      });
      this.status = failureStatus;
      this.responseText = JSON.stringify(
        this.url.endsWith("client-config")
          ? { algorithms: [{ id: algorithm, verification: "gateway" }] }
          : this.url.endsWith("challenge")
            ? {
                algorithm: "fast",
                challenge: "fallback-😀",
                difficulty: 1,
                challenge_token: "signed-challenge",
              }
            : this.url.endsWith("verify")
              ? { receipt: "signed-receipt" }
              : this.url.endsWith("session")
                ? { authenticated: true }
                : {},
      );
      setTimeout(() => this.onload(), 0);
    }
  }
  const context = vm.createContext({
    Promise: undefined,
    Worker: undefined,
    TextEncoder: undefined,
    fetch: undefined,
    AbortController: undefined,
    AbortSignal: undefined,
    XMLHttpRequest,
    setTimeout,
    clearTimeout,
    navigator: {},
    addEventListener() {},
    removeEventListener() {},
    location: {
      reload() {
        reloads++;
      },
    },
    document: {
      querySelector: element,
      createElement() {
        return {};
      },
      head: {
        appendChild(script) {
          setTimeout(() => script.onerror(), 0);
        },
      },
    },
    console: {
      error(error) {
        errors.push(error);
      },
    },
  });
  context.window = context;
  if (randomx) {
    context.Worker = function () {
      assert.fail("incapable RandomX must not create a worker");
    };
    context.WebAssembly = randomx === "unsupported" ? undefined : WebAssembly;
    context.HTMLScriptElement = Object.assign(function () {}, { prototype: { noModule: false } });
    element("#pow-i18n").textContent = JSON.stringify({
      ...english,
      "challenge.randomx_unsupported": "Localized RandomX requirements",
    });
  }
  if (algorithm === "hashwx") {
    context.WebAssembly = undefined;
    element("#pow-i18n").textContent = JSON.stringify({
      ...english,
      "challenge.hashwx_unsupported": "Localized HashWX requirements",
    });
  }
  vm.runInContext(
    "Object.hasOwn = undefined; Number.isFinite = undefined; Number.isInteger = undefined; Map = undefined; Symbol = undefined; Iterator = undefined; URL = undefined; Object.assign = undefined; Array.prototype.includes = undefined; String.prototype.startsWith = undefined;",
    context,
  );
  vm.runInContext(challengeSource, context);
  return { elements, requests, errors, reloads: () => reloads };
}

test("fast page completes over XHR with Promise, workers, TextEncoder and modern helpers absent", async () => {
  const state = legacyPage();
  await until(() => state.reloads() === 1);
  assert.deepEqual(state.errors, []);
  assert.equal(state.requests.length, 5);
  const verify = state.requests.find((request) => request.url.endsWith("verify"));
  const proof = JSON.parse(verify.body);
  assert.equal(proof.challenge_token, "signed-challenge");
  assert.match(createHash("sha256").update(`fallback-😀${proof.nonce}`).digest("hex"), /^0/);
  assert.deepEqual(JSON.parse(state.requests[3].body), { receipt: "signed-receipt" });
  assert.equal(state.requests[1].timeout, 30000);
  assert.equal(state.requests[1].headers["content-type"], "application/json");
});

test("XHR errors retain Retry-After handling and expose retry controls", async () => {
  const state = legacyPage(429);
  await until(() => state.elements.get("#status")?.textContent.includes("Try again in 30 seconds"));
  assert.equal(state.elements.get("#retry").hidden, true);
  assert.equal(state.reloads(), 0);
});

test("RandomX without WebAssembly fails after consent through localized retry controls", async () => {
  const state = legacyPage(200, "unsupported");
  await until(() => state.elements.get("#randomx-start")?.hidden === false);
  assert.equal(state.errors.length, 0);
  state.elements.get("#randomx-start").click();
  await until(
    () =>
      state.elements.get("#status")?.textContent === english["challenge.state.unsupported.status"],
  );
  assert.equal(state.elements.get("#retry").hidden, true);
  assert.equal(state.requests.length, 1);
  assert.equal(state.reloads(), 0);
});

test("RandomX module load failures reach retry controls without requesting fast work", async () => {
  const state = legacyPage(200, "module-error");
  await until(() => state.elements.get("#randomx-start")?.hidden === false);
  state.elements.get("#randomx-start").click();
  await until(() => state.elements.get("#retry")?.hidden === false);
  assert.equal(
    state.elements.get("#status").textContent,
    english["challenge.state.blocked.status"],
  );
  assert.equal(state.requests.length, 1);
  assert.equal(state.reloads(), 0);
});

test("standalone generated fast worker solves with no TextEncoder or Promise", async () => {
  const source = await readFile(
    new URL("../crates/rilgate-web/generated/solvers/fast/worker.js", import.meta.url),
    "utf8",
  );
  const messages = [];
  const context = vm.createContext({
    TextEncoder: undefined,
    Promise: undefined,
    setTimeout,
    clearTimeout,
    self: {
      postMessage(message) {
        messages.push(message);
      },
    },
  });
  vm.runInContext(source, context);
  context.self.onmessage({ data: { challenge: "worker-😀", difficulty: 1, start: 0 } });
  await until(() => messages.some((message) => message.type === "solved"));
  const solution = messages.find((message) => message.type === "solved");
  assert.match(createHash("sha256").update(`worker-😀${solution.nonce}`).digest("hex"), /^0/);
});

test("HashWX capability failure uses localized retry controls without requesting other work", async () => {
  const state = legacyPage(200, null, "hashwx");
  await until(
    () =>
      state.elements.get("#status")?.textContent === english["challenge.state.unsupported.status"],
  );
  assert.equal(state.elements.get("#retry").hidden, true);
  assert.equal(state.requests.length, 1);
  assert.equal(state.reloads(), 0);
});
