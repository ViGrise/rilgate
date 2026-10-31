import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

const runtime = readFileSync(new URL('../web/static/js/challenge/runtime.js', import.meta.url), 'utf8');
const startup = readFileSync(new URL('../web/static/js/challenge.js', import.meta.url), 'utf8');
const english = JSON.parse(readFileSync(new URL('../web/i18n/en.json', import.meta.url), 'utf8'));
function page(ids = ['status'], bootstrap = {}) {
  const elements = new Map(ids.map(id => ['#' + id, {
    style: {}, hidden: true, textContent: '', removeAttribute() {},
    addEventListener() {}, removeEventListener() {},
  }]));
  elements.set('#pow-i18n', { textContent: JSON.stringify(english) });
  elements.set('#pow-bootstrap', { textContent: JSON.stringify({ internal_prefix: '/gateway', algorithm: 'fast', presentation_token: 'presentation', embedded: false, solver_module_url: '/solver.js', ...bootstrap }) });
  const context = vm.createContext({
    document: { documentElement: { lang: 'en' }, querySelector: id => elements.get(id) || null },
    window: {}, console: { error() {} }, TypeError,
  });
  vm.runInContext(runtime, context);
  return { context, client: context.window.PowClient, elements };
}

test('custom templates can omit new elements and retain status updates', () => {
  const { client, elements } = page();
  client.state('verifying');
  client.stats(1234, 56);
  assert.equal(elements.get('#status').textContent, english['challenge.state.verifying.status']);
  client.status('Operator status');
  assert.equal(elements.get('#status').textContent, 'Operator status');
});

test('asset paths use the static URL from the page bootstrap', () => {
  const { client } = page(['status'], { static_url: '/gateway/static/v-3f2a9c01b7de' });
  assert.equal(client.asset('/js/x.js'), '/gateway/static/v-3f2a9c01b7de/js/x.js');
  assert.equal(page().client.asset('/js/x.js'), '/gateway/static/js/x.js');
});

test('statistics use page locale and remain visible through result confirmation', () => {
  const { client, elements } = page(['status', 'stats', 'attempts', 'hashrate']);
  client.state('fast');
  client.stats(1234, 56);
  assert.equal(elements.get('#attempts').textContent, '1,234');
  assert.equal(elements.get('#hashrate').textContent, '56');
  for (const state of ['verifying', 'pool', 'success']) {
    client.state(state);
    assert.equal(elements.get('#stats').hidden, false);
  }
  client.state('failed');
  assert.equal(elements.get('#stats').hidden, true);
});

for (const [name, error] of [
  ['expired', new Error('RandomX challenge expired. Retry to request new work.')],
  ['expired', new Error('HashWX challenge expired')],
  ['expired', new Error('deadline exceeded')],
  ['network', new TypeError('Failed to fetch')],
  ['limited', Object.assign(new Error('HTTP 429'), { status: 429, retryAfter: ' Try again in 7 seconds.' })],
  ['unavailable', Object.assign(new Error('HTTP 503'), { status: 503 })],
  ['unsupported', Object.assign(new Error('Unsupported browser'), { code: 'unsupported_browser' })],
  ['failed', new Error('Private diagnostic details')],
]) {
  test(`startup maps ${error.message} to ${name}`, async () => {
    const { client, context, elements } = page(['status', 'retry']);
    client.jsonFetch = async () => { throw error; };
    vm.runInContext(startup, context);
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(elements.get('#status').textContent, english[`challenge.state.${name}.status`] + (error.retryAfter || ''));
    assert.equal(elements.get('#retry').hidden, ['expired', 'network', 'unavailable', 'failed'].includes(name) ? false : true);
    assert.ok(!elements.get('#status').textContent.includes('Private diagnostic'));
  });
}

test('the bar keeps running across work states and fills only on success', () => {
  const { client, context, elements } = page(['bar', 'bar-shimmer', 'bar-complete', 'randomx-start', 'retry']);
  let restarts = 0;
  Object.defineProperty(elements.get('#bar-shimmer').style, 'animation', {
    set(value) { if (value === 'none') restarts++; },
  });
  const frames = [];
  context.requestAnimationFrame = callback => frames.push(callback);
  client.state('preparing');
  client.state('fast');
  client.state('verifying');
  assert.equal(restarts, 1);
  assert.equal(elements.get('#bar').hidden, false);
  assert.equal(elements.get('#bar-complete').style.transform, 'scaleX(0)');
  client.state('success');
  frames.shift()();
  frames.shift()();
  assert.equal(elements.get('#bar-shimmer').hidden, true);
  assert.equal(elements.get('#bar-complete').style.transform, 'scaleX(1)');
  client.state('consent');
  assert.equal(elements.get('#bar').hidden, true);
  assert.equal(elements.get('#randomx-start').hidden, false);
  context.matchMedia = () => ({ matches: true });
  client.state('initializing');
  client.state('success');
  assert.equal(restarts, 2);
  assert.equal(frames.length, 0);
  assert.equal(elements.get('#bar-complete').style.transform, 'scaleX(1)');
});

test('XHR network failures reach the network state', async () => {
  const { context, elements } = page(['status', 'retry']);
  context.XMLHttpRequest = class {
    open() {}
    setRequestHeader() {}
    send() { this.onerror(); }
  };
  vm.runInContext(startup, context);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(elements.get('#status').textContent, english['challenge.state.network.status']);
  assert.equal(elements.get('#retry').hidden, false);
});
