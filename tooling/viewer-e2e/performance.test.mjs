import assert from 'node:assert/strict';
import test from 'node:test';
import { distribution, summarize } from './performance.ts';
import { compareReports } from './compare-performance.mjs';

test('median and nearest-rank p95 retain outliers without mutating samples', () => {
  const values = [100, 1, 3, 2];
  assert.deepEqual(distribution(values), { n: 4, min: 1, median: 2.5, p95: 100, max: 100 });
  assert.deepEqual(values, [100, 1, 3, 2]);
  assert.equal(distribution(Array.from({ length: 100 }, (_, i) => i + 1)).p95, 95);
  assert.throws(() => distribution([]));
  assert.throws(() => distribution([NaN]));
});

test('summaries separate flows and browsers and exclude failed/retried measurements', () => {
  const sample = {
    project: 'chromium-typescript',
    flow: 'F1 open',
    retry: 0,
    status: 'passed',
    metrics: { flowWallMs: 10 },
  };
  const summary = summarize([
    sample,
    { ...sample, metrics: { flowWallMs: 20 } },
    { ...sample, status: 'failed', metrics: { flowWallMs: 999 } },
    { ...sample, retry: 1, metrics: { flowWallMs: 999 } },
    { ...sample, project: 'firefox-typescript' },
    { ...sample, flow: 'F2 scroll' },
  ]);
  assert.equal(summary.length, 3);
  assert.equal(summary[0].n, 2);
  assert.equal(summary[0].median, 15);
});

test('comparisons reject incompatible/failed runs and report the direction of change', () => {
  const report = {
    schemaVersion: 1,
    status: 'passed',
    environment: { delivery: 'workspace-dev', projects: [] },
    samples: [],
    summary: [
      { project: 'chromium-typescript', flow: 'F1 open', metric: 'flowWallMs', n: 5, median: 10 },
    ],
  };
  const faster = { ...report, summary: [{ ...report.summary[0], median: 8 }] };
  assert.ok(Math.abs(compareReports(report, faster)[0].deltaPercent + 20) < 1e-10);
  assert.throws(() => compareReports(report, { ...faster, status: 'failed' }));
  assert.throws(() =>
    compareReports(report, {
      ...faster,
      environment: { ...report.environment, delivery: 'packed-production' },
    }),
  );
  assert.throws(() =>
    compareReports(report, { ...faster, summary: [{ ...faster.summary[0], flow: 'F2 scroll' }] }),
  );
  assert.equal(
    compareReports({ ...report, summary: [{ ...report.summary[0], median: 0 }] }, faster)[0]
      .deltaPercent,
    null,
  );
});
