import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

for (const algorithm of ['hashwx', 'randomx']) {
  test(`${algorithm} loads the solver module selected by its descriptor`, async () => {
    let adapter, selected;
    const moduleUrl = '/custom/solver.js';
    const element = { removeAttribute() {}, hidden: false };
    const context = vm.createContext({
      Worker: function () {}, WebAssembly, BigInt, Promise, setTimeout, clearTimeout,
      document: { querySelector: () => element },
      window: { POW_SOLVER_MODULE_URL: moduleUrl, PowClient: {
        t: (_, text) => text, status() {}, state() {}, stats() {}, once: (_, __, callback) => { callback(); return () => {}; },
        register: (_, value) => { adapter = value; },
        loadSolver: async url => { selected = url; throw new Error('Module request captured'); },
      } },
    });
    const source = await readFile(new URL(`../web/static/js/solvers/${algorithm}/adapter.js`, import.meta.url), 'utf8');
    vm.runInContext(source, context);
    await assert.rejects(adapter.run({ enforce: 'local_verify' }), /Module request captured/);
    assert.equal(selected, moduleUrl);
  });
}
