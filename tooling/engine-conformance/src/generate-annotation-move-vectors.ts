/**
 * Emits `vectors/annotation/move.json`: what `annotations.move` leaves on the
 * page and what it reports.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-move
 *   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-move:check
 *
 * `crates/epdf-engine/tests/annotation_move.rs` replays it.
 *
 * `annotation_base.pdf` is the page, because a move is about ORDER and that
 * fixture has nine annotations whose identities differ: reordering three
 * identical ones would pass for almost any implementation.
 *
 * What is recorded is the resulting order, by `/NM`, and the envelope. The
 * order is the whole operation; the envelope is what tells a client its
 * index-addressed references are now stale.
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

const FIXTURE = 'tooling/engine-conformance/fixtures/annotation_base.pdf';

interface MoveCase {
  readonly name: string;
  readonly covers: string;
  /** Source indices, in the order the caller wants them to end up. */
  readonly from: readonly number[];
  readonly toIndex: number;
}

const CASES: readonly MoveCase[] = [
  {
    name: 'one-forward',
    covers: 'The single-annotation case, which is a block of one moved later.',
    from: [0],
    toIndex: 4,
  },
  {
    name: 'one-backward',
    covers: 'The same in the other direction, where every index between shifts up rather than down.',
    from: [7],
    toIndex: 1,
  },
  {
    name: 'a-contiguous-block',
    covers: 'Three adjacent annotations moved together, which is what a selection drag is.',
    from: [1, 2, 3],
    toIndex: 5,
  },
  {
    name: 'a-scattered-block',
    covers:
      'Three annotations that were NOT adjacent, gathered into one block. The operation is a detach and an insert, not a rotation, so they end up next to each other.',
    from: [0, 4, 8],
    toIndex: 2,
  },
  {
    name: 'the-callers-order-not-the-documents',
    covers:
      'The same three in reverse. They come out in the order they were asked for, so a reader that sorted the list would produce a different page.',
    from: [8, 4, 0],
    toIndex: 2,
  },
  {
    name: 'to-the-front',
    covers: 'Destination zero, the edge of the post-removal index space.',
    from: [5, 6],
    toIndex: 0,
  },
  {
    name: 'to-the-end',
    covers:
      'Destination equal to the post-removal count, which is the other edge and the one an off-by-one rejects.',
    from: [0, 1],
    toIndex: 7,
  },
  {
    name: 'a-move-that-changes-nothing',
    covers:
      'A block moved to where it already is. Still structural: the revision bumps and the envelope still asks for a refetch, because the engine does not inspect whether the order came out the same.',
    from: [2, 3],
    toIndex: 2,
  },
];

async function readCase(entry: MoveCase): Promise<unknown> {
  const bytes = new Uint8Array(await readFile(join(repoRoot, FIXTURE)));
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
      { kind: 'bytes', id: `move:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      const pages = await doc.pages.list();
      const first = pages.pages[0];
      if (!first) throw new Error(`${entry.name}: the fixture has no pages`);
      const pon = first.ref.pageObjectNumber;
      const before = await doc.annotations.listRaw(first.ref);

      const refs = entry.from.map((index) => ({
        kind: 'index' as const,
        page: first.ref,
        index,
        revision: before.pageState.revision,
      }));

      const page = doc.page(first.ref);
      const result = await page.annotations.move(refs as never, entry.toIndex);
      const after = await doc.annotations.listRaw(first.ref);

      return {
        ...entry,
        pageObjectNumber: pon,
        /** What was there, in order, so the case is readable on its own. */
        beforeOrder: before.annotations.map((a) => a.nm),
        afterOrder: after.annotations.map((a) => a.nm),
        movedOrder: result.moved.map((a) => a.nm),
        movedIndices: result.moved.map((a) => a.index),
        meta: {
          changed: result.meta.changed,
          weakRefsInvalidated: result.meta.weakRefsInvalidated,
          shouldRefetch: result.meta.shouldRefetch,
        },
      };
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
}

/**
 * A `/NM` the engine minted, replaced by a placeholder.
 *
 * A move strengthens every weak annotation in its batch, and the stamp is a
 * fresh UUID v4, so a verbatim recording would differ on every run. What
 * matters is that a name APPEARED where there was none; its value is opaque
 * to everyone including the engine.
 */
const STAMPED = '<stamped>';
const UUID_V4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

function normalizeStampedNames(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(normalizeStampedNames);
  if (typeof value === 'string') return UUID_V4.test(value) ? STAMPED : value;
  if (value === null || typeof value !== 'object') return value;
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>).map(([key, entry]) => [
      key,
      normalizeStampedNames(entry),
    ]),
  );
}

/** See `generate-annotation-read-vectors.ts`. */
function normalizeSessionIds(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(normalizeSessionIds);
  if (value === null || typeof value !== 'object') return value;
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>).map(([key, entry]) =>
      key === 'docSessionId' ? [key, '<session>'] : [key, normalizeSessionIds(entry)],
    ),
  );
}

function serialize(cases: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'annotation.move', fixture: FIXTURE },
    null,
    2,
  ).replace(/\n}$/, ',');
  const body = cases
    .map((entry) => `  ${JSON.stringify(normalizeStampedNames(normalizeSessionIds(entry)))}`)
    .join(',\n');
  return `${head}\n  "cases": [\n${body}\n  ]\n}\n`;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const cases: unknown[] = [];
  for (const entry of CASES) cases.push(await readCase(entry));
  const text = serialize(cases);
  const absolute = join(vectorsRoot, 'move.json');

  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write('  ok      annotation/move.json\n');
    return;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS annotation/move.json${current === undefined ? ' (missing)' : ''}\n`,
    );
    process.stderr.write(
      '\nAnnotation move vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run annotation-move\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write('  written annotation/move.json\n');
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-annotation-move-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
