import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const source = (name) => readFileSync(new URL("../web/static/js/" + name, import.meta.url), "utf8");
const english = JSON.parse(readFileSync(new URL("../web/i18n/en.json", import.meta.url), "utf8"));

function page(responses = [], failLoads = false) {
  const elements = new Map();
  function element(id) {
    const el = {
      id,
      style: {},
      hidden: true,
      textContent: "",
      children: [],
      appendChild(child) {
        child.parentNode = this;
        this.children.push(child);
        if (child.id) elements.set("#" + child.id, child);
        return child;
      },
      removeChild(child) {
        this.children.splice(this.children.indexOf(child), 1);
        child.parentNode = null;
      },
      removeAttribute() {},
      addEventListener(type, handler) {
        this[type] = handler;
      },
      removeEventListener(type) {
        delete this[type];
      },
    };
    return el;
  }
  for (const id of ["title", "description", "status", "step", "retry", "hook-container"]) {
    elements.set("#" + id, element(id));
  }
  const hook = {
    name: "fake",
    mode: "embed",
    widget: {
      script: "https://example.test/widget.js?render=explicit",
      onload: "onload",
      call: "fakeCheck.render",
      args: ["$container", { callback: "$result" }],
      returns: "callback",
      integrity: null,
    },
  };
  elements.set("#pow-bootstrap", {
    textContent: JSON.stringify({
      internal_prefix: "/gateway",
      method: "hook",
      hook,
      algorithm: "fast",
      presentation_token: "presentation",
      embedded: true,
    }),
  });
  elements.set("#pow-i18n", { textContent: JSON.stringify(english) });
  const scripts = [];
  const posts = [];
  const renders = [];
  const classes = new Set();
  const head = element("head");
  let context;
  head.appendChild = (script) => {
    scripts.push(script);
    script.parentNode = head;
    head.children.push(script);
    if (failLoads) queueMicrotask(() => script.onerror());
  };
  const provider = {
    render(container, options) {
      assert.equal(this, provider);
      renders.push({ container, options });
    },
  };
  context = vm.createContext({
    document: {
      documentElement: {
        lang: "en",
        classList: { add: (name) => classes.add(name), remove: (name) => classes.delete(name) },
      },
      head,
      createElement: element,
      querySelector: (id) => elements.get(id) || null,
    },
    window: {
      fakeCheck: provider,
      addEventListener() {},
      removeEventListener() {},
    },
    navigator: {},
    console: { error() {} },
    TypeError,
    AbortController,
    setTimeout: (fn, ms) => setTimeout(fn, ms < 10000 ? 0 : ms),
    clearTimeout,
    fetch: async (url, options) => {
      let status = 200;
      let body = {};
      if (url.endsWith("/api/client-config")) body = { algorithms: [{ id: "fast" }] };
      else if (url.endsWith("/api/session")) body = { authenticated: true };
      else if (url.endsWith("/api/hook/verify")) {
        posts.push(JSON.parse(options.body));
        ({ status = 200, body = {} } = responses.shift() || {});
      } else throw new Error(`Unexpected URL: ${url}`);
      return { status, text: async () => JSON.stringify(body), headers: { get: () => null } };
    },
  });
  context.window.parent = context.window;
  for (const name of ["challenge/runtime.js", "challenge/registry.js", "challenge/hook.js"]) {
    vm.runInContext(source(name), context);
  }
  return {
    context,
    client: context.window.PowClient,
    elements,
    scripts,
    posts,
    renders,
    classes,
    start: () => vm.runInContext(source("challenge.js"), context),
    loaded() {
      const name = new URL(scripts[0].src).searchParams.get("onload");
      context.window[name]();
    },
  };
}

async function until(predicate) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  assert.fail("Expected hook progress");
}

test("onload widget receives the child and posts its callback result", async () => {
  const view = page();
  const running = view.client.runHook(view.context.window.POW_HOOK);
  assert.match(view.scripts[0].src, /&onload=__powHookLoaded\d+/);
  view.loaded();
  await until(() => view.renders.length === 1);
  const { container, options } = view.renders[0];
  assert.equal(container, view.elements.get("#hook-container").children[0]);
  options.callback("pass-token");
  await running;
  assert.deepEqual(view.posts, [{ presentation_token: "presentation", result: "pass-token" }]);
});

test("rejected verification renders again and posts the second result", async () => {
  const view = page([{ status: 401, body: { code: "hook_rejected" } }]);
  const running = view.client.runHook(view.context.window.POW_HOOK);
  view.loaded();
  await until(() => view.renders.length === 1);
  view.renders[0].options.callback("first");
  await until(() => view.renders.length === 2);
  assert.equal(view.client.currentState(), "rejected");
  assert.notEqual(view.renders[0].container, view.renders[1].container);
  view.renders[1].options.callback("second");
  await running;
  assert.deepEqual(
    view.posts.map((post) => post.result),
    ["first", "second"],
  );
});

test("unavailable hook starts the supplied proof step in the page loop", async () => {
  const next = {
    method: "pow",
    algorithm: "fast",
    solver_module_url: "/fast.js",
    presentation_token: "fallback",
    step: 1,
    steps: 1,
  };
  const view = page([{ status: 503, body: { code: "hook_unavailable", next } }]);
  let proofs = 0;
  view.client.register("fast", {
    async run() {
      proofs++;
      assert.equal(view.context.window.POW_PRESENTATION_TOKEN, "fallback");
      assert.equal(view.classes.has("pow-hook"), false);
      assert.equal(view.elements.get("#hook-container").hidden, true);
    },
  });
  view.start();
  await until(() => view.scripts.length === 1);
  view.loaded();
  await until(() => view.renders.length === 1);
  view.renders[0].options.callback("token");
  await until(() => view.client.currentState() === "success");
  assert.equal(proofs, 1);
});

test("three script load errors show the blocked state", async () => {
  const view = page([], true);
  view.start();
  await until(() => view.client.currentState() === "blocked");
  assert.equal(view.scripts.length, 3);
  assert.equal(view.posts.length, 0);
});
