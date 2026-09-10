import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

export function compareReports(before, after) {
  for (const report of [before, after]) {
    if (report.schemaVersion !== 1 || report.status !== 'passed' || !report.summary?.length) {
      throw new Error('Comparison requires two successful, nonempty schemaVersion 1 reports');
    }
  }
  for (const key of [
    'platform',
    'arch',
    'osRelease',
    'cpu',
    'cpuCount',
    'node',
    'playwright',
    'delivery',
    'fixtureSha256',
    'workers',
  ]) {
    if (before.environment[key] !== after.environment[key])
      throw new Error(`Incompatible environment: ${key}`);
  }
  const key = (row) => JSON.stringify([row.project, row.flow, row.metric]);
  const oldRows = new Map(before.summary.map((row) => [key(row), row]));
  if (oldRows.size !== after.summary.length)
    throw new Error('Reports contain different metrics/flows');
  const versions = (report, project) =>
    [
      ...new Set(
        report.samples
          .filter((s) => s.project === project && s.status === 'passed')
          .map((s) => s.browserVersion),
      ),
    ]
      .sort()
      .join(',');
  const view = (report, project) => {
    const p = report.environment.projects.find((p) => p.name === project);
    return JSON.stringify([p?.viewport, p?.deviceScaleFactor]);
  };
  return after.summary.map((row) => {
    const previous = oldRows.get(key(row));
    if (!previous) throw new Error(`Missing baseline metric: ${key(row)}`);
    if (
      versions(before, row.project) !== versions(after, row.project) ||
      view(before, row.project) !== view(after, row.project)
    ) {
      throw new Error(`Incompatible browser or viewport: ${row.project}`);
    }
    return {
      project: row.project,
      flow: row.flow,
      metric: row.metric,
      before: previous.median,
      after: row.median,
      deltaMs: row.median - previous.median,
      deltaPercent: previous.median === 0 ? null : (row.median / previous.median - 1) * 100,
      beforeN: previous.n,
      afterN: row.n,
    };
  });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.length !== 4)
      throw new Error('Usage: node compare-performance.mjs before.json after.json');
    const rows = compareReports(
      ...process.argv.slice(2).map((p) => JSON.parse(readFileSync(p, 'utf8'))),
    );
    console.log(
      'Median changes in ms; positive means slower. Informational, not a regression gate.',
    );
    console.log('| Project | Flow | Metric | Before | After | Change | Samples |');
    console.log('|---|---|---|---:|---:|---:|---:|');
    for (const row of rows)
      console.log(
        `| ${row.project} | ${row.flow.split(' ')[0]} | ${row.metric} | ${row.before.toFixed(2)} | ${row.after.toFixed(2)} | ${row.deltaPercent === null ? 'n/a' : row.deltaPercent.toFixed(1) + '%'} | ${row.beforeN}/${row.afterN} |`,
      );
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
