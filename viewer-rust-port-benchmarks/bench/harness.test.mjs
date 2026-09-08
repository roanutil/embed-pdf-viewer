import assert from 'node:assert/strict';
import { test } from 'node:test';
import { displayListDifferences, normalizeDisplayList } from './harness.mjs';

const item = (id, x, extra = {}) => ({
  id,
  quad: [{ x, y: 0 }, { x: 1, y: 0 }, { x: 1, y: 1 }, { x: 0, y: 1 }],
  color: '#c00',
  selected: false,
  projected: false,
  ...extra,
});

test('coordinates that straddle a rounding boundary still agree', () => {
  // 5e-10 rounds up to 1e-9 and a value 1e-19 below it rounds down to 0: the
  // same answer to any tolerance, but different bytes after rounding.
  const a = normalizeDisplayList([item('s1', 5e-10)]);
  const b = normalizeDisplayList([item('s1', 4.999999999e-10)]);
  assert.notEqual(JSON.stringify(a), JSON.stringify(b), 'the byte compare should disagree here');
  assert.deepEqual(displayListDifferences(a, b), []);
});

test('a real geometric difference is reported by item index', () => {
  const a = normalizeDisplayList([item('s1', 0), item('s2', 5)]);
  const b = normalizeDisplayList([item('s1', 0), item('s2', 5.00001)]);
  assert.deepEqual(displayListDifferences(a, b), [1]);
});

test('non-numeric fields are compared exactly, and length mismatches count', () => {
  const a = normalizeDisplayList([item('s1', 0), item('s2', 0)]);
  assert.deepEqual(displayListDifferences(a, normalizeDisplayList([item('s1', 0, { color: '#00c' }), item('s2', 0)])), [0]);
  assert.deepEqual(displayListDifferences(a, normalizeDisplayList([item('s1', 0, { selected: true }), item('s2', 0)])), [0]);
  assert.deepEqual(displayListDifferences(a, normalizeDisplayList([item('s1', 0)])), [1]);
  assert.deepEqual(displayListDifferences(a, normalizeDisplayList([item('s1', 0), item('s3', 0)])), [1]);
});
