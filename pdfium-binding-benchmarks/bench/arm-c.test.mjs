import assert from 'node:assert/strict';
import { test } from 'node:test';
import { canonicalFor, compare } from './gate.mjs';

test('arm c agrees with arm a on every workload they share', async () => {
  const a = await canonicalFor('a');
  const c = await canonicalFor('c');
  // gate.mjs's compare() returns { ok, compared }, not a bare boolean; the
  // brief's literal `assert.equal(compare(...), true)` predates that shape.
  assert.equal(compare('a', a, 'c', c).ok, true);
  assert.equal(a.glyphCount, c.glyphCount);
});
