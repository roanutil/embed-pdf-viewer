/**
 * Reports each suite's state against its vector file without stopping at the
 * first failure: does not import, throws, stale, or unchanged. For catching
 * up with a TypeScript change, where `vectors:check` stops at the first suite
 * that cannot load.
 *
 *   pnpm --filter @embedpdf/tooling-plugin-conformance exec tsx --conditions=development src/staleness.ts
 */
import { readdir } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { emit, type Suite } from './vectors';

process.env.TZ = 'UTC';

const suitesDir = join(dirname(fileURLToPath(import.meta.url)), 'suites');
const rows: [string, string, string][] = [];
const log = console.log;

// Import every module before emitting any: scenario suites register the
// factories other suites name, as generate.ts relies on.
const loaded: Suite[] = [];
for (const file of (await readdir(suitesDir)).filter((f) => f.endsWith('.ts')).sort()) {
  let suites: Suite[] | undefined;
  try {
    suites = ((await import(pathToFileURL(join(suitesDir, file)).href)) as { suites?: Suite[] }).suites;
  } catch (e) {
    rows.push(['no-import', file, String((e as Error).message).split('\n')[0]]);
    continue;
  }
  if (!suites) rows.push(['no-import', file, 'exports no suites']);
  else loaded.push(...suites);
}
for (const suite of loaded) {
  process.exitCode = 0;
  // Console untouched: scenario suites capture console output into vectors.
  try {
    await emit([suite], 'check', suite.file);
    rows.push([process.exitCode ? 'stale' : 'unchanged', suite.file, '']);
  } catch (e) {
    rows.push(['throws', suite.file, String((e as Error).message).split('\n')[0]]);
  }
}
process.exitCode = 0;
for (const [state, name, detail] of rows) log(`${state.padEnd(10)} ${name}${detail ? `  ${detail}` : ''}`);
const counts = new Map<string, number>();
for (const [state] of rows) counts.set(state, (counts.get(state) ?? 0) + 1);
log([...counts].map(([k, v]) => `${k}: ${v}`).join(', '));
