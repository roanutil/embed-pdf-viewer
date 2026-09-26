/**
 * Writes the vector files of the named suite modules only, so one suite can be
 * regenerated while others do not yet import.
 *
 *   pnpm --filter @embedpdf/tooling-plugin-conformance exec tsx --conditions=development src/emit-one.ts kernel metadata
 */
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { emit, type Suite } from './vectors';

process.env.TZ = 'UTC';
const suitesDir = join(dirname(fileURLToPath(import.meta.url)), 'suites');
const check = process.argv.includes('--check');
for (const name of process.argv.slice(2).filter((a) => a !== '--check')) {
  const module = (await import(pathToFileURL(join(suitesDir, `${name}.ts`)).href)) as { suites: Suite[] };
  await emit(module.suites, check);
}
