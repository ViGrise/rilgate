import { test, expect } from "@playwright/test";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { once } from "node:events";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { createHash } from "node:crypto";

const root = process.env.RILGATE_PROJECT_ROOT || resolve(import.meta.dirname, "../..");
const assets = join(root, "web/static/js/solvers/hashwx");
let server, base;
const challenge = { challenge: "ab".repeat(32), nonces_per_seed: 463, difficulty: 1000 };

test.beforeAll(async () => {
  server = createServer(async (request, response) => {
    const path = new URL(request.url, "http://localhost").pathname;
    if (path === "/" || path === "/blocked") {
      response.setHeader("Content-Type", "text/html");
      if (path === "/blocked")
        response.setHeader("Content-Security-Policy", "script-src 'self' 'unsafe-inline'");
      response.end(
        '<!doctype html><title>HashWX solver test</title><script type="module">import * as solver from "/solver.js"; window.solver = solver;</script>',
      );
    } else if (path === "/worker-pool.js") {
      response.setHeader("Content-Type", "text/javascript");
      response.end(await readFile(join(assets, "../worker-pool.js")));
    } else if (["/solver.js", "/worker.js", "/hashwx_wasm.wasm"].includes(path)) {
      response.setHeader(
        "Content-Type",
        path.endsWith(".wasm") ? "application/wasm" : "text/javascript",
      );
      response.end(await readFile(join(assets, path.slice(1))));
    } else response.writeHead(404).end();
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  base = `http://127.0.0.1:${server.address().port}`;
});
test.afterAll(async () => {
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
});

for (const fallback of [false, true]) {
  test(`real workers solve without BigInt, generated fallback=${fallback}`, async ({ page }) => {
    await page.route("**/worker.js", async (route) => {
      const source = await readFile(join(assets, "worker.js"), "utf8");
      const prefix = fallback
        ? `let compilations = 0; const instantiate = WebAssembly.instantiate; WebAssembly.instantiate = (...args) => ++compilations > 1 ? Promise.reject(new Error('Generated compilation denied')) : instantiate(...args);`
        : "";
      await route.fulfill({
        contentType: "text/javascript",
        body: "globalThis.BigInt = undefined;\n" + prefix + source,
      });
    });
    await page.goto(base);
    await page.waitForFunction(() => window.solver);
    const nonceText = await page.evaluate(
      (work) =>
        window.solver
          .solve(
            { ...work, expires_in: 60, expires_at: Math.floor(Date.now() / 1000) + 60 },
            { workers: 2 },
          )
          .then((result) => result.nonce),
      challenge,
    );
    const nonce = BigInt(nonceText),
      batch = Buffer.alloc(8);
    batch.writeBigUInt64LE(nonce / 463n);
    const seed = createHash("sha256")
      .update(Buffer.from(challenge.challenge, "hex"))
      .update(batch)
      .digest();
    const { loadHashWx, createHashWx } = await import(
      pathToFileURL(join(root, "scripts/lib/hashwx-wasm.mjs"))
    );
    const hasher = createHashWx(await loadHashWx(), seed);
    try {
      const [lo, hi] = hasher.hash(Number(nonce & 0xffffffffn), Number(nonce >> 32n));
      expect(BigInt(lo) | (BigInt(hi) << 32n)).toBeLessThan(((1n << 64n) - 1n) / 1000n);
    } finally {
      hasher.destroy();
    }
  });
}

test("a worker CSP that denies all WASM rejects calculation", async ({ page }) => {
  await page.route("**/worker.js", async (route) => {
    const response = await route.fetch();
    await route.fulfill({
      response,
      headers: { ...response.headers(), "content-security-policy": "script-src 'self'" },
    });
  });
  await page.goto(`${base}/blocked`);
  await page.waitForFunction(() => window.solver);
  const error = await page.evaluate(async (work) => {
    try {
      await window.solver.solve(
        { ...work, expires_in: 60, expires_at: Math.floor(Date.now() / 1000) + 60 },
        { workers: 1 },
      );
      return "";
    } catch (error) {
      return error.message;
    }
  }, challenge);
  expect(error).not.toBe("");
});
