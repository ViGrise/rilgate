// Records the fast, HashWX, RandomX and refresh challenge pages in Chromium and writes the
// README previews to docs/images/challenge-<algorithm>.gif.
//
// The gateway renders the real challenge pages. For the JavaScript challenges, the page solves
// nothing: the proof API requests never complete, and the script drives the page states through
// window.PowClient on a fixed timeline. The refresh page runs as served, because its delay is an
// HTML meta refresh.
import { execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readdir, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { createServer as createTcpServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const { chromium } = createRequire(join(root, "tests/package.json"))("@playwright/test");
const binary = process.env.RILGATE_BINARY || join(root, "target/release/rilgate");
const viewport = { width: 720, height: 450 };
const frameRate = 25;
const job = { id: "8c1f2e", seed: "ab".repeat(32) };
// Each step is [page state, seconds on screen, options]. `rate` animates the attempt counter in
// hashes per second, `click` moves the pointer to "Start check" and clicks it, and `job` shows the
// RandomX job line.
const previews = [
  {
    name: "fast",
    steps: [
      ["preparing", 3],
      ["fast", 5, { rate: 330_000 }],
      ["verifying", 3],
      ["success", 3],
    ],
  },
  {
    name: "hashwx",
    steps: [
      ["preparing", 3],
      ["hashwx", 5, { rate: 610_000 }],
      ["verifying", 3],
      ["success", 3],
    ],
  },
  {
    name: "randomx",
    steps: [
      ["preparing", 3],
      ["consent", 3, { click: true }],
      ["initializing", 3],
      ["randomx", 5, { rate: 185, job: true }],
      ["verifying", 3],
      ["success", 3],
    ],
  },
  { name: "refresh", difficulty: 4 },
];
// Seconds that the application page stays on screen at the end of each preview.
const applicationSeconds = 3;
const applicationTitle = "Example application";
const applicationPage = `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>${applicationTitle}</title>
<style>body{margin:0;font:16px system-ui,sans-serif;background:#f6f7fb;color:#1d2433;display:grid;place-items:center;height:100vh}
main{background:#fff;border-radius:16px;padding:40px 48px;box-shadow:0 8px 30px rgba(20,30,60,.08);text-align:center}
h1{margin:0 0 8px;font-size:28px}p{margin:0;color:#5b6475}</style></head>
<body><main><h1>${applicationTitle}</h1><p>The upstream page loads after the challenge.</p></main></body></html>`;

const wait = (seconds) => new Promise((resolve) => setTimeout(resolve, seconds * 1000));

async function listen(server) {
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return server.address().port;
}

async function freePort() {
  const server = createServer();
  const port = await listen(server);
  await new Promise((resolve) => server.close(resolve));
  return port;
}

// A minimal Monero-style Stratum pool, so the gateway has RandomX work and renders the RandomX
// page. It hands out one fixed job and accepts every share.
function createStratumPool() {
  const work = {
    blob: "00".repeat(76),
    job_id: job.id,
    seed_hash: job.seed,
    target: "ffffffff",
    algo: "rx/0",
  };
  return createTcpServer((socket) => {
    let buffered = "";
    socket.on("error", () => {});
    socket.on("data", (chunk) => {
      buffered += chunk;
      let newline = buffered.indexOf("\n");
      while (newline >= 0) {
        const line = buffered.slice(0, newline).trim();
        buffered = buffered.slice(newline + 1);
        newline = buffered.indexOf("\n");
        if (!line) continue;
        const request = JSON.parse(line);
        const result =
          request.method === "login"
            ? { id: "preview-miner", job: work, status: "OK" }
            : { status: request.method === "keepalived" ? "KEEPALIVED" : "OK" };
        socket.write(
          `${JSON.stringify({ id: request.id, jsonrpc: "2.0", error: null, result })}\n`,
        );
      }
    });
  });
}

async function startGateway(directory, preview, upstreamPort, poolPort) {
  const port = await freePort();
  const operationsPort = await freePort();
  const challenge = preview.difficulty
    ? `{algorithm: ${preview.name}, difficulty: ${preview.difficulty}}`
    : `{algorithm: ${preview.name}}`;
  const config = join(directory, `${preview.name}.yml`);
  await writeFile(
    config,
    `server:
  listen: "127.0.0.1:${port}"
  operations:
    listen: "127.0.0.1:${operationsPort}"
  upstream:
    url: "http://127.0.0.1:${upstreamPort}"
security:
  secret: "challenge-preview-proof-key-with-at-least-32-bytes"
pow:
  stratum_pool:
    pools:
      - name: preview
        address: "127.0.0.1:${poolPort}"
        login: {user: "x+{difficulty}", password: x}
policy:
  rules:
    - name: every-path
      when: {path: {prefix: /}}
      action: challenge
      challenge: ${challenge}
`,
  );
  const gateway = spawn(binary, ["--config", config], { stdio: ["ignore", "ignore", "pipe"] });
  let errors = "";
  gateway.stderr.on("data", (chunk) => {
    errors = (errors + chunk).slice(-4000);
  });
  // localhost is a secure context, so the browser keeps the Secure access cookie over HTTP.
  const url = `http://localhost:${port}/`;
  for (let attempt = 0; ; attempt++) {
    try {
      await fetch(url);
      return { gateway, url };
    } catch {
      if (attempt > 100 || gateway.exitCode !== null) {
        throw new Error(`The gateway did not start: ${errors}`);
      }
      await wait(0.1);
    }
  }
}

// Runs in the page before its scripts. Proof API requests never complete, so the challenge
// runtime waits and the recorder sets every state.
function holdProofApi() {
  const send = window.fetch.bind(window);
  window.fetch = (input, init) =>
    String(input instanceof Request ? input.url : input).includes("/api/")
      ? new Promise(() => {})
      : send(input, init);
}

// Runs in the page before its scripts and draws a pointer at the mouse position, because
// headless Chromium records no cursor.
function showPointer() {
  // Each shape has its hot spot, the point that the mouse position names, at its offset.
  const shapes = {
    arrow: {
      offset: [5, 3],
      svg: '<svg width="24" height="24" viewBox="0 0 24 24"><path d="M5 3l14 9-6.5 1.5L16 20l-2.6 1.2-3-6.6L5 19z" fill="#111" stroke="#fff" stroke-width="1.5" stroke-linejoin="round"/></svg>',
    },
    hand: {
      offset: [10.5, 2],
      svg: '<svg width="24" height="24" viewBox="0 0 24 24"><path d="M9 3.5a1.5 1.5 0 0 1 3 0V10h.5V8.5a1.5 1.5 0 0 1 3 0V10h.5V9.5a1.5 1.5 0 0 1 3 0V15a6 6 0 0 1-6 6h-1.2a5 5 0 0 1-4-2l-3.3-4.4a1.5 1.5 0 0 1 2.3-1.9L9 14.5z" fill="#fff" stroke="#111" stroke-width="1.5" stroke-linejoin="round"/></svg>',
    },
  };
  addEventListener("DOMContentLoaded", () => {
    const pointer = document.createElement("div");
    Object.assign(pointer.style, {
      position: "fixed",
      left: "0",
      top: "0",
      zIndex: "2147483647",
      pointerEvents: "none",
      display: "none",
      transition: "transform 0.12s, opacity 0.4s",
    });
    document.documentElement.append(pointer);
    let shape;
    addEventListener(
      "mousemove",
      (event) => {
        const next = event.target.closest?.("button, a, [role=button]") ? "hand" : "arrow";
        if (next !== shape) {
          shape = next;
          pointer.innerHTML = shapes[shape].svg;
          pointer.style.transformOrigin = `${shapes[shape].offset[0]}px ${shapes[shape].offset[1]}px`;
        }
        pointer.style.display = "block";
        pointer.style.left = `${event.clientX - shapes[shape].offset[0]}px`;
        pointer.style.top = `${event.clientY - shapes[shape].offset[1]}px`;
      },
      true,
    );
    addEventListener("mousedown", () => (pointer.style.transform = "scale(0.85)"), true);
    addEventListener(
      "mouseup",
      () => {
        pointer.style.transform = "";
        // The pointer leaves after the click, so it does not cover the next states.
        setTimeout(() => (pointer.style.opacity = "0"), 400);
      },
      true,
    );
  });
}

// Moves the mouse along an eased path, one position per display frame.
async function glide(page, from, to, seconds) {
  const started = Date.now();
  for (;;) {
    const progress = Math.min((Date.now() - started) / 1000 / seconds, 1);
    const eased = progress < 0.5 ? 4 * progress ** 3 : 1 - (-2 * progress + 2) ** 3 / 2;
    await page.mouse.move(from.x + (to.x - from.x) * eased, from.y + (to.y - from.y) * eased);
    if (progress === 1) return;
    await wait(1 / 60);
  }
}

// Moves the pointer to "Start check" and clicks it, `seconds` after the button appears.
async function clickStart(page, seconds) {
  const button = page.locator("#randomx-start");
  await button.waitFor({ state: "visible" });
  const box = await button.boundingBox();
  const from = { x: viewport.width * 0.82, y: viewport.height * 0.86 };
  const to = { x: box.x + box.width * 0.55, y: box.y + box.height * 0.6 };
  await page.mouse.move(from.x, from.y);
  await wait(0.4);
  await glide(page, from, to, 1.4);
  await wait(Math.max(seconds - 2.2, 0.3));
  await page.mouse.down();
  await wait(0.12);
  await page.mouse.up();
}

// Shows one page state for `seconds`, with a live attempt counter while the state solves.
async function play(page, [state, seconds, options = {}]) {
  await page.evaluate(
    ({ state, showJob }) => {
      window.PowClient.state(state);
      if (showJob) {
        window.PowClient.setJobDetails(true);
        window.PowClient.job(showJob);
      }
    },
    { state, showJob: options.job ? job : null },
  );
  if (options.click) return clickStart(page, seconds);
  if (!options.rate) return wait(seconds);
  const started = Date.now();
  let attempts = 0;
  for (let elapsed = 0; elapsed < seconds; elapsed = (Date.now() - started) / 1000) {
    const rate = options.rate * (0.96 + Math.random() * 0.08);
    attempts += rate * 0.1;
    await page.evaluate(
      ([attempts, rate]) => window.PowClient.stats(attempts, rate),
      [Math.round(attempts), Math.round(rate)],
    );
    await wait(0.1);
  }
}

// Returns the video and the time, in seconds from the start of the video, at which the page
// first shows the challenge.
async function record(directory, preview, url, applicationUrl) {
  const videos = join(directory, `video-${preview.name}`);
  const browser = await chromium.launch();
  const context = await browser.newContext({
    viewport,
    recordVideo: { dir: videos, size: viewport },
    colorScheme: "light",
    locale: "en-US",
  });
  await context.addInitScript(showPointer);
  if (preview.steps) await context.addInitScript(holdProofApi);
  const started = Date.now();
  const page = await context.newPage();
  await page.goto(url);
  const start = (Date.now() - started) / 1000;
  if (preview.steps) {
    await page.waitForFunction(() => window.PowClient?.state);
    for (const step of preview.steps) await play(page, step);
    await page.goto(applicationUrl);
  }
  await page.getByText(applicationTitle).waitFor({ timeout: 60_000 });
  await wait(applicationSeconds);
  await context.close();
  await browser.close();
  const [video] = await readdir(videos);
  return { video: join(videos, video), start };
}

function writeGif({ video, start }, output) {
  const duration = Number(
    execFileSync("ffprobe", [
      "-v",
      "error",
      "-show_entries",
      "format=duration",
      "-of",
      "csv=p=0",
      video,
    ]).toString(),
  );
  // The recording ends on a blank frame.
  const end = Math.max(duration - 0.3, start + 1);
  const filter = [
    `trim=start=${start}:end=${end},setpts=PTS-STARTPTS,fps=${frameRate},scale=640:-1:flags=lanczos,split[a][b]`,
    "[a]palettegen=max_colors=128:stats_mode=full[p]",
    "[b][p]paletteuse=dither=sierra2_4a",
  ].join(";");
  execFileSync("ffmpeg", [
    "-v",
    "error",
    "-y",
    "-i",
    video,
    "-filter_complex",
    filter,
    "-loop",
    "0",
    output,
  ]);
}

const directory = await mkdtemp(join(tmpdir(), "rilgate-previews-"));
const upstream = createServer((_request, response) => {
  response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
  response.end(applicationPage);
});
const pool = createStratumPool();
try {
  const upstreamPort = await listen(upstream);
  const poolPort = await listen(pool);
  const applicationUrl = `http://127.0.0.1:${upstreamPort}/`;
  const only = process.argv.slice(2);
  for (const preview of previews.filter((item) => !only.length || only.includes(item.name))) {
    const { gateway, url } = await startGateway(directory, preview, upstreamPort, poolPort);
    try {
      const recording = await record(directory, preview, url, applicationUrl);
      const output = join(root, "docs/images", `challenge-${preview.name}.gif`);
      writeGif(recording, output);
      console.log(`Wrote ${output}`);
    } finally {
      gateway.kill("SIGTERM");
      await once(gateway, "exit");
    }
  }
} finally {
  upstream.close();
  pool.close();
  await rm(directory, { recursive: true, force: true });
}
