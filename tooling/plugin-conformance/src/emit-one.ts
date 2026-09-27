/**
 * Writes the vector files of the named suite modules only, so one suite can be
 * regenerated while others do not yet import.
 *
 *   pnpm --filter @embedpdf/tooling-plugin-conformance exec tsx --conditions=development src/emit-one.ts kernel metadata
 */
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { readdir } from 'node:fs/promises';

import { emit, type Suite } from './vectors';

process.env.TZ = 'UTC';
const suitesDir = join(dirname(fileURLToPath(import.meta.url)), 'suites');
const check = process.argv.includes('--check');
// Import every suite first, as generate.ts does: a scenario may name a plugin
// factory that another suite registers (link's cases use interaction).
const modules = new Map<string, { suites?: Suite[] }>();
for (const file of (await readdir(suitesDir)).filter((f) => f.endsWith('.ts')).sort()) {
  modules.set(file.slice(0, -3), await import(pathToFileURL(join(suitesDir, file)).href));
}
for (const name of process.argv.slice(2).filter((a) => a !== '--check')) {
  const module = modules.get(name);
  if (!module?.suites) throw new Error(`src/suites/${name}.ts exports no suites`);
  await emit(module.suites, check);
}
