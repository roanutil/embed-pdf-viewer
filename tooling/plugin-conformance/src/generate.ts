/**
 * Writes every vector file under `vectors/`, or with `--check` fails if any
 * would change, if a file there has no suite, or if no Rust source names it.
 *
 *   pnpm --filter @embedpdf/tooling-plugin-conformance run vectors
 *   pnpm --filter @embedpdf/tooling-plugin-conformance run vectors:check
 *   pnpm --filter @embedpdf/tooling-plugin-conformance run vectors -- --only kernel/
 *
 * `--compared-only` drops every scenario case whose trace carries no state
 * (`snapshot: 'none'`), runs the rest, and writes nothing. `coverage.ts` runs
 * it to find the lines only those cases reach.
 *
 * Suites are discovered: every module in `src/suites/` exports `suites`. A new
 * crate's generator is a new file there and touches nothing else.
 */
import { readdir } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { emit, type Suite } from './vectors';

// The oracle's wall clock is faked per case, but its time zone is the host's
// unless pinned: `getTimezoneOffset` (the actions plugin's UTC offset, date
// formatting) would otherwise write a different vector on every machine. The
// Rust side takes the offset as an input and replays with zero.
process.env.TZ = 'UTC';

const suitesDir = join(dirname(fileURLToPath(import.meta.url)), 'suites');

const argv = process.argv.slice(2);
const check = argv.includes('--check');
const comparedOnly = argv.includes('--compared-only');
const onlyAt = argv.indexOf('--only');
const only = onlyAt >= 0 ? argv[onlyAt + 1] : undefined;

const suites: Suite[] = [];
for (const file of (await readdir(suitesDir)).filter((f) => f.endsWith('.ts')).sort()) {
  const module = (await import(pathToFileURL(join(suitesDir, file)).href)) as { suites?: Suite[] };
  if (!module.suites) throw new Error(`src/suites/${file} exports no suites`);
  suites.push(...module.suites);
}
if (comparedOnly) {
  const compared = suites.map((suite) =>
    suite.kind === 'scenario' ? { ...suite, cases: suite.cases.filter((c) => c.snapshot !== 'none') } : suite,
  );
  await emit(compared, 'render', only);
} else {
  await emit(suites, check ? 'check' : 'write', only);
}
