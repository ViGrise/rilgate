import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import test from 'node:test';

const source = await readFile(new URL('../scripts/lib/hashwx-benchmark-worker.js', import.meta.url), 'utf8');

test('benchmark targets retain all 64 bits at difficulty boundaries', () => {
  const context = vm.createContext({ self: {} });
  vm.runInContext(source, context);
  for (const difficulty of [1, 2, 3, 463, 1000, 10000, 50000, 5000000, 2 ** 40]) {
    const words = vm.runInContext(`targetWords(${difficulty})`, context);
    const target = BigInt(words[0]) | (BigInt(words[1]) << 32n);
    assert.equal(target, ((1n << 64n) - 1n) / BigInt(difficulty));
  }
});
