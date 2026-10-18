import { createHash } from 'node:crypto';

const base = process.argv[2];
if (!base) throw new Error('Usage: node tests/container/proof.mjs URL');
async function request(path, data) {
  const response = await fetch(new URL(path, base), {
    method: data ? 'POST' : 'GET',
    headers: data ? { 'content-type': 'application/json' } : {},
    body: data ? JSON.stringify(data) : undefined,
    signal: AbortSignal.timeout(10_000),
    redirect: 'error',
  });
  if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
  return response;
}
const html = await (await request('/protected')).text();
const match = html.match(/window\.POW_PRESENTATION_TOKEN = ("[^"\n]+");/);
if (!match) throw new Error('Missing production presentation token');
const api = '/.well-known/rilgate/api';
const work = await (await request(`${api}/challenge`, { presentation_token: JSON.parse(match[1]) })).json();
if (work.algorithm !== 'fast' || work.difficulty !== 4) throw new Error('Production smoke requires Fast difficulty 4');
let nonce;
const deadline = Date.now() + 30_000;
for (let value = 0; Date.now() < deadline; value++) {
  if (createHash('sha256').update(`${work.challenge}${value}`).digest('hex').startsWith('0000')) {
    nonce = String(value);
    break;
  }
}
if (nonce === undefined) throw new Error('Production proof search timed out');
const { receipt } = await (await request(`${api}/verify`, { challenge_token: work.challenge_token, nonce })).json();
const exchange = await request(`${api}/exchange`, { receipt });
const cookie = exchange.headers.get('set-cookie')?.split(';', 1)[0];
if (!cookie) throw new Error('Exchange did not issue an access cookie');
await exchange.arrayBuffer();
process.stdout.write(cookie);
