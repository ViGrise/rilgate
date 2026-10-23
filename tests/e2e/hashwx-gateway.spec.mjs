import { test, expect } from '@playwright/test';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { spawn } from 'node:child_process';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const root = process.env.RILGATE_PROJECT_ROOT || resolve(import.meta.dirname, '../..');
let origin, gateway, directory, base;
async function listen(server) {
  server.listen(0, '127.0.0.1'); await once(server, 'listening'); return server.address().port;
}

test.beforeAll(async () => {
  directory = await mkdtemp(join(tmpdir(), 'rilgate-hashwx-gateway-'));
  origin = createServer((_, response) => { response.setHeader('Content-Type', 'text/html'); response.end('<h1>HashWX protected origin</h1>'); });
  const upstreamPort = await listen(origin);
  const reservation = createServer(), port = await listen(reservation);
  await new Promise(resolve => reservation.close(resolve));
  base = `http://127.0.0.1:${port}`;
  const config = `environment: development
server:
  listen: '127.0.0.1:${port}'
  upstream:
    url: 'http://127.0.0.1:${upstreamPort}'
security:
  secret: 'hashwx-browser-test-secret-with-at-least-32-bytes'
pow:
  default_algorithm: hashwx
  algorithms:
    hashwx:
      nonces_per_seed: 463
      difficulty: {base: 1000, score_step: 10, max: 1000}
policy:
  mode: custom
  default_action: challenge
`;
  const path = join(directory, 'rilgate.yml'); await writeFile(path, config);
  const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !name.startsWith('RILGATE_')));
  gateway = spawn(process.env.RILGATE_BINARY || join(root, 'target/debug/rilgate'), ['--config', path], { cwd: directory, env, stdio: ['ignore', 'ignore', 'pipe'] });
  let errors = ''; gateway.stderr.on('data', chunk => { errors = (errors + chunk).slice(-8192); });
  for (let attempt = 0; attempt < 100; attempt++) {
    try {
      const response = await fetch(`${base}/.well-known/rilgate/api/client-config`, { signal: AbortSignal.timeout(500) });
      if (response.ok) return;
    } catch {}
    if (gateway.exitCode !== null) throw new Error(`HashWX gateway exited: ${errors}`);
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  throw new Error(`HashWX gateway startup timed out: ${errors}`);
});

test.afterAll(async () => {
  origin?.closeAllConnections(); origin?.close();
  if (gateway && gateway.exitCode === null) {
    const exited = once(gateway, 'exit');
    const deadline = setTimeout(() => gateway.kill('SIGKILL'), 5000);
    gateway.kill('SIGTERM');
    try { await exited; } finally { clearTimeout(deadline); }
  }
  if (directory) await rm(directory, { force: true, recursive: true });
});

test('the HashWX page issues work, verifies proof, exchanges the receipt and permits access', async ({ page }) => {
  const verified = [];
  page.on('response', async response => { if (response.url().endsWith('/api/verify')) verified.push(response.status()); });
  await page.goto(`${base}/protected`);
  await expect(page.getByRole('heading', { name: 'HashWX protected origin' })).toBeVisible();
  expect(verified).toEqual([200]);
  const cookies = await page.context().cookies();
  expect(cookies.some(cookie => cookie.name === 'rilgate_auth' && cookie.httpOnly)).toBe(true);
  await page.reload();
  await expect(page.getByRole('heading', { name: 'HashWX protected origin' })).toBeVisible();
  expect(verified).toEqual([200]);
});
