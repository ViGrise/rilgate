import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

test('page controls open their dialog and reload on retry', async () => {
  const source = await readFile(new URL('../web/static/js/page.js', import.meta.url), 'utf8');
  function button() { return { addEventListener(type, handler) { assert.equal(this[type], undefined); this[type] = handler; } }; }
  const legal = button();
  legal.getAttribute = name => name === 'data-dialog' ? 'privacy' : null;
  const retry = button();
  let opened = 0, reloads = 0;
  const context = vm.createContext({
    window: {},
    document: {
      querySelectorAll: selector => selector === '[data-dialog]' ? [legal] : [retry],
      getElementById: id => id === 'privacy' ? { showModal() { opened++; } } : null,
    },
    location: { reload() { reloads++; } },
  });
  vm.runInContext(source, context);
  vm.runInContext(source, context);
  legal.click();
  retry.click();
  assert.equal(opened, 1);
  assert.equal(reloads, 1);
});
