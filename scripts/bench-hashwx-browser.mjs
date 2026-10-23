import { createServer } from 'node:http';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { networkInterfaces } from 'node:os';
import { randomUUID } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const resultsDirectory = new URL('../target/hashwx-bench/', import.meta.url);
const routes = new Map([
  ['/', ['./lib/hashwx-benchmark.html', 'text/html; charset=utf-8']],
  ['/hashwx.wasm', ['../web/static/js/solvers/hashwx/hashwx_wasm.wasm', 'application/wasm']],
  ['/worker.js', ['./lib/hashwx-benchmark-worker.js', 'text/javascript']],
  ['/fast-worker.js', ['../web/static/js/solvers/fast/worker.js', 'text/javascript']],
]);
const server = createServer(async (request, response) => {
  response.setHeader('Cache-Control', 'no-store');
  try {
    const path = new URL(request.url, 'http://localhost').pathname;
    if (request.method === 'POST' && path === '/results') {
      let body = '';
      for await (const chunk of request) {
        body += chunk;
        if (Buffer.byteLength(body) > 262144) { response.writeHead(413).end(); return; }
      }
      const result = JSON.parse(body);
      if (result.schemaVersion !== 1 || typeof result.userAgent !== 'string' ||
          !Number.isInteger(result.hardwareConcurrency) || !result.parameters ||
          !Array.isArray(result.rows) || !Array.isArray(result.solves)) {
        response.writeHead(400).end('Invalid benchmark result');
        return;
      }
      const name = `${new Date().toISOString().replaceAll(':', '-')}-${randomUUID()}.json`;
      await mkdir(resultsDirectory, { recursive: true });
      await writeFile(new URL(name, resultsDirectory), JSON.stringify(result, null, 2) + '\n', { flag: 'wx' });
      const saved = `target/hashwx-bench/${name}`;
      console.log(`Saved ${saved}`);
      response.setHeader('Content-Type', 'application/json');
      response.end(JSON.stringify({ path: saved }));
    } else if (request.method === 'GET' && routes.has(path)) {
      const [file, contentType] = routes.get(path);
      response.setHeader('Content-Type', contentType);
      response.end(await readFile(new URL(file, import.meta.url)));
    } else { response.writeHead(404).end(); }
  } catch (error) {
    response.writeHead(error instanceof SyntaxError ? 400 : 500).end('Benchmark request failed');
  }
});
server.listen(Number(process.env.RILGATE_HASHWX_BENCH_PORT || 0), '0.0.0.0', () => {
  const port = server.address().port;
  console.log(`Benchmark: http://localhost:${port}/`);
  for (const addresses of Object.values(networkInterfaces())) {
    for (const address of addresses || []) {
      if (!address.internal && address.family === 'IPv4') console.log(`LAN: http://${address.address}:${port}/`);
    }
  }
  console.log(`Results: ${fileURLToPath(resultsDirectory)}`);
  console.log('Press Ctrl+C to stop.');
});
