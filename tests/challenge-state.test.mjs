import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const runtime = readFileSync(
  new URL("../web/static/js/challenge/runtime.js", import.meta.url),
  "utf8",
);
const startup = readFileSync(new URL("../web/static/js/challenge.js", import.meta.url), "utf8");
const boot = readFileSync(new URL("../web/static/js/challenge/boot.js", import.meta.url), "utf8");
const english = JSON.parse(readFileSync(new URL("../web/i18n/en.json", import.meta.url), "utf8"));
function page(ids = ["status"], bootstrap = {}, extra = {}) {
  const elements = new Map(
    ids.map((id) => [
      "#" + id,
      {
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
      },
    ]),
  );
  elements.set("#pow-i18n", { textContent: JSON.stringify(english) });
  elements.set("#pow-bootstrap", {
    textContent: JSON.stringify({
      internal_prefix: "/gateway",
      algorithm: "fast",
      presentation_token: "presentation",
      embedded: false,
      solver_module_url: "/solver.js",
      ...bootstrap,
    }),
  });
  const context = vm.createContext({
    document: { documentElement: { lang: "en" }, querySelector: (id) => elements.get(id) || null },
    window: { addEventListener() {} },
    navigator: {},
    console: { error() {} },
    TypeError,
    ...extra,
  });
  vm.runInContext(runtime, context);
  return { context, client: context.window.PowClient, elements };
}

test("custom templates can omit new elements and retain status updates", () => {
  const { client, elements } = page();
  client.state("verifying");
  client.stats(1234, 56);
  assert.equal(elements.get("#status").textContent, english["challenge.state.verifying.status"]);
  client.status("Operator status");
  assert.equal(elements.get("#status").textContent, "Operator status");
});

test("asset paths use the static URL from the page bootstrap", () => {
  const { client } = page(["status"], { static_url: "/gateway/static/v-3f2a9c01b7de" });
  assert.equal(client.asset("/js/x.js"), "/gateway/static/v-3f2a9c01b7de/js/x.js");
  assert.equal(page().client.asset("/js/x.js"), "/gateway/static/js/x.js");
});

test("statistics use page locale and remain visible through result confirmation", () => {
  const { client, elements } = page(["status", "stats", "attempts", "hashrate"]);
  client.state("fast");
  client.stats(1234, 56);
  assert.equal(elements.get("#attempts").textContent, "1,234");
  assert.equal(elements.get("#hashrate").textContent, "56");
  for (const state of ["verifying", "pool", "success"]) {
    client.state(state);
    assert.equal(elements.get("#stats").hidden, false);
  }
  client.state("failed");
  assert.equal(elements.get("#stats").hidden, true);
});

for (const [name, error] of [
  ["expired", new Error("RandomX challenge expired. Retry to request new work.")],
  ["expired", new Error("HashWX challenge expired")],
  ["expired", new Error("deadline exceeded")],
  ["network", new TypeError("Failed to fetch")],
  [
    "limited",
    Object.assign(new Error("HTTP 429"), { status: 429, retryAfter: " Try again in 7 seconds." }),
  ],
  ["unavailable", Object.assign(new Error("HTTP 503"), { status: 503 })],
  ["unsupported", Object.assign(new Error("Unsupported browser"), { code: "unsupported_browser" })],
  ["failed", new Error("Private diagnostic details")],
]) {
  test(`startup maps ${error.message} to ${name}`, async () => {
    const { client, context, elements } = page(["status", "retry"]);
    client.jsonFetch = async () => {
      throw error;
    };
    vm.runInContext(startup, context);
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(
      elements.get("#status").textContent,
      english[`challenge.state.${name}.status`] + (error.retryAfter || ""),
    );
    assert.equal(
      elements.get("#retry").hidden,
      ["expired", "network", "unavailable", "failed"].includes(name) ? false : true,
    );
    assert.ok(!elements.get("#status").textContent.includes("Private diagnostic"));
  });
}

test("the bar keeps running across work states and fills only on success", () => {
  const { client, context, elements } = page([
    "bar",
    "bar-shimmer",
    "bar-complete",
    "randomx-start",
    "retry",
  ]);
  let restarts = 0;
  Object.defineProperty(elements.get("#bar-shimmer").style, "animation", {
    set(value) {
      if (value === "none") restarts++;
    },
  });
  const frames = [];
  context.requestAnimationFrame = (callback) => frames.push(callback);
  client.state("preparing");
  client.state("fast");
  client.state("verifying");
  assert.equal(restarts, 1);
  assert.equal(elements.get("#bar").hidden, false);
  assert.equal(elements.get("#bar-complete").style.transform, "scaleX(0)");
  client.state("success");
  frames.shift()();
  frames.shift()();
  assert.equal(elements.get("#bar-shimmer").hidden, true);
  assert.equal(elements.get("#bar-complete").style.transform, "scaleX(1)");
  client.state("consent");
  assert.equal(elements.get("#bar").hidden, true);
  assert.equal(elements.get("#randomx-start").hidden, false);
  context.matchMedia = () => ({ matches: true });
  client.state("initializing");
  client.state("success");
  assert.equal(restarts, 2);
  assert.equal(frames.length, 0);
  assert.equal(elements.get("#bar-complete").style.transform, "scaleX(1)");
});

test("XHR network failures reach the network state", async () => {
  const { context, elements } = page(["status", "retry"]);
  context.XMLHttpRequest = class {
    open() {}
    setRequestHeader() {}
    send() {
      this.onerror();
    }
  };
  vm.runInContext(startup, context);
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(elements.get("#status").textContent, english["challenge.state.network.status"]);
  assert.equal(elements.get("#retry").hidden, false);
});

test("a cookie that does not reach the gateway shows the cookies state", async () => {
  for (const embedded of [false, true]) {
    for (const cookieEnabled of [true, false]) {
      const { client, context, elements } = page(["status", "retry"], { embedded });
      let fetches = 0;
      let reloads = 0;
      let messages = 0;
      context.navigator.cookieEnabled = cookieEnabled;
      context.location = {
        reload() {
          reloads++;
        },
      };
      context.window.parent = {
        postMessage() {
          messages++;
        },
      };
      context.window.opener = {
        postMessage() {
          messages++;
        },
      };
      client.jsonFetch = async (url) => {
        fetches++;
        return url.endsWith("/api/session") ? { authenticated: false } : {};
      };
      client.algorithm = () => ({ async run() {} });
      vm.runInContext(startup, context);
      await new Promise((resolve) => setImmediate(resolve));
      assert.equal(elements.get("#status").textContent, english["challenge.state.cookies.status"]);
      assert.equal(elements.get("#retry").hidden, false);
      assert.equal(reloads, 0);
      assert.equal(messages, 0);
      assert.equal(fetches, cookieEnabled ? 2 : 0);
      elements.get("#retry").click();
      assert.equal(reloads, 1);
    }
  }
});

function bootPage(window = {}) {
  const listeners = {};
  const root = { className: "" };
  const context = vm.createContext({
    window: { addEventListener: (type, handler) => (listeners[type] = handler), ...window },
    document: {
      documentElement: root,
      createElement: () => ({}),
      getElementById: () => null,
      querySelector: () => null,
    },
    location: { reload() {} },
    console: { error() {} },
  });
  return { context, listeners, root };
}

test("a failed runtime shows the load error instead of hanging", async () => {
  const { context, root } = bootPage();
  vm.runInContext(boot, context);
  assert.doesNotThrow(() => vm.runInContext(startup, context));
  await new Promise((resolve) => setImmediate(resolve));
  assert.match(root.className, /pow-load-failed/);
});

test("a duplicate bundle stops before it runs twice", () => {
  const { context, listeners, root } = bootPage({ POW_STATIC_URL: "https://example.test/static" });
  vm.runInContext(boot, context);
  assert.throws(() => vm.runInContext(boot, context), { name: "PowDuplicateLoad" });
  const filename = "https://example.test/static/js/challenge.bundle.js";
  listeners.error({
    filename,
    message: "Uncaught Error: Challenge script is already running",
  });
  assert.equal(root.className, "");
  listeners.error({ filename: "https://elsewhere.test/extension.js", message: "boom" });
  assert.equal(root.className, "");
  listeners.error({ filename, message: "boom" });
  assert.match(root.className, /pow-load-failed/);
});

test("safe requests retry a short outage", async () => {
  const calls = [];
  const answer = (status, body) => ({
    status,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  });
  const { client, context } = page(
    ["status", "retry"],
    {},
    {
      AbortController,
      setTimeout,
      clearTimeout,
      fetch: async (url) => {
        calls.push(url);
        if (url.endsWith("client-config"))
          return calls.filter((call) => call.endsWith("client-config")).length === 1
            ? answer(503, {})
            : answer(200, { algorithms: [{ id: "fast" }] });
        if (url.endsWith("session")) return answer(200, { authenticated: true });
        return answer(503, {});
      },
    },
  );
  let ran = false;
  client.algorithm = () => ({
    run: async () => {
      ran = true;
    },
  });
  context.location = { reload() {} };
  vm.runInContext(startup, context);
  for (let attempt = 0; attempt < 400 && !calls.some((call) => call.endsWith("session")); attempt++)
    await new Promise((resolve) => setTimeout(resolve, 10));
  assert.equal(ran, true);
  assert.equal(calls.filter((call) => call.endsWith("client-config")).length, 2);
  calls.length = 0;
  await assert.rejects(client.verify("token", "1"), { status: 503 });
  assert.equal(calls.length, 1);
});

test("a timed-out safe request retries", async () => {
  let calls = 0;
  const { client } = page(
    ["status"],
    {},
    {
      AbortController,
      clearTimeout,
      // The request timeout fires at once; retry backoff delays run normally.
      setTimeout: (callback, delay) => setTimeout(callback, delay === 30000 ? 0 : delay),
      fetch: (_url, { signal }) => {
        calls += 1;
        if (calls > 1)
          return Promise.resolve({
            status: 200,
            headers: { get: () => null },
            text: async () => "{}",
          });
        return new Promise((_resolve, reject) =>
          signal.addEventListener("abort", () =>
            reject(Object.assign(new Error("aborted"), { name: "AbortError" })),
          ),
        );
      },
    },
  );
  await client.jsonFetch("/gateway/api/client-config", { retries: 1 });
  assert.equal(calls, 2);
});

test("a page reload ends the check without another request or state", async () => {
  const calls = [];
  const { client, context, elements } = page(
    ["status", "retry"],
    {},
    {
      AbortController,
      setTimeout,
      clearTimeout,
      fetch: async (url) => {
        calls.push(url);
        return {
          status: 200,
          headers: { get: () => null },
          text: async () => JSON.stringify({ algorithms: [{ id: "fast" }] }),
        };
      },
    },
  );
  let reloads = 0;
  context.window.location = { href: "https://example.test/", reload: () => reloads++ };
  client.algorithm = () => ({
    run: async () => {
      throw client.reloadPage();
    },
  });
  context.location = { reload() {} };
  vm.runInContext(startup, context);
  for (let attempt = 0; attempt < 100 && reloads === 0; attempt++)
    await new Promise((resolve) => setTimeout(resolve, 10));
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(reloads, 1);
  assert.equal(
    calls.some((call) => call.endsWith("session")),
    false,
  );
  assert.equal(elements.get("#status").textContent, english["challenge.state.preparing.status"]);
});
