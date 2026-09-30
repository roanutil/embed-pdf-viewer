import { test } from 'node:test';
import assert from 'node:assert/strict';
import { summarize, bootstrapRatio, bootstrapDifference, noiseBand, verdict, calibrate, comparePair, slope } from '../lib/stats.mjs';

test('summarize uses linear-interpolated quantiles', () => {
  const s = summarize([5, 1, 4, 2, 3]);
  assert.equal(s.median, 3);
  assert.equal(s.p95, 4.8);
  assert.equal(s.min, 1);
  assert.equal(s.max, 5);
  assert.equal(s.n, 5);
});

test('summarize rejects non-finite samples', () => {
  assert.throws(() => summarize([1, NaN]), /finite/);
  assert.throws(() => summarize([]), /at least one/);
});

test('bootstrap is deterministic for a seed', () => {
  const a = [10, 11, 9, 10, 12, 10, 11];
  const b = [20, 21, 19, 22, 20, 20, 21];
  assert.deepEqual(bootstrapRatio(a, b, { seed: 7 }), bootstrapRatio(a, b, { seed: 7 }));
});

test('bootstrap brackets a clear 2x', () => {
  const a = [10, 11, 9, 10, 12, 10, 11, 10, 9, 11];
  const b = a.map((x) => x * 2);
  const r = bootstrapRatio(a, b);
  assert.equal(r.ratio, 2);
  assert.ok(r.lo > 1.5 && r.hi < 2.5, JSON.stringify(r));
});

test('verdict respects the noise band and the 3% floor', () => {
  assert.equal(verdict({ ratio: 1.2, lo: 1.1, hi: 1.3 }, 0.05), 'slower');
  assert.equal(verdict({ ratio: 0.8, lo: 0.7, hi: 0.9 }, 0.05), 'faster');
  assert.equal(verdict({ ratio: 1.2, lo: 0.95, hi: 1.3 }, 0.05), 'same');
  assert.equal(verdict({ ratio: 1.04, lo: 1.01, hi: 1.06 }, 0.05), 'same');
  assert.equal(verdict({ ratio: 1.02, lo: 1.01, hi: 1.03 }, 0), 'same');
});

test('an A/A pair with a consistent 2x slowdown fails calibration', () => {
  // Judged against its own band, this used to come out `same`.
  const aa = bootstrapRatio(Array(10).fill(100), Array(10).fill(200));
  assert.deepEqual(calibrate(aa), { ok: false, noise: 1 });
});

test('a clean A/A pair calibrates', () => {
  const a = [100, 102, 98, 101, 99, 100, 103, 97, 100, 101];
  const b = [101, 99, 100, 98, 102, 100, 97, 103, 101, 100];
  const c = calibrate(bootstrapRatio(a, b));
  assert.equal(c.ok, true);
  assert.ok(c.noise < 0.05, String(c.noise));
});

test('comparePair refuses a verdict when A/A failed', () => {
  const clear = { ratio: 2, lo: 1.8, hi: 2.2 };
  assert.equal(comparePair(clear, { ok: false, noise: 0 }), 'unreliable');
  assert.equal(comparePair(clear, { ok: true, noise: 0.04 }), 'slower');
});

test('a ratio refuses zero and negative samples', () => {
  assert.throws(() => bootstrapRatio(Array(10).fill(0), Array(10).fill(0)), /positive/);
  assert.throws(() => bootstrapRatio([1, 2, 3], [-1, 2, 3]), /positive/);
});

test('verdict refuses a non-finite interval', () => {
  assert.throws(() => verdict({ ratio: NaN, lo: NaN, hi: NaN }, 0), /finite/);
  assert.throws(() => calibrate({ kind: 'difference', diff: 0, lo: -Infinity, hi: 0, scale: 0 }), /finite/);
});

test('an all-zero A/A difference calibrates with no noise', () => {
  const aa = bootstrapDifference(Array(10).fill(0), Array(10).fill(0));
  assert.deepEqual(calibrate(aa), { ok: true, noise: 0 });
  const worse = bootstrapDifference(Array(10).fill(0), Array(10).fill(40));
  assert.equal(comparePair(worse, calibrate(aa)), 'slower');
});

test('a signed slope moving from -10 to -5 is worse, not better', () => {
  // As a ratio this is 0.5, which used to read as "faster".
  const d = bootstrapDifference(Array(10).fill(-10), Array(10).fill(-5));
  assert.equal(d.diff, 5);
  assert.equal(verdict(d, 0), 'slower');
});

test('noiseBand is the widest distance from 1', () => {
  assert.equal(noiseBand({ lo: 0.96, hi: 1.02 }).toFixed(2), '0.04');
});

test('slope of a line', () => {
  assert.equal(slope([10, 12, 14, 16]), 2);
  assert.equal(slope([5, 5, 5]), 0);
});
