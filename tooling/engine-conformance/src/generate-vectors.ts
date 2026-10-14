/**
 * Emits the frozen conformance vectors under `vectors/`, reading every corpus
 * case through the shipping TypeScript engine.
 *
 * From the repository root:
 *   pnpm --filter @embedpdf/tooling-engine-conformance run vectors
 *   pnpm --filter @embedpdf/tooling-engine-conformance run vectors:check
 *
 * `--check` regenerates in memory and exits non-zero on any difference, which
 * is the CI gate: a vector may only change when someone regenerates it
 * deliberately and commits the diff.
 *
 * The TypeScript engine is the oracle. The Rust runner in
 * `crates/epdf-engine/tests/geometry_corpus.rs` reads the same files and fails
 * on divergence, so neither implementation can move alone.
 */
import { mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createLocalEngine } from '../../../packages/engine/main/src/index';

import { CORPUS, type CorpusCase, type CorpusOperation } from './corpus';
import { quantizeDeep } from './quantize';
import { pageRef } from './wire-page-refs';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors');

/** Bump when the envelope below changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

interface PageVector {
  readonly index: number;
  readonly pageObjectNumber: number;
  readonly snapshot: unknown;
}

interface VectorFile {
  readonly schemaVersion: number;
  readonly operation: CorpusOperation;
  readonly case: string;
  readonly fixture: string;
  readonly covers: string;
  readonly pages: readonly PageVector[];
}

/**
 * Rejects non-finite numbers rather than letting `JSON.stringify` turn them
 * into `null`, which would freeze a silent hole into the corpus.
 */
function assertFinite(path: string, value: unknown): void {
  if (typeof value === 'number' && !Number.isFinite(value)) {
    throw new Error(`Non-finite value at ${path}: ${value}`);
  }
  if (Array.isArray(value)) {
    value.forEach((item, i) => assertFinite(`${path}[${i}]`, item));
    return;
  }
  if (value && typeof value === 'object') {
    for (const [key, child] of Object.entries(value)) assertFinite(`${path}.${key}`, child);
  }
}

/**
 * One line of JSON per page inside an otherwise readable envelope.
 *
 * Indenting a 5,000-glyph snapshot produces a megabyte of whitespace and a diff
 * nobody reads; putting the whole file on one line produces a diff nobody can
 * localize. A page is the unit a reviewer actually reasons about, so it is the
 * unit of a line.
 */
function serialize(file: VectorFile): string {
  assertFinite(file.case, file);
  const header = {
    schemaVersion: file.schemaVersion,
    operation: file.operation,
    case: file.case,
    fixture: file.fixture,
    covers: file.covers,
  };
  const head = JSON.stringify(header, null, 2).replace(/\n}$/, ',');
  const pages = file.pages.map((page) => `  ${JSON.stringify(page)}`).join(',\n');
  return `${head}\n  "pages": [\n${pages}\n  ]\n}\n`;
}

async function readCase(entry: CorpusCase): Promise<Map<CorpusOperation, VectorFile>> {
  const bytes = new Uint8Array(await readFile(join(repoRoot, entry.fixture)));
  const engine = createLocalEngine({
    runtime: {
      prefer: 'wasm',
      // Reserve stdout for progress; PDFium's own diagnostics go to stderr.
      wasm: {
        print: (message: string) => process.stderr.write(`${message}\n`),
        printErr: (message: string) => process.stderr.write(`${message}\n`),
      },
    },
  });

  const files = new Map<CorpusOperation, VectorFile>();
  try {
    const doc = await engine.open(
      { kind: 'bytes', id: `conformance:${entry.name}`, bytes, password: entry.password },
      { scope: ['*'] },
    );
    try {
      const { pages } = await doc.pages.list();
      const wanted =
        entry.pages === 'all'
          ? pages
          : entry.pages.map((index) => {
              const page = pages.find((candidate) => candidate.index === index);
              if (!page) throw new Error(`${entry.name}: no page at index ${index}`);
              return page;
            });

      for (const operation of entry.operations) {
        const emitted: PageVector[] = [];
        for (const page of wanted) {
          emitted.push({
            index: page.index,
            pageObjectNumber: page.ref.pageObjectNumber,
            snapshot: quantizeDeep(await readOperation(doc, operation, page.ref.pageObjectNumber)),
          });
        }
        files.set(operation, {
          schemaVersion: SCHEMA_VERSION,
          operation,
          case: entry.name,
          fixture: entry.fixture,
          covers: entry.covers,
          pages: emitted,
        });
      }
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
  return files;
}

async function readOperation(
  doc: Awaited<ReturnType<ReturnType<typeof createLocalEngine>['open']>>,
  operation: CorpusOperation,
  pageObjectNumber: number,
): Promise<unknown> {
  switch (operation) {
    case 'pages.geometry':
      return await doc.page(pageRef(pageObjectNumber)).geometry.read();
    case 'pages.text':
      return await doc.page(pageRef(pageObjectNumber)).text.read();
    default: {
      // Exhaustiveness: adding an operation to CorpusOperation must not
      // silently emit nothing.
      const unreachable: never = operation;
      throw new Error(`Unhandled corpus operation: ${String(unreachable)}`);
    }
  }
}

/**
 * Every vector file this generator owns, so a case removed from the corpus
 * takes its file with it.
 *
 * Scoped to the operation directories the corpus declares rather than to
 * everything under `vectors/`: `vectors/schema/` belongs to
 * `generate-schema-vectors.ts`, and a generator that deleted another
 * generator's output would be a sharp edge with no upside.
 */
async function existingVectorPaths(): Promise<Set<string>> {
  const found = new Set<string>();
  const operations = new Set(CORPUS.flatMap((entry) => entry.operations));
  for (const operation of operations) {
    let files: string[];
    try {
      files = await readdir(join(vectorsRoot, operation));
    } catch {
      continue;
    }
    for (const file of files) {
      if (file.endsWith('.json')) found.add(join(operation, file));
    }
  }
  return found;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const stale = await existingVectorPaths();
  const differences: string[] = [];

  for (const entry of CORPUS) {
    const files = await readCase(entry);
    for (const [operation, file] of files) {
      const relative = join(operation, `${entry.name}.json`);
      stale.delete(relative);
      const absolute = join(vectorsRoot, relative);
      const next = serialize(file);
      let current: string | undefined;
      try {
        current = await readFile(absolute, 'utf8');
      } catch {
        current = undefined;
      }
      if (current === next) {
        process.stdout.write(`  ok      ${relative}\n`);
        continue;
      }
      differences.push(relative);
      if (check) {
        process.stdout.write(`  DIFFERS ${relative}${current === undefined ? ' (missing)' : ''}\n`);
        continue;
      }
      await mkdir(dirname(absolute), { recursive: true });
      await writeFile(absolute, next);
      process.stdout.write(`  written ${relative}\n`);
    }
  }

  for (const orphan of stale) {
    differences.push(orphan);
    if (check) {
      process.stdout.write(`  ORPHAN  ${orphan}\n`);
      continue;
    }
    await rm(join(vectorsRoot, orphan));
    process.stdout.write(`  removed ${orphan}\n`);
  }

  if (check && differences.length > 0) {
    process.stderr.write(
      `\n${differences.length} vector(s) differ from the corpus. Regenerate with:\n` +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run vectors\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
  }
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
