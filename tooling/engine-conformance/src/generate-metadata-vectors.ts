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
    name: 'rotated',
    fixture: 'tooling/engine-conformance/fixtures/rotated_pages.pdf',
    covers:
      'Four pages at /Rotate 0, 90, 180 and 270. The listing reports the rotation and the NORMALIZED size, so a reader using the rotated size would disagree with the page a caller then opens.',
  },
  {
    name: 'encrypted',
    fixture: 'tooling/engine-conformance/fixtures/encrypted-r4-aes-128.pdf',
    covers: 'Metadata behind encryption, which is read through the same path once unlocked.',
    password: 'user',
  },
];

async function readCase(entry: MetadataCase): Promise<unknown> {
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
    const doc = await engine.open(
      { kind: 'bytes', id: `metadata:${entry.name}`, bytes, password: entry.password },
      { scope: ['*'] },
    );
    try {
      return {
        ...entry,
        metadata: await doc.metadata.read(),
        pages: await doc.pages.list(),
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
    cases: cases.map(({ pages: _pages, ...rest }) => rest),
  };
  const pages = {
    schemaVersion: SCHEMA_VERSION,
    kind: 'pages.list',
    cases: cases.map(({ metadata: _metadata, ...rest }) => rest),
  };
  let differed = await emit('read.json', `${JSON.stringify(metadata, null, 2)}\n`, check);
  differed =
    (await emit('pages.json', `${JSON.stringify(pages, null, 2)}\n`, check)) || differed;
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
