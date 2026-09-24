/**
 * Emits `vectors/pages/extract.json`: what `pages.extract` produces, read
 * back SEMANTICALLY.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run extract
 *   pnpm --filter @embedpdf/tooling-engine-conformance run extract:check
 *
 * G9's gate. `crates/epdf-engine/tests/page_extract_corpus.rs` replays it.
 *
 * # Not the bytes
 *
 * `multiplatform-port-order.md` G10 says it outright: the conformance suites
 * do not establish byte-stable PDF serialization. Two runs of one serializer
 * can differ in object numbering and in the file identifier, and comparing
 * the buffers would be comparing PDFium's writer to itself with a coin toss
 * attached.
 *
 * What IS compared is what an extract is for: reopen the result and ask it
 * how many pages it has, how big each one is, which way each one is rotated,
 * and what text is on it. A buffer that answers all four correctly is the
 * document that was asked for, whatever its object numbers say.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createLocalEngine } from '../../../packages/engine/main/src/index';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'pages');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

interface ExtractCase {
  readonly name: string;
  readonly fixture: string;
  readonly covers: string;
  /** Zero-based page indices of the SOURCE, resolved to object numbers. */
  readonly sourcePages: readonly number[];
}

const CASES: readonly ExtractCase[] = [
  {
    name: 'single',
    fixture: 'pdfium-multiplatform-prototype/fixtures/report.pdf',
    covers: 'One page out of fourteen, which is the ordinary case.',
    sourcePages: [4],
  },
  {
    name: 'reordered',
    fixture: 'pdfium-multiplatform-prototype/fixtures/report.pdf',
    covers:
      "The caller's order, not the document's. Pages 3, 0 and 1 come out in that order, so a reader that sorted the selection would produce a different document.",
    sourcePages: [3, 0, 1],
  },
  {
    name: 'rotated',
    fixture: 'tooling/engine-conformance/fixtures/rotated_pages.pdf',
    covers:
      'Four pages at /Rotate 0, 90, 180 and 270. The extracted pages keep their rotation, and their sizes are the NORMALIZED ones, so a reader using the unrotated size disagrees on half of them.',
    sourcePages: [0, 1, 2, 3],
  },
  {
    name: 'whole-document',
    fixture: 'tooling/engine-conformance/fixtures/annotation_kinds.pdf',
    covers:
      'Every page of a small document, which is the copy-everything case, and one carrying annotations so the import has something besides content to bring.',
    sourcePages: [0],
  },
];

interface ExtractedPage {
  readonly width: number;
  readonly height: number;
  readonly rotation: number;
  /** The page's text, which is what proves the right page was copied. */
  readonly text: string;
}

async function readCase(entry: ExtractCase): Promise<unknown> {
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
      { kind: 'bytes', id: `extract:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    let extracted: Uint8Array;
    let sourceObjectNumbers: number[];
    try {
      const listing = await doc.pages.list();
      sourceObjectNumbers = entry.sourcePages.map((index) => {
        const page = listing.pages[index];
        if (!page) throw new Error(`${entry.name}: the fixture has no page ${index}`);
        return page.pageObjectNumber;
      });
      extracted = await doc.pages.extract(sourceObjectNumbers);
    } finally {
      await doc.close();
    }

    // Reopen the result through the same engine and describe it.
    const reopened = await engine.open(
      { kind: 'bytes', id: `extract:${entry.name}:reopened`, bytes: extracted },
      { scope: ['*'] },
    );
    try {
      const listing = await reopened.pages.list();
      const pages: ExtractedPage[] = [];
      for (const page of listing.pages) {
        const handle = reopened.page(page.pageObjectNumber);
        pages.push({
          width: page.size.width,
          height: page.size.height,
          rotation: page.rotation,
          text: (await handle.text.read()).text,
        });
      }
      return { ...entry, sourceObjectNumbers, pageCount: listing.pageCount, pages };
    } finally {
      await reopened.close();
    }
  } finally {
    await engine.destroy();
  }
}

function serialize(cases: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'pages.extract' },
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
  const absolute = join(vectorsRoot, 'extract.json');

  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write('  ok      pages/extract.json\n');
    return;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS pages/extract.json${current === undefined ? ' (missing)' : ''}\n`,
    );
    process.stderr.write(
      '\nExtract vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run extract\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write('  written pages/extract.json\n');
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-extract-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
