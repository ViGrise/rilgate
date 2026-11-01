import { createHash } from "node:crypto";

const origin = process.argv[2];
if (!origin) throw new Error("Usage: node scripts/perf/auth-cookie.mjs http://127.0.0.1:8080");

async function request(path, data) {
  const response = await fetch(new URL(path, origin), {
    method: data ? "POST" : "GET",
    headers: data ? { "content-type": "application/json" } : {},
    body: data ? JSON.stringify(data) : undefined,
    signal: AbortSignal.timeout(10_000),
    redirect: "error",
  });
  if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
  return response;
}

const html = await (await request("/protected")).text();
const match = html.match(
  /<script type="application\/json" id="pow-bootstrap">([\s\S]*?)<\/script>/,
);
if (!match) throw new Error("Missing presentation token");
const prefix = "/.well-known/rilgate/api";
const challenge = await (
  await request(`${prefix}/challenge`, {
    presentation_token: JSON.parse(match[1]).presentation_token,
  })
).json();
if (challenge.algorithm !== "fast" || challenge.difficulty !== 1) {
  throw new Error("Performance lab requires Fast difficulty 1");
}
let nonce;
for (let value = 0; value < 100_000; value++) {
  const hash = createHash("sha256").update(`${challenge.challenge}${value}`).digest();
  if ((hash[0] & 0xf0) === 0) {
    nonce = String(value);
    break;
  }
}
if (nonce === undefined) throw new Error("Fast proof search exhausted");
const { receipt } = await (
  await request(`${prefix}/verify`, {
    challenge_token: challenge.challenge_token,
    nonce,
  })
).json();
const exchanged = await request(`${prefix}/exchange`, { receipt });
const cookie = exchanged.headers.get("set-cookie")?.split(";", 1)[0];
if (!cookie) throw new Error("Exchange did not set an authentication cookie");
await exchanged.arrayBuffer();
process.stdout.write(cookie);
