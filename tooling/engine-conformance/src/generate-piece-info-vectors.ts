/**
 * Emits `vectors/files/piece-info.json`: what `/PieceInfo` reads back, on the
 * catalog and on a page.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run piece-info
 *   pnpm --filter @embedpdf/tooling-engine-conformance run piece-info:check
 *
 * G8's `/PieceInfo` gate. `crates/epdf-engine/tests/piece_info_reader.rs`
 * replays it.
 *
 * Both holders are recorded for every case, because the two families of
 * accessors differ only by a page object number and a branch written the
 * wrong way round at one call site reads the catalog's data while claiming
 * the page's.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createLocalEngine } from '../../../packages/engine/main/src/index';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'files');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

interface PieceInfoCase {
  readonly name: string;
  readonly fixture: string;
  readonly covers: string;
  /** Applications to read on each holder, whether or not they are present. */
  readonly applications: readonly string[];
}

const CASES: readonly PieceInfoCase[] = [
  {
    name: 'both-holders',
    fixture: 'tooling/engine-conformance/fixtures/piece_info.pdf',
    covers:
      'One value of every readable shape and several unreadable ones, on the catalog, with DIFFERENT data on the page so a reader that confused the two holders gets the wrong answer rather than the same one.',
    applications: ['com.embedpdf', 'OtherApp', 'NeverWritten'],
  },
  {
    name: 'none',
    fixture: 'tooling/engine-conformance/fixtures/rich_metadata.pdf',
    covers:
      'A document with no /PieceInfo at all. A null read is a different answer from an empty snapshot.',
    applications: ['com.embedpdf'],
  },
];

async function readCase(entry: PieceInfoCase): Promise<unknown> {
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
      { kind: 'bytes', id: `piece-info:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      const pages = await doc.pages.list();
      const first = pages.pages[0];
      if (!first) throw new Error(`${entry.name}: the fixture has no pages`);
      const page = doc.page(first.ref);

      const catalog = doc.pieceInfo;
      const pageInfo = page.pieceInfo;
      if (!catalog || !pageInfo) {
        throw new Error(`${entry.name}: the engine exposes no pieceInfo service`);
      }

      const holders: Record<string, unknown> = {};
      for (const [label, service] of [
        ['catalog', catalog],
        ['page', pageInfo],
      ] as const) {
        const snapshots: Record<string, unknown> = {};
        for (const application of entry.applications) {
          snapshots[application] = await service.read(application);
        }
        holders[label] = { applications: await service.applications(), snapshots };
      }

      return { ...entry, pageObjectNumber: first.ref.pageObjectNumber, holders };
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
}

function serialize(cases: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'files.pieceInfo' },
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
  const absolute = join(vectorsRoot, 'piece-info.json');

  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write('  ok      files/piece-info.json\n');
    return;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS files/piece-info.json${current === undefined ? ' (missing)' : ''}\n`,
    );
    process.stderr.write(
      '\nPiece-info vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run piece-info\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write('  written files/piece-info.json\n');
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-piece-info-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
