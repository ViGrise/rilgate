import { test, expect } from "@playwright/test";
import { spawn, execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { createServer as createTLS } from "node:https";
import { mkdtemp, readFile, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import { once } from "node:events";

let origin, edge, gateway, directory, base;
const root = process.env.RILGATE_PROJECT_ROOT || resolve(import.meta.dirname, "../..");

async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return server.address().port;
}

async function stop() {
  edge?.closeAllConnections();
  edge?.close();
  origin?.closeAllConnections();
  origin?.close();
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
    directory = await mkdtemp(join(tmpdir(), "rilgate-browser-"));
    origin = createServer((_request, res) => {
      res.writeHead(200, { "content-type": "text/html" });
      res.end("<h1>Protected origin</h1>");
    });
    const originPort = await listen(origin);
    const reservation = createServer();
    const port = await listen(reservation);
    const operationsReservation = createServer();
    const operationsPort = await listen(operationsReservation);
    await new Promise((resolve) => operationsReservation.close(resolve));
    await new Promise((resolve) => reservation.close(resolve));
    const config = (await readFile(join(root, "config/rilgate.production.yml"), "utf8"))
      .replace('listen: ":8080"', `listen: "127.0.0.1:${port}"`)
      .replace("http://127.0.0.1:3000", `http://127.0.0.1:${originPort}`);
    const configPath = join(directory, "production.yml");
    await writeFile(configPath, config);
    const environment = Object.fromEntries(
      Object.entries(process.env).filter(
        ([key]) => !/^(RILGATE_|POW_|UPSTREAM_|WEB_|SERVE_|ENV$|APP_ENV$|CONFIG$|PORT$)/.test(key),
      ),
    );
    gateway = spawn(
      process.env.RILGATE_BINARY || join(root, "target/debug/rilgate"),
      ["--config", configPath],
      {
        cwd: directory,
        // Every browser project solves many proofs from the same loopback client.
        env: {
          ...environment,
          RILGATE_SERVER_OPERATIONS_LISTEN: `127.0.0.1:${operationsPort}`,
          RILGATE_POW_RATE_LIMIT_CLIENT_BURST: "1000",
          RILGATE_SECURITY_SECRET: "browser-test-proof-key-with-at-least-32-bytes",
        },
        stdio: ["ignore", "ignore", "pipe"],
      },
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
      if (attempt >= 100 || gateway.exitCode !== null)
        throw new Error(`Gateway did not become ready: ${errors}`);
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    execFileSync(
      "openssl",
      [
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        join(directory, "key.pem"),
        "-out",
        join(directory, "cert.pem"),
        "-days",
        "1",
        "-subj",
        "/CN=localhost",
      ],
      { stdio: "ignore" },
    );
    const { request } = await import("node:http");
    edge = createTLS(
      {
        key: await readFile(join(directory, "key.pem")),
        cert: await readFile(join(directory, "cert.pem")),
      },
      (req, res) => {
        const upstream = request(
          { hostname: "127.0.0.1", port, path: req.url, method: req.method, headers: req.headers },
          (reply) => {
            res.writeHead(reply.statusCode, reply.headers);
            reply.pipe(res);
          },
        );
        upstream.on("error", () => {
          res.writeHead(502);
          res.end();
        });
        req.pipe(upstream);
      },
    );
    base = `https://127.0.0.1:${await listen(edge)}`;
  } catch (error) {
    await stop();
    throw error;
  }
});

test.afterAll(stop);

test("production policy protects missing and arbitrary user agents and unsafe methods", async ({
  request,
}) => {
  for (const userAgent of ["", "curl/8.0", "Mozilla/5.0"]) {
    const response = await request.get(`${base}/protected?query=kept`, {
      headers: { "user-agent": userAgent },
    });
    expect(response.status()).toBe(200);
    expect(await response.text()).toContain("pow-bootstrap");
    expect(await response.text()).not.toContain("Protected origin");
  }
  const post = await request.post(`${base}/protected`, { data: "payload" });
  expect(post.status()).toBe(401);
});

test("real worker solves proof, receives a secure cookie and returns to the original URL", async ({
  page,
  context,
}) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(`${base}/protected?query=kept`);
  await expect(page.getByRole("heading", { name: "Protected origin" })).toBeVisible({
    timeout: 45_000,
  });
  expect(page.url()).toBe(`${base}/protected?query=kept`);
  const cookie = (await context.cookies()).find((cookie) => cookie.name === "rilgate_auth");
  expect(cookie).toBeDefined();
  expect(cookie.secure).toBe(true);
  expect(cookie.httpOnly).toBe(true);
  expect(cookie.sameSite).toBe("Lax");
  expect(cookie.value.split(".")).toHaveLength(2);
  await page.reload();
  await expect(page.getByRole("heading", { name: "Protected origin" })).toBeVisible();
  expect(errors).toEqual([]);
});

test("AJAX receives JSON, solves in an iframe and retries the original request", async ({
  page,
}) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(`${base}/robots.txt`);
  const result = await page.evaluate(async () => {
    const options = {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ operation: "original" }),
    };
    const blocked = await fetch("/protected/ajax", options);
    const body = await blocked.json();
    if (blocked.status !== 403 || blocked.headers.get("rilgate-challenge") !== "required") {
      throw new Error("Expected a JSON challenge notification");
    }
    const frame = document.createElement("iframe");
    const completed = new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        window.removeEventListener("message", listener);
        reject(new Error("Iframe verification timed out"));
      }, 45000);
      function listener(event) {
        if (
          event.origin !== location.origin ||
          event.source !== frame.contentWindow ||
          event.data?.type !== "rilgate:verified"
        )
          return;
        clearTimeout(timer);
        window.removeEventListener("message", listener);
        resolve();
      }
      window.addEventListener("message", listener);
    });
    frame.src = body.challenge_url;
    document.body.append(frame);
    await completed;
    frame.remove();
    const retried = await fetch("/protected/ajax", options);
    return { status: retried.status, body: await retried.text(), url: location.pathname };
  });
  expect(result.status).toBe(200);
  expect(result.body).toContain("Protected origin");
  expect(result.url).toBe("/robots.txt");
  expect(errors).toEqual([]);
});

for (const missing of [
  ["fetch"],
  ["Promise"],
  ["TextEncoder"],
  ["Worker"],
  ["AbortSignal.timeout"],
  ["Object.hasOwn"],
  [
    "fetch",
    "Promise",
    "TextEncoder",
    "Worker",
    "AbortSignal.timeout",
    "Object.hasOwn",
    "AbortController",
  ],
]) {
  test(`fast proof completes without ${missing.join(", ")}`, async ({ page, context }) => {
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page.addInitScript((missing) => {
      for (const name of missing) {
        if (name === "AbortSignal.timeout") {
          if (window.AbortSignal) window.AbortSignal.timeout = undefined;
        } else if (name === "Object.hasOwn") {
          Object.hasOwn = undefined;
        } else {
          window[name] = undefined;
        }
      }
    }, missing);
    if (missing.includes("TextEncoder")) {
      await page.route("**/static/**/js/solvers/fast/worker.js", async (route) => {
        const response = await route.fetch();
        await route.fulfill({
          response,
          body: `self.TextEncoder = undefined;\n${await response.text()}`,
        });
      });
    }
    await page.goto(`${base}/protected?legacy=${encodeURIComponent(missing.join(","))}`);
    await expect(page.getByRole("heading", { name: "Protected origin" })).toBeVisible({
      timeout: 45_000,
    });
    expect((await context.cookies()).some((cookie) => cookie.name === "rilgate_auth")).toBe(true);
    expect(errors).toEqual([]);
  });
}

test("RandomX without WebAssembly reaches the localized unsupported state before loading a module", async ({
  page,
  context,
}) => {
  const errors = [];
  const workRequests = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    if (
      /\/api\/(challenge|verify|exchange)$|\/solvers\/randomx\/(loader|solver)\.js/.test(
        request.url(),
      )
    )
      workRequests.push(request.url());
  });
  await page.addInitScript(() => {
    window.WebAssembly = undefined;
  });
  await page.route("**/randomx-incapable", async (route) => {
    const response = await route.fetch();
    const html = (await response.text()).replace('"algorithm":"fast"', '"algorithm":"randomx"');
    await route.fulfill({ response, body: html });
  });
  await page.route("**/api/client-config", (route) =>
    route.fulfill({
      json: {
        algorithms: [{ id: "randomx", verification: "gateway" }],
      },
    }),
  );
  await page.goto(`${base}/randomx-incapable`);
  await page.locator("#randomx-start").click();
  await expect(page.locator("#status")).toContainText("An up-to-date browser may help.");
  await expect(page.locator("#retry")).toBeHidden();
  expect(workRequests).toEqual([]);
  expect((await context.cookies()).some((cookie) => cookie.name === "rilgate_auth")).toBe(false);
  expect(errors).toEqual([]);
});

test("the page recovers when the challenge script fails to load", async ({ page, context }) => {
  let requests = 0;
  await page.route("**/challenge.bundle.js*", (route) => {
    requests += 1;
    if (requests <= 2) return route.fulfill({ status: 503, body: "" });
    return route.continue();
  });
  await page.goto(`${base}/protected?script-retry`);
  await expect(page.getByRole("heading", { name: "Protected origin" })).toBeVisible({
    timeout: 45_000,
  });
  expect(requests).toBeGreaterThanOrEqual(3);
  expect((await context.cookies()).some((cookie) => cookie.name === "rilgate_auth")).toBe(true);
});

test("the page shows the load error when the challenge script never loads", async ({ page }) => {
  await page.route("**/challenge.bundle.js*", (route) => route.fulfill({ status: 503, body: "" }));
  await page.goto(`${base}/protected?script-blocked`);
  await expect(page.locator("#load-error")).toBeVisible({ timeout: 45_000 });
  await expect(page.locator("#status")).toBeHidden();
});
