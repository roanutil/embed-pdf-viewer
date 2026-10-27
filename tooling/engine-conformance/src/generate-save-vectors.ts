/**
 * Emits `vectors/save/roundtrip.json`: what a saved document turns out to be
 * when it is opened again.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run save
 *   pnpm --filter @embedpdf/tooling-engine-conformance run save:check
 *
 * G10's save gate. `crates/epdf-engine/tests/document_save.rs` replays it.
 *
 * Semantic, for the reason `multiplatform-port-order.md` G10 gives in so many
 * words: the conformance suites do not establish byte-stable PDF
 * serialization. What is recorded is what the reopened document IS, which is
 * the thing a save is supposed to preserve.
 *
 * Both modes for every case. An incremental save appends a revision to the
 * original bytes and a full save rewrites the file, so they produce different
 * files that have to mean the same thing, and a reader that ignored the mode
 * would agree with one of them.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createLocalEngine } from '../../../packages/engine/main/src/index';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'save');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

interface SaveCase {
  readonly name: string;
  readonly fixture: string;
  readonly covers: string;
  readonly password?: string;
}

const CASES: readonly SaveCase[] = [
  {
    name: 'report',
    fixture: 'pdfium-multiplatform-prototype/fixtures/report.pdf',
    covers: 'A real fourteen-page document with text on every page.',
  },
  {
    name: 'rotated',
    fixture: 'tooling/engine-conformance/fixtures/rotated_pages.pdf',
    covers:
      'Four pages at /Rotate 0, 90, 180 and 270, so a save that rebuilt page dictionaries rather than copying them would flatten the rotation.',
  },
  {
    name: 'annotations',
    fixture: 'tooling/engine-conformance/fixtures/annotation_kinds.pdf',
    covers:
      'Every annotation subtype, so a save that dropped /Annots would still produce a document that opens and renders.',
  },
  {
    name: 'rich-metadata',
    fixture: 'tooling/engine-conformance/fixtures/rich_metadata.pdf',
    covers:
      'An Info dictionary with an absent key, a present-but-empty one and a UTF-16 title, which is what a save has to carry across unchanged.',
  },
  {
    name: 'encrypted',
    fixture: 'tooling/engine-conformance/fixtures/encrypted-r4-aes-128.pdf',
    covers:
      'An encrypted document. Saving it produces a file that opens with the same password, which is the case most likely to come out readable-but-wrong.',
    password: 'user',
  },
];

interface Reopened {
  readonly pageCount: number;
  readonly pages: readonly {
    readonly width: number;
    readonly height: number;
    readonly rotation: number;
    readonly text: string;
    readonly annotationCount: number;
  }[];
  readonly title: string | null;
  readonly author: string | null;
}

async function describe(
  engine: ReturnType<typeof createLocalEngine>,
  id: string,
  bytes: Uint8Array,
  password: string | undefined,
): Promise<Reopened> {
  // The password moved to `OpenOptions` in 09c24496f ("Engine API v3, round
  // 2"). `metadata.read`, `text.read` and `annotations.listRawAll` became
  // `metadata.get`, `text.get` and `annotations.list` in 83f6598d2 ("Engine
  // API v3, round 3a"); the list is flat, each annotation naming its page.
  // Pages are addressed by `PageRef` (`page.ref`), as they already were
  // before this catch-up (9f13cea3c, "Address pages by PageRef everywhere").
  const doc = await engine.open({ kind: 'bytes', id, bytes }, { scope: ['*'], password });
  try {
    const listing = await doc.pages.list();
    const metadata = await doc.metadata.get();
    const annotations = await doc.annotations.list();
    const byPage = new Map<number, number>();
    for (const annotation of annotations.annotations) {
      const key = annotation.page.objectNumber;
      byPage.set(key, (byPage.get(key) ?? 0) + 1);
    }
    const pages = [];
    for (const page of listing.pages) {
      const handle = doc.page(page.ref);
      pages.push({
        width: page.size.width,
        height: page.size.height,
        rotation: page.rotation,
        text: (await handle.text.get()).text,
        annotationCount: byPage.get(page.ref.objectNumber) ?? 0,
      });
    }
    return {
      pageCount: listing.pageCount,
      pages,
      title: metadata.title,
      author: metadata.author,
    };
  } finally {
    await doc.close();
  }
}

async function readCase(entry: SaveCase): Promise<unknown> {
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
    const original = await describe(engine, `save:${entry.name}:original`, bytes, entry.password);

    const modes: Record<string, Reopened> = {};
    for (const mode of ['incremental', 'full'] as const) {
      const doc = await engine.open(
        { kind: 'bytes', id: `save:${entry.name}:${mode}`, bytes },
        { scope: ['*'], password: entry.password },
      );
      let saved: Uint8Array;
      try {
        // `PdfSaveMode` spells the full save `'rewrite'`, and did before this
        // catch-up too: `'full'` only ever worked because every mode other
        // than `'incremental'` saves non-incrementally (`pdfSaveModeFlags`).
        // The vector keeps its `full` key.
        saved = await doc.download({ mode: mode === 'full' ? 'rewrite' : mode });
      } finally {
        await doc.close();
      }
      modes[mode] = await describe(
        engine,
        `save:${entry.name}:${mode}:reopened`,
        saved,
        entry.password,
      );
    }

    return { ...entry, original, modes };
  } finally {
    await engine.destroy();
  }
}

function serialize(cases: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'save.roundtrip' },
    null,
    2,
  ).replace(/\n}$/, ',');
  const body = cases.map((entry) => `  ${JSON.stringify(entry)}`).join(',\n');
  return `${head}\n  "cases": [\n${body}\n  ]\n}\n`;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const cases: unknown[] = [];
  for (const entry of CASES) cases.push(await readCase(entry));
  const text = serialize(cases);
  const absolute = join(vectorsRoot, 'roundtrip.json');

  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write('  ok      save/roundtrip.json\n');
    return;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS save/roundtrip.json${current === undefined ? ' (missing)' : ''}\n`,
    );
    process.stderr.write(
      '\nSave vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run save\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write('  written save/roundtrip.json\n');
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-save-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
