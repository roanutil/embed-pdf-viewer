/**
 * Emits `vectors/annotation/read.json`: what the TypeScript engine answers for
 * `annotations.list` (the old `listRawAll`), per page, per annotation.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-read
 *   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-read:check
 *
 * G7's gate for the reads that touch PDFium, as opposed to the pure tables in
 * `generate-annotation-vectors.ts`. `crates/epdf-engine/tests/
 * annotation_reader.rs` replays it.
 *
 * The raw path rather than the full one, deliberately. `listRawAll` reads off
 * the document pointer with no page acquisition, which is the path a Rust
 * reader can run without owning a session, and both paths share
 * `collectPageAnnotations`, so the per-annotation answer is the same object.
 *
 * The whole DTO is recorded, not the base shell, even though only the shell is
 * ported. A vector that held what Rust can already produce would have to be
 * regenerated every time a subtype reader lands, and the diff would be the
 * thing under review rather than the thing being checked. The Rust runner
 * instead declares which fields it produces and fails when the vector grows
 * one it does not.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createLocalEngine } from '../../../packages/engine/main/src/index';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'annotation');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

interface ReadCase {
  readonly name: string;
  readonly fixture: string;
  readonly covers: string;
  readonly password?: string;
}

const CASES: readonly ReadCase[] = [
  {
    name: 'base',
    fixture: 'tooling/engine-conformance/fixtures/annotation_base.pdf',
    covers:
      'Every distinction the base shell makes: an indirect annotation, a direct one with /NM, a direct one without, the three /RT cases, present-but-empty against absent strings, /EMBD_Metadata, a reversed /Rect, and a /Sound that no reader claims.',
  },
  {
    name: 'kinds',
    fixture: 'tooling/engine-conformance/fixtures/annotation_kinds.pdf',
    covers:
      'One annotation per geometry subtype, paired so each resolved default sits beside an explicit entry: a decorated square against a bare circle, a strikeout with /IT against one without, highlighter ink against pen ink, and a highlight with no /C against an underline with one.',
  },
  {
    name: 'form',
    fixture: 'tooling/engine-conformance/fixtures/annotation_appearances.pdf',
    covers:
      'A widget reachable from /AcroForm beside an inert one, which is what the widget-to-field join has to tell apart. Nothing else in the corpus has a form at all.',
  },
  {
    name: 'no-annotations',
    fixture: 'tooling/engine-conformance/fixtures/rich_metadata.pdf',
    covers:
      'A document with no /Annots at all. The empty listing is a different answer from a failed one and nothing else here would catch a reader that confused them.',
  },
  {
    name: 'report',
    fixture: 'pdfium-multiplatform-prototype/fixtures/report.pdf',
    covers: 'A real multi-page document, whose annotations are whatever its producer wrote.',
  },
];

async function readCase(entry: ReadCase): Promise<unknown> {
  const bytes = new Uint8Array(await readFile(join(repoRoot, entry.fixture)));
  const engine = createLocalEngine({
    runtime: {
      prefer: 'wasm',
      wasm: {
        print: (message: string) => process.stderr.write(`${message}\n`),
        printErr: (message: string) => process.stderr.write(`${message}\n`),
      },
    },
  });
  try {
    // The password moved to `OpenOptions` in 09c24496f ("Engine API v3,
    // round 2").
    const doc = await engine.open(
      { kind: 'bytes', id: `annotation:${entry.name}`, bytes },
      { scope: ['*'], password: entry.password },
    );
    try {
      // `listRawAll` became `annotations.list()` in 83f6598d2 ("Engine API
      // v3, round 3a"), still the raw read. It returns one flat list, each
      // annotation naming its `page`, beside the pages it covered; the
      // per-page `pageState` went with the snapshot types. The vector keeps
      // one entry per page, in the list's page order. Widgets left the
      // annotation list for `forms.list()` in 1233d1349 ("Read and write
      // widgets through the form").
      const list = await doc.annotations.list();
      const pages = list.pages.map((page) => ({
        page,
        annotations: list.annotations.filter(
          (annotation) => annotation.page.objectNumber === page.objectNumber,
        ),
      }));
      return { ...entry, pages };
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
}

/**
 * A document session id is minted per open and appears in every revision
 * token, so a verbatim recording would differ on every run and the `:check`
 * script would never pass. Rust mints its own anyway, so the value carries no
 * comparison: what matters is that the token is THERE and that its page and
 * generation agree. The id is replaced rather than deleted so a reader of the
 * file can still see the shape.
 */
const SESSION_PLACEHOLDER = '<session>';

function normalizeSessionIds(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(normalizeSessionIds);
  if (value === null || typeof value !== 'object') return value;
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>).map(([key, entry]) =>
      key === 'docSessionId' ? [key, SESSION_PLACEHOLDER] : [key, normalizeSessionIds(entry)],
    ),
  );
}

function serialize(cases: readonly unknown[]): string {
  const head = JSON.stringify({ schemaVersion: SCHEMA_VERSION, kind: 'annotation.read' }, null, 2)
    .replace(/\n}$/, ',');
  const body = cases
    .map((entry) => `  ${JSON.stringify(normalizeSessionIds(entry))}`)
    .join(',\n');
  return `${head}\n  "cases": [\n${body}\n  ]\n}\n`;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const cases: unknown[] = [];
  for (const entry of CASES) cases.push(await readCase(entry));
  const text = serialize(cases);
  const absolute = join(vectorsRoot, 'read.json');

  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write('  ok      annotation/read.json\n');
    return;
  }
  if (check) {
    process.stdout.write(`  DIFFERS annotation/read.json${current === undefined ? ' (missing)' : ''}\n`);
    process.stderr.write(
      '\nAnnotation read vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run annotation-read\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write('  written annotation/read.json\n');
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-annotation-read-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
