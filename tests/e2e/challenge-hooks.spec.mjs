import { test, expect } from "@playwright/test";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";

const root = process.env.RILGATE_PROJECT_ROOT || resolve(import.meta.dirname, "../..");
let origin, provider, gateway, directory, base;

async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return server.address().port;
}

async function stop() {
  for (const server of [origin, provider]) {
    server?.closeAllConnections();
    server?.close();
  }
  if (gateway && gateway.exitCode === null) {
    const exited = once(gateway, "exit");
    const deadline = setTimeout(() => gateway.kill("SIGKILL"), 5000);
    gateway.kill("SIGTERM");
    try {
      await exited;
    } finally {
      clearTimeout(deadline);
    }
  }
  if (directory) await rm(directory, { recursive: true, force: true });
}

test.beforeAll(async () => {
  try {
    directory = await mkdtemp(join(tmpdir(), "rilgate-hook-browser-"));
    origin = createServer((_req, res) => {
      res.writeHead(200, { "content-type": "text/html" });
      res.end("<h1>Protected origin</h1>");
    });
    const originPort = await listen(origin);
    provider = createServer(async (req, res) => {
      const url = new URL(req.url, "http://127.0.0.1");
      if (req.method === "GET" && url.pathname === "/widget.js") {
        res.writeHead(200, { "content-type": "text/javascript" });
        res.end(`window.fakeCheck = {render(el, o) {
          const button = document.createElement("button");
          button.id = "fake-check";
          button.textContent = "Complete check";
          button.addEventListener("click", () => o.callback("pass-token"));
          el.appendChild(button);
        }};
        window[${JSON.stringify(url.searchParams.get("onload"))}]();`);
      } else if (req.method === "POST" && url.pathname === "/verify") {
        let body = "";
        for await (const chunk of req) body += chunk;
        res.writeHead(200, { "content-type": "application/json" });
        res.end(
          JSON.stringify({ success: new URLSearchParams(body).get("response") === "pass-token" }),
        );
      } else {
        res.writeHead(404);
        res.end();
      }
    });
    const providerPort = await listen(provider);
    const reservation = createServer();
    const port = await listen(reservation);
    const operationsReservation = createServer();
    const operationsPort = await listen(operationsReservation);
    await new Promise((resolve) => operationsReservation.close(resolve));
    await new Promise((resolve) => reservation.close(resolve));
    const configPath = join(directory, "hooks.yml");
    await writeFile(
      configPath,
      `environment: development
server:
  listen: "127.0.0.1:${port}"
  operations:
    listen: "127.0.0.1:${operationsPort}"
  upstream:
    url: "http://127.0.0.1:${originPort}"
security:
  secret: browser-test-proof-key-with-at-least-32-bytes
challenge_hooks:
  fake:
    presentation:
      mode: embed
      widget:
        script: http://127.0.0.1:${providerPort}/widget.js
        onload: onload
        call: fakeCheck.render
        args: [$container, {callback: $result}]
      csp:
        script-src: [http://127.0.0.1:${providerPort}/]
    verify:
      request:
        url: http://127.0.0.1:${providerPort}/verify
        fields: {response: "{result}"}
      accept: [{field: /success, equals: true}]
policy:
  rules:
    - name: protected
      when: {path: {prefix: /}}
      action: challenge
      challenge:
        steps: [{algorithm: fast, difficulty: 1}, {hook: fake}]
`,
    );
    const environment = Object.fromEntries(
      Object.entries(process.env).filter(
        ([key]) => !/^(RILGATE_|POW_|UPSTREAM_|WEB_|SERVE_|ENV$|APP_ENV$|CONFIG$|PORT$)/.test(key),
      ),
    );
    gateway = spawn(
      process.env.RILGATE_BINARY || join(root, "target/debug/rilgate"),
      ["--config", configPath],
      { cwd: directory, env: environment, stdio: ["ignore", "ignore", "pipe"] },
    );
    let errors = "";
    gateway.stderr.on("data", (chunk) => {
      errors = (errors + chunk).slice(-8192);
    });
    for (let attempt = 0; ; attempt++) {
      try {
        const ready = await fetch(
          `http://127.0.0.1:${operationsPort}/.well-known/rilgate/api/ready`,
          { signal: AbortSignal.timeout(500) },
        );
        if (ready.ok) break;
      } catch {
        /* Startup is asynchronous. */
      }
      if (attempt >= 100 || gateway.exitCode !== null) {
        throw new Error(`Gateway did not become ready: ${errors}`);
      }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    base = `http://127.0.0.1:${port}`;
  } catch (error) {
    await stop();
    throw error;
  }
});

test.afterAll(stop);

test("Fast proof and embedded widget grant access to the origin", async ({ page }) => {
  await page.goto(base + "/");
  await expect(page.locator("#fake-check")).toBeVisible({ timeout: 45_000 });
  await page.locator("#fake-check").click();
  await expect(page.getByRole("heading", { name: "Protected origin" })).toBeVisible();
});
