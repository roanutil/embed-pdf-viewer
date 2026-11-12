// Write tooling/plugin-conformance/historical/catchup-2026-11-12-measurement/accounting.json:
// each measurement case task-12b2b3-i's accounting left current (64), mapped to the case
// that carries it now, retired with its reason, and the cases added since.
//
// Run from tooling/plugin-conformance (it imports the suite as the tests do):
//   cp <this> test/zz-accounting.ts && node --import=tsx --conditions=development test/zz-accounting.ts
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { suites } from '../src/suites/measurement';
import { canonical } from '../src/canonical';

const sha = (data: string | Buffer) => createHash('sha256').update(data).digest('hex');
const previousPath = 'tooling/plugin-conformance/historical/task-12b2b3-i/accounting.json';
const previousBytes = readFileSync(new URL('../historical/task-12b2b3-i/accounting.json', import.meta.url));
const previous = JSON.parse(previousBytes.toString()) as {
  mappings: { after: { file: string; name: string; inputSha256: string } }[];
  additions: { file: string; name: string; inputSha256: string }[];
};
const rows = [...previous.mappings.map((m) => m.after), ...previous.additions].map(
  ({ file, name, inputSha256 }) => ({ file, name, inputSha256 }),
);
const current = new Map<string, { file: string; name: string; inputSha256: string }>();
for (const s of suites)
  for (const c of s.cases)
    current.set(s.file + ':' + c.name, {
      file: s.file,
      name: c.name,
      inputSha256: sha(JSON.stringify(canonical(c))),
    });

const LAYOUT = ['c7f8d75ff', '8bc6546e5'];
const GENERATOR = 'generator c74ec3e9d (CU-C3-D3)';
const reasonFor = (file: string, name: string, same: boolean): { reason: string; upstream: string[] } => {
  if (same) return { reason: 'Input unchanged.', upstream: [] };
  if (file === 'plugins/measurement.json')
    return {
      reason: `Rewritten for scales on the page layout and a scale change as one change: pages carry their viewports, and the engine answers pages.setScale through apply in place of page.measure reads and writes (${GENERATOR}).`,
      upstream: LAYOUT,
    };
  if (name === 'reduce-page-scale')
    return {
      reason: `Its state argument lost loadErrors with the viewport mirror (${GENERATOR}), and its covers text names the layout's viewports (finding 019a6653-693d-4d30-9c72-4424baa54cba/R2).`,
      upstream: ['8bc6546e5'],
    };
  return {
    reason: `Its state argument lost loadErrors with the viewport mirror (${GENERATOR}).`,
    upstream: ['8bc6546e5'],
  };
};
const RETIRED: Record<string, string> = {
  'viewport-reads-fail-refresh-and-rehydrate':
    "The viewports page mirror, its failed reads and retries are gone: a page's viewports come with its layout. scale-events-move-the-layout covers what replaced them.",
  'refresh-epochs':
    "The mirror's read epochs are gone with it. scale-events-move-the-layout covers a desync re-reading the page list.",
  'reduce-unknown': 'clearLoadError, which it called on an absent page entry, is gone with loadErrors.',
  'page-scale-loading': 'pageScaleOf is gone: a page of an open document is always ready.',
  'page-scale-failed-known': 'pageScaleOf and its read error are gone.',
  'page-scale-failed-empty': 'pageScaleOf and its read error are gone.',
};
const ADDED: Record<string, string> = {
  'scale-events-move-the-layout':
    "A page's scale comes with its layout: canMeasure and ensureLoaded answer at once, another session's pages.scaleSet moves it, stream.desynced re-reads the page list (a4805a492), and document.versioned changes nothing.",
};

const seen = new Set<string>();
const mappings = rows.map((before) => {
  const key = before.file + ':' + before.name;
  const after = current.get(key) ?? null;
  if (after) seen.add(key);
  if (!after) {
    const reason = RETIRED[before.name];
    if (!reason) throw new Error(`no retirement reason for ${key}`);
    return { before, after: null, disposition: 'retired', reason, upstream: ['8bc6546e5'] };
  }
  const same = after.inputSha256 === before.inputSha256;
  return { before, after, disposition: same ? 'unchanged' : 'rewritten', ...reasonFor(before.file, before.name, same) };
});
const additions = [...current.entries()]
  .filter(([key]) => !seen.has(key))
  .map(([, row]) => {
    const reason = ADDED[row.name];
    if (!reason) throw new Error(`no addition reason for ${row.file}:${row.name}`);
    return { ...row, reason, upstream: ['8bc6546e5', 'a4805a492'] };
  });
const out = {
  catchUp: '2026-11-12',
  upstream: 'ed064e0c867e7688234ad36c0350377544ef93f9',
  previousAccounting: { path: previousPath, sha256: sha(previousBytes) },
  previous: rows.length,
  current: current.size,
  unchanged: mappings.filter((m) => m.disposition === 'unchanged').length,
  rewritten: mappings.filter((m) => m.disposition === 'rewritten').length,
  retired: mappings.filter((m) => m.disposition === 'retired').length,
  added: additions.length,
  mappings,
  additions,
};
const dir = new URL('../historical/catchup-2026-11-12-measurement/', import.meta.url);
mkdirSync(dir, { recursive: true });
writeFileSync(new URL('accounting.json', dir), JSON.stringify(out, null, 2) + '\n');
console.log(out.previous, out.current, out.unchanged, out.rewritten, out.retired, out.added);
