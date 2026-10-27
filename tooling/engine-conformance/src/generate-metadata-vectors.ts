/**
 * Emits the document-level reads: `vectors/metadata/read.json` and
 * `vectors/metadata/pages.json`.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run metadata
 *   pnpm --filter @embedpdf/tooling-engine-conformance run metadata:check
 *
 * G6's gate. `crates/epdf-engine/tests/{metadata_corpus,pages_corpus}.rs`
 * replay them.
 *
 * A separate generator from `generate-vectors.ts` because these are
 * document-level and that one is built around per-page vectors. Folding a
 * document-level operation into a per-page envelope would mean emitting the
 * same answer once per page.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createLocalEngine } from '../../../packages/engine/main/src/index';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'metadata');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

interface MetadataCase {
  readonly name: string;
  readonly fixture: string;
  readonly covers: string;
  readonly password?: string;
}

const CASES: readonly MetadataCase[] = [
  {
    name: 'rich',
    fixture: 'tooling/engine-conformance/fixtures/rich_metadata.pdf',
    covers:
      'Every distinction the read has to make: an absent key, a present-but-empty one, a UTF-16 title outside Latin-1, custom keys including an empty one, and dates in both the offset and the Z forms.',
  },
  {
    name: 'report',
    fixture: 'pdfium-multiplatform-prototype/fixtures/report.pdf',
    covers: 'A real document, whose Info dictionary is whatever its producer wrote.',
  },
  {
    name: 'colors',
    fixture: 'pdfium-multiplatform-prototype/fixtures/colors.pdf',
    covers: 'A minimal document with almost no metadata: the mostly-absent case.',
  },
  {
    name: 'document-actions',
    fixture: 'tooling/engine-conformance/fixtures/document_actions.pdf',
    covers:
      'Every catalog action slot: a two-entry JavaScript name tree in boot order, an /OpenAction in its action form, and all five additional events. Nothing else in the repository has any of them.',
  },
  {
    name: 'rotated',
    fixture: 'tooling/engine-conformance/fixtures/rotated_pages.pdf',
    covers:
      'Four pages at /Rotate 0, 90, 180 and 270. The listing reports the rotation and the NORMALIZED size, so a reader using the rotated size would disagree with the page a caller then opens.',
  },
  {
    name: 'page-actions',
    fixture: 'tooling/engine-conformance/fixtures/page_actions.pdf',
    covers:
      'A page carrying open and close actions: a JavaScript node chaining to a URI through /Next, and a GoTo with an explicit /XYZ destination. PageLayout.actions was absent on both sides until this existed, so a comparison could not tell an unread action from a missing one.',
  },
  {
    name: 'encrypted',
    fixture: 'tooling/engine-conformance/fixtures/encrypted-r4-aes-128.pdf',
    covers: 'Metadata behind encryption, which is read through the same path once unlocked.',
    password: 'user',
  },
];

async function readCase(entry: MetadataCase): Promise<Record<string, unknown>> {
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
    // The password moved from the input to `OpenOptions` in 09c24496f
    // ("Engine API v3, round 2: opening and lifecycle").
    const doc = await engine.open(
      { kind: 'bytes', id: `metadata:${entry.name}`, bytes },
      { scope: ['*'], password: entry.password },
    );
    try {
      // `metadata.read` and `actions.read` became `get` in 83f6598d2 ("Engine
      // API v3, round 3a"). The Info dict's other keys left the metadata read
      // for `metadata.custom.get()` in a4b8259f7 ("Custom metadata keys are an
      // object of their own"); they go back under `custom`, where the vector
      // has always carried them.
      return {
        ...entry,
        metadata: { ...(await doc.metadata.get()), custom: await doc.metadata.custom.get() },
        pages: await doc.pages.list(),
        actions: await doc.actions.get(),
      };
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
}

async function emit(relative: string, text: string, check: boolean): Promise<boolean> {
  const absolute = join(vectorsRoot, relative);
  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write(`  ok      metadata/${relative}\n`);
    return false;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS metadata/${relative}${current === undefined ? ' (missing)' : ''}\n`,
    );
    return true;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write(`  written metadata/${relative}\n`);
  return true;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');

  const cases: Record<string, unknown>[] = [];
  for (const entry of CASES) {
    cases.push(await readCase(entry));
  }

  const metadata = {
    schemaVersion: SCHEMA_VERSION,
    kind: 'metadata.read',
    cases: cases.map(({ pages: _pages, actions: _actions, ...rest }) => rest),
  };
  const pages = {
    schemaVersion: SCHEMA_VERSION,
    kind: 'pages.list',
    cases: cases.map(({ metadata: _metadata, actions: _actions, ...rest }) => rest),
  };
  const actions = {
    schemaVersion: SCHEMA_VERSION,
    kind: 'actions.read',
    cases: cases.map(({ metadata: _metadata, pages: _pages, ...rest }) => rest),
  };
  let differed = await emit('read.json', `${JSON.stringify(metadata, null, 2)}\n`, check);
  differed =
    (await emit('pages.json', `${JSON.stringify(pages, null, 2)}\n`, check)) || differed;
  differed =
    (await emit('actions.json', `${JSON.stringify(actions, null, 2)}\n`, check)) || differed;
  if (check && differed) {
    process.stderr.write(
      '\nMetadata vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run metadata\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
  }
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-metadata-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
