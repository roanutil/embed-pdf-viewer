import assert from 'node:assert/strict';
import { test } from 'node:test';
import { canonicalFor, compare } from './gate.mjs';
import { GATE_FIXTURES } from './fixture.mjs';

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

  // The timing fixture (report.pdf page 4) is entirely upright, so it never
  // exercises the rotation/ascent-flip branch or the SPACE flag branch that
  // all four arms implement. `rotated-p0` does; this is the assertion that
  // proves the gate actually runs that branch rather than merely declaring
  // a fixture that could. Each geometry tuple is
  // [flags, objectKeyChanged, fontSize, rotation, ascentFlip, upright, ...]
  // (see `canonicalGeometry` in bench/workloads.mjs), so rotation is index 3.
  const rotated = first.fixtures['rotated-p0'];
  assert.ok(rotated, 'expected the rotated fixture to be present in the canonical answer');
  assert.ok(rotated.glyphCount > 0);
  assert.ok(
    rotated.geometry.some((g) => g[3] !== 0),
    'expected at least one glyph with a non-zero rotation on the rotated fixture',
  );

  // Every matrix in rotated_text.pdf has a positive determinant, so neither
  // fixture above ever set ascentFlip (index 4), and the producer bit that
  // arms c and d OR into the record was never once compared against arm a.
  // That is how FLAG_ASCENT_FLIP sat on PDFium's SYNTHESIZED bit (fixed in
  // b26d9f314) with the gate green. `flipped-p0` is a page whose text
  // matrices have det < 0, and this is the assertion that the gate really
  // runs that branch.
  const flipped = first.fixtures['flipped-p0'];
  assert.ok(flipped, 'expected the flipped fixture to be present in the canonical answer');
  assert.ok(
    flipped.geometry.some((g) => g[4] === 1),
    'expected at least one glyph with ascentFlip set on the flipped fixture',
  );
  assert.ok(
    flipped.geometry.some((g) => g[4] === 0 && g[5] === 0),
    'expected a non-upright glyph WITHOUT ascentFlip too, so the bit is not simply "non-upright"',
  );
});

/** Shaped like a `canonicalFor()` result: a `.fixtures` map keyed by label. */
const SHARED_FIXTURE = {
  fixtures: {
    'report-p4': {
      glyphCount: 10,
      pageIndex: 4,
      'null-call': [1, 2],
      geometry: [],
      text: 'hello',
      'render-1x': { hash: 'a', len: 1, firstNonWhite: 0 },
      'render-4x': { hash: 'a', len: 1, firstNonWhite: 0 },
      search: [],
    },
    'rotated-p0': {
      glyphCount: 5,
      pageIndex: 0,
      'null-call': [3, 4],
      geometry: [],
      text: '',
      'render-1x': { hash: 'b', len: 1, firstNonWhite: 0 },
      'render-4x': { hash: 'b', len: 1, firstNonWhite: 0 },
      search: [],
    },
    'flipped-p0': {
      glyphCount: 3,
      pageIndex: 0,
      'null-call': [5, 6],
      geometry: [],
      text: '',
      'render-1x': { hash: 'c', len: 1, firstNonWhite: 0 },
      'render-4x': { hash: 'c', len: 1, firstNonWhite: 0 },
      search: [],
    },
  },
};

// Sanity check on the fixture above: it must actually cover every fixture
// the gate compares, or these tests would silently stop exercising the
// per-fixture loop in `compare()` if GATE_FIXTURES ever grew a new entry.
test('SHARED_FIXTURE covers every gate fixture', () => {
  for (const { label } of GATE_FIXTURES) {
    assert.ok(label in SHARED_FIXTURE.fixtures, `SHARED_FIXTURE is missing fixture ${label}`);
  }
});

test('compare() fails when a workload both arms are declared to run is missing from the baseline', () => {
  const originalError = console.error;
  console.error = () => {};
  try {
    // `text` is declared for every arm (WORKLOAD_TABLE), so a and b both
    // owe it. The baseline is missing only `text`, on one fixture — the
    // exact shape of the bug in Finding 1, where a key present only in
    // `actual` was never examined because the old `compare()` only walked
    // `baseline`'s keys.
    const { text: _omit, ...reportBaseline } = SHARED_FIXTURE.fixtures['report-p4'];
    const baseline = { fixtures: { ...SHARED_FIXTURE.fixtures, 'report-p4': reportBaseline } };
    const result = compare('a', baseline, 'b', { fixtures: { ...SHARED_FIXTURE.fixtures } });
    assert.equal(result.ok, false);
  } finally {
    console.error = originalError;
  }
});

test('compare() fails when a workload both arms are declared to run is missing from the actual side', () => {
  const originalError = console.error;
  console.error = () => {};
  try {
    const { text: _omit, ...reportActual } = SHARED_FIXTURE.fixtures['report-p4'];
    const actual = { fixtures: { ...SHARED_FIXTURE.fixtures, 'report-p4': reportActual } };
    const result = compare('a', { fixtures: { ...SHARED_FIXTURE.fixtures } }, 'b', actual);
    assert.equal(result.ok, false);
  } finally {
    console.error = originalError;
  }
});

test('compare() fails when a workload is missing only on the second fixture', () => {
  const originalError = console.error;
  console.error = () => {};
  try {
    // Same bug shape as above, but on `rotated-p0` rather than `report-p4`
    // — proves compare() checks every fixture in GATE_FIXTURES, not just
    // the first one.
    const { text: _omit, ...rotatedBaseline } = SHARED_FIXTURE.fixtures['rotated-p0'];
    const baseline = { fixtures: { ...SHARED_FIXTURE.fixtures, 'rotated-p0': rotatedBaseline } };
    const result = compare('a', baseline, 'b', { fixtures: { ...SHARED_FIXTURE.fixtures } });
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
    // render-4x, search — six ids with a real canonical answer. That count
    // does not multiply by the number of fixtures: it is how many distinct
    // workloads were compared, each of them checked on every fixture.
    const result = compare(
      'a',
      { fixtures: { ...SHARED_FIXTURE.fixtures } },
      'b',
      { fixtures: { ...SHARED_FIXTURE.fixtures } },
    );
    assert.equal(result.ok, true);
    assert.equal(result.compared, 6);
  } finally {
    console.error = originalError;
  }
});
