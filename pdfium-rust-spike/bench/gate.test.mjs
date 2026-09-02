import assert from 'node:assert/strict';
import { test } from 'node:test';
import { canonicalFor, compare } from './gate.mjs';

test('arm a produces a stable canonical answer for every workload it runs', async () => {
  const first = await canonicalFor('a');
  const second = await canonicalFor('a');

  assert.ok(first.glyphCount > 500);
  assert.equal(JSON.stringify(first.geometry), JSON.stringify(second.geometry));
  assert.equal(first.text, second.text);
  assert.equal(JSON.stringify(first['render-1x']), JSON.stringify(second['render-1x']));
  assert.equal(JSON.stringify(first['render-4x']), JSON.stringify(second['render-4x']));
  assert.equal(JSON.stringify(first.search), JSON.stringify(second.search));

  assert.equal(first.geometry.length, first.glyphCount);
  assert.ok(first.text.length > 100, 'expected real text on the fixture page');
  assert.ok(first.search.length > 0, 'expected the search term to hit');

  assert.notEqual(first['render-1x'].hash, '00000000');
  assert.notEqual(first['render-1x'].hash, first['render-4x'].hash);
  assert.ok(first['render-1x'].firstNonWhite >= 0, 'expected non-white pixels in the 1x render');
  assert.ok(first['render-4x'].firstNonWhite >= 0, 'expected non-white pixels in the 4x render');
  assert.equal(first['render-1x'].len, first['render-1x'].len | 0, 'len must be an integer');

  // null-peek/null-readbytes (Finding 2) and geometry-copyout/render-copyout-4x
  // (Finding 6) declare `canonical: null` and must never enter the canonical
  // answer at all.
  for (const id of ['null-peek', 'null-readbytes', 'geometry-copyout', 'render-copyout-4x']) {
    assert.ok(!(id in first), `expected ${id} to be excluded from the canonical answer`);
  }
});

const SHARED_FIXTURE = {
  glyphCount: 10,
  pageIndex: 4,
  'null-call': [1, 2],
  geometry: [],
  text: 'hello',
  'render-1x': { hash: 'a', len: 1, firstNonWhite: 0 },
  'render-4x': { hash: 'a', len: 1, firstNonWhite: 0 },
  search: [],
};

test('compare() fails when a workload both arms are declared to run is missing from the baseline', () => {
  const originalError = console.error;
  console.error = () => {};
  try {
    // `text` is declared for every arm (WORKLOAD_TABLE), so a and b both
    // owe it. The baseline is missing only `text` — the exact shape of the
    // bug in Finding 1, where a key present only in `actual` was never
    // examined because the old `compare()` only walked `baseline`'s keys.
    const { text: _omit, ...baseline } = SHARED_FIXTURE;
    const result = compare('a', baseline, 'b', { ...SHARED_FIXTURE });
    assert.equal(result.ok, false);
  } finally {
    console.error = originalError;
  }
});

test('compare() fails when a workload both arms are declared to run is missing from the actual side', () => {
  const originalError = console.error;
  console.error = () => {};
  try {
    const { text: _omit, ...actual } = SHARED_FIXTURE;
    const result = compare('a', { ...SHARED_FIXTURE }, 'b', actual);
    assert.equal(result.ok, false);
  } finally {
    console.error = originalError;
  }
});

test('compare() reports how many workloads it actually compared', () => {
  const originalError = console.error;
  console.error = () => {};
  try {
    // WORKLOAD_TABLE owes a and b: null-call, geometry, text, render-1x,
    // render-4x, search — six ids with a real canonical answer.
    const result = compare('a', { ...SHARED_FIXTURE }, 'b', { ...SHARED_FIXTURE });
    assert.equal(result.ok, true);
    assert.equal(result.compared, 6);
  } finally {
    console.error = originalError;
  }
});
