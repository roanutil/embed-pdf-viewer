import test from 'node:test';
import assert from 'node:assert/strict';
import { canonicalFor, compare } from './gate.mjs';

test('arm e Swift agrees with arm a and the Rust coarse arm', async () => {
  const [a, c, e] = await Promise.all(['a', 'c', 'e'].map(canonicalFor));
  assert.equal(compare('a', a, 'e', e).ok, true);
  assert.deepEqual(e.fixtures, c.fixtures);
});
