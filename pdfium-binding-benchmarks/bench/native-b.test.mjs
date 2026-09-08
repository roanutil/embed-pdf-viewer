import assert from 'node:assert/strict';
import { test } from 'node:test';
import { canonicalForNative, compare } from './gate.mjs';

test('native arm b agrees with native arm a on every workload they share', async () => {
  const a = await canonicalForNative('a');
  const b = await canonicalForNative('b');
  // compare()'s name arguments are looked up against WORKLOAD_TABLE's `arms`
  // lists, which spell arm ids as 'a'/'b'/'c'/'d' regardless of loader, so
  // these must be the bare ids, not 'native-a'/'native-b' -- a label of
  // that shape would match nothing in `arms` and compare zero workloads.
  // gate.mjs's compare() returns { ok, compared }, not a bare boolean.
  const result = compare('a', a, 'b', b);
  assert.equal(result.ok, true);
  assert.ok(result.compared > 0, 'expected at least one workload to be compared');
  assert.equal(a.glyphCount, b.glyphCount);
});
