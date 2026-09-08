import assert from 'node:assert/strict';
import { test } from 'node:test';
import { canonicalForNative, compare } from './gate.mjs';

test('native arms c and d agree with native arm a, and with each other', async () => {
  const a = await canonicalForNative('a');
  const c = await canonicalForNative('c');
  const d = await canonicalForNative('d');
  assert.equal(compare('a', a, 'c', c).ok, true);
  assert.equal(compare('a', a, 'd', d).ok, true);
  // arms c and d write the same record layout and must agree exactly, not
  // merely both agree with arm a -- matching what bench/arm-d.test.mjs
  // asserts for the wasm arms.
  assert.equal(JSON.stringify(c.geometry), JSON.stringify(d.geometry));
});
