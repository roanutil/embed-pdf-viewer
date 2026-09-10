import assert from 'node:assert/strict';
import { test } from 'node:test';
import { canonicalFor, compare } from './gate.mjs';

test('arm d agrees with arm a, and matches arm c byte for byte', async () => {
  const a = await canonicalFor('a');
  const c = await canonicalFor('c');
  const d = await canonicalFor('d');
  // gate.mjs's compare() returns { ok, compared }, not a bare boolean; the
  // brief's literal `assert.equal(compare(...), true)` predates that shape.
  assert.equal(compare('a', a, 'd', d).ok, true);
  assert.equal(
    JSON.stringify(c.geometry),
    JSON.stringify(d.geometry),
    'arms c and d write the same record layout and must agree exactly',
  );
});
