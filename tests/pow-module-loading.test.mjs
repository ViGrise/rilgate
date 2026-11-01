import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

for (const algorithm of ["hashwx", "randomx"]) {
  test(`${algorithm} loads the solver module selected by its descriptor`, async () => {
    let adapter, selected;
    const moduleUrl = "/custom/solver.js";
    const element = { removeAttribute() {}, hidden: false };
    const context = vm.createContext({
      Worker: function () {},
      WebAssembly,
      BigInt,
      Promise,
      setTimeout,
      clearTimeout,
      document: { querySelector: () => element },
      window: {
        POW_SOLVER_MODULE_URL: moduleUrl,
        PowClient: {
          t: (_, text) => text,
          supportsWasm: () => true,
          status() {},
          state() {},
          stats() {},
          once: (_, __, callback) => {
            callback();
            return () => {};
          },
          register: (_, value) => {
            adapter = value;
          },
          loadSolver: async (url) => {
            selected = url;
            throw new Error("Module request captured");
          },
        },
      },
    });
    const source = await readFile(
      new URL(`../web/static/js/solvers/${algorithm}/adapter.js`, import.meta.url),
      "utf8",
    );
    vm.runInContext(source, context);
    await assert.rejects(adapter.run({ enforce: "local_verify" }), /Module request captured/);
    assert.equal(selected, moduleUrl);
  });
}

async function runtimePage() {
  const listeners = [];
  const scripts = [];
  const english = JSON.parse(
    await readFile(new URL("../web/i18n/en.json", import.meta.url), "utf8"),
  );
  const elements = new Map([
    [
      "#pow-bootstrap",
      {
        textContent: JSON.stringify({
          internal_prefix: "/gateway",
          algorithm: "fast",
          static_url: "/gateway/static/v-1",
          presentation_token: "presentation",
          embedded: false,
        }),
      },
    ],
    ["#pow-i18n", { textContent: JSON.stringify(english) }],
  ]);
  for (const id of ["title", "description", "status", "retry"]) {
    elements.set(`#${id}`, {
      hidden: true,
      textContent: "",
      removeAttribute() {},
      addEventListener() {},
      removeEventListener() {},
    });
  }
  const context = vm.createContext({
    document: {
      documentElement: { lang: "en", className: "" },
      querySelector: (selector) => elements.get(selector) || null,
      createElement: () => ({ style: {} }),
      head: { appendChild: (script) => scripts.push(script) },
    },
    window: {
      addEventListener: (type, handler) => listeners.push([type, handler]),
      removeEventListener() {},
    },
    HTMLScriptElement: Object.assign(function () {}, { prototype: { noModule: false } }),
    navigator: {},
    location: { reload() {} },
    console: { error() {} },
    TypeError,
    setTimeout,
    clearTimeout,
  });
  vm.runInContext(
    await readFile(new URL("../web/static/js/challenge/runtime.js", import.meta.url), "utf8"),
    context,
  );
  return { context, listeners, scripts, elements, english };
}

test("a solver syntax error reports an unsupported browser at once", async () => {
  const { context, listeners, scripts } = await runtimePage();
  const loading = context.window.PowClient.loadSolver("/solver.js");
  const [, handler] = listeners.find(([type]) => type === "error");
  handler({
    filename: scripts[0].src.split("loader.js")[0] + "hashwx/solver.js",
    error: vm.runInContext('new SyntaxError("Unexpected token")', context),
    message: "Uncaught SyntaxError: Unexpected token",
  });
  const outcome = await Promise.race([
    loading.then(
      () => "resolved",
      (error) => error.code,
    ),
    new Promise((resolve) => setTimeout(resolve, 1000, "still waiting")),
  ]);
  assert.equal(outcome, "unsupported_browser");
});

test("a blocked solver module reports blocked", async () => {
  const { context, scripts, elements, english } = await runtimePage();
  const client = context.window.PowClient;
  await assert.rejects(
    (async () => {
      const loading = client.loadSolver("/solver.js");
      scripts[0].onerror();
      await loading;
    })(),
    { code: "asset_blocked" },
  );
  client.jsonFetch = async () => ({ algorithms: [{ id: "fast" }] });
  client.algorithm = () => ({
    run: async () => {
      const loading = client.loadSolver("/solver.js");
      scripts[1].onerror();
      await loading;
    },
  });
  vm.runInContext(
    await readFile(new URL("../web/static/js/challenge.js", import.meta.url), "utf8"),
    context,
  );
  for (let attempt = 0; attempt < 200 && elements.get("#retry").hidden; attempt++)
    await new Promise((resolve) => setTimeout(resolve, 5));
  assert.equal(elements.get("#title").textContent, english["challenge.state.blocked.title"]);
  assert.equal(elements.get("#retry").hidden, false);
});
