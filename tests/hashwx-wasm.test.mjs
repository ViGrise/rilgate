import assert from "node:assert/strict";
import test from "node:test";
import { createHashWx, loadHashWx, words } from "../scripts/lib/hashwx-wasm.mjs";

const seedA = Buffer.from("5468697320697320612074657374207365656420666f72206861736877780000", "hex");
const seedB = Buffer.from("4c6f72656d20697073756d20646f6c6f722073697420616d6574000000000000", "hex");
const vectors = [
  [seedA, [0, 0], [0x6f8ee362, 0x97368417]],
  [seedA, [123456, 0], [0x07d69b07, 0x401983bb]],
  [seedB, [123456, 0], [0x4a9a8d3d, 0x4af38d83]],
  [seedB, [0x4b717715, 0x0db4da5f], [0x432e17a3, 0x6a8a5514]],
];

for (const [index, [seed, nonce, expected]] of vectors.entries()) {
  for (const mode of ["interpreter", "generated"]) {
    test(`Appendix F.${index + 1} via ${mode}`, async () => {
      const exports = await loadHashWx();
      const context = createHashWx(exports, seed);
      try {
        if (mode === "interpreter") {
          assert.deepEqual(context.hash(...nonce), expected);
        } else {
          const bytes = context.moduleBytes();
          assert.ok(WebAssembly.validate(bytes));
          const { instance } = await WebAssembly.instantiate(bytes, {});
          assert.equal(instance.exports.hash(...nonce), 0);
          assert.deepEqual(words(instance.exports.memory, 16400), expected);
        }
      } finally {
        assert.equal(context.destroy(), 0);
      }
    });
  }
}

test("4096 pseudorandom nonces across 64 seeds match in both engines", async () => {
  const exports = await loadHashWx();
  let state = 0x12345678;
  function random() {
    state ^= state << 13; state ^= state >>> 17; state ^= state << 5;
    return state >>> 0;
  }
  for (let i = 0; i < 64; i++) {
    const seed = Uint8Array.from({ length: 32 }, () => random() & 255);
    const context = createHashWx(exports, seed);
    try {
      const bytes = context.moduleBytes();
      assert.ok(bytes.length < 32768);
      const { instance } = await WebAssembly.instantiate(bytes, {});
      const generated = instance.exports;
      const boundary = [[0, 0], [1, 0], [0xffffffff, 0], [0, 1], [0xffffffff, 0x7fffffff], [0, 0x80000000], [0xffffffff, 0xffffffff]];
      for (let j = 0; j < 64; j++) {
        const nonce = boundary[j] ?? [random(), random()];
        assert.equal(generated.hash(...nonce), 0);
        assert.deepEqual(words(generated.memory, 16400), context.hash(...nonce), `seed ${i}, nonce ${nonce}`);
      }
      assert.deepEqual(context.moduleBytes(), bytes);
    } finally {
      assert.equal(context.destroy(), 0);
    }
  }
});

function less(a, b) {
  return a[1] < b[1] || (a[1] === b[1] && a[0] < b[0]);
}

function increment([lo, hi]) {
  return [(lo + 1) >>> 0, (hi + (lo === 0xffffffff ? 1 : 0)) >>> 0];
}

test("range search returns the first strict match, handles carries, wraparound and exhaustion", async () => {
  const exports = await loadHashWx();
  const context = createHashWx(exports, seedA);
  const { instance } = await WebAssembly.instantiate(context.moduleBytes(), {});
  const generated = instance.exports;
  try {
    for (const start of [[0, 0], [0xfffffff0, 0], [0xfffffff0, 0xffffffff]]) {
      const candidates = [];
      let nonce = start;
      for (let i = 0; i < 64; i++) {
        candidates.push({ nonce, hash: context.hash(...nonce) });
        nonce = increment(nonce);
      }
      const targets = [[0, 0], [0xffffffff, 0xffffffff], ...candidates.map(({ hash }) => hash)];
      for (const target of targets) {
        for (const count of [0, 1, 64]) {
          const expected = candidates.slice(0, count).find(({ hash }) => less(hash, target));
          const previousInterpreted = words(exports.memory, context.output, 4);
          const previousGenerated = words(generated.memory, 16400, 4);
          assert.equal(context.search(...start, count, ...target), expected ? 0 : -1);
          assert.equal(generated.search(...start, count, ...target), expected ? 0 : -1);
          if (expected) {
            assert.deepEqual(words(exports.memory, context.output, 4), [...expected.nonce, ...expected.hash]);
            assert.deepEqual(words(generated.memory, 16400, 4), [...expected.hash, ...expected.nonce]);
          } else {
            assert.deepEqual(words(exports.memory, context.output, 4), previousInterpreted);
            assert.deepEqual(words(generated.memory, 16400, 4), previousGenerated);
          }
        }
      }
    }
  } finally {
    assert.equal(context.destroy(), 0);
  }
});

test("ABI rejects invalid handles and seed lengths and keeps instances independent", async () => {
  const exports = await loadHashWx();
  const first = createHashWx(exports, seedA);
  const second = createHashWx(exports, seedB);
  try {
    assert.equal(exports.hx_create(0, 31), -1);
    assert.equal(exports.hx_create(0, 33), -1);
    for (const handle of [-1, 100000]) {
      assert.equal(exports.hx_hash(handle, 0, 0, first.output), -1);
      assert.equal(exports.hx_search(handle, 0, 0, 1, 0, 0, first.output), -1);
      assert.equal(exports.hx_module_ptr(handle), 0);
      assert.equal(exports.hx_module_len(handle), 0);
      assert.equal(exports.hx_destroy(handle), -1);
    }
    const bytes = first.moduleBytes();
    for (let i = 0; i < 3; i++) {
      assert.deepEqual(first.hash(123456, 0), vectors[1][2]);
      assert.deepEqual(second.hash(123456, 0), vectors[2][2]);
    }
    assert.deepEqual(bytes, first.moduleBytes());
  } finally {
    assert.equal(first.destroy(), 0);
    assert.equal(exports.hx_destroy(first.handle), -1);
    assert.equal(second.destroy(), 0);
  }
});
