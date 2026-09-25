/**
 * Writes every vector file under `vectors/`, or with `--check` fails if any
 * would change.
 *
 *   pnpm --filter @embedpdf/tooling-plugin-conformance run vectors
 *   pnpm --filter @embedpdf/tooling-plugin-conformance run vectors:check
 *   pnpm --filter @embedpdf/tooling-plugin-conformance run vectors -- --only kernel/
 *
 * Suites are discovered: every module in `src/suites/` exports `suites`. A new
 * crate's generator is a new file there and touches nothing else.
 */
import { readdir } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { emit, type Suite } from './vectors';

const suitesDir = join(dirname(fileURLToPath(import.meta.url)), 'suites');

const argv = process.argv.slice(2);
const check = argv.includes('--check');
const onlyAt = argv.indexOf('--only');
const only = onlyAt >= 0 ? argv[onlyAt + 1] : undefined;

const suites: Suite[] = [];
for (const file of (await readdir(suitesDir)).filter((f) => f.endsWith('.ts')).sort()) {
  const module = (await import(pathToFileURL(join(suitesDir, file)).href)) as { suites?: Suite[] };
  if (!module.suites) throw new Error(`src/suites/${file} exports no suites`);
  suites.push(...module.suites);
}
await emit(suites, check, only);
