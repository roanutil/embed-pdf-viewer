/**
 * Emits `vectors/pages/mutate.json`: what `pages.move`, `pages.rotate` and
 * `pages.delete` leave in the document.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run pages-mutate
 *   pnpm --filter @embedpdf/tooling-engine-conformance run pages-mutate:check
 *
 * G13's first gate. `crates/epdf-engine/tests/pages_mutate.rs` replays it.
 *
 * # The layout is the whole answer
 *
 * A reorder changes every page's index, a rotation changes what a page
 * measures, and a delete changes both for everything after it. So each
 * operation reports the whole list, and what the comparison watches is
 * which OBJECT NUMBER ends up at which index: an index is a position and
 * only the object number is identity.
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

/** Four pages, object numbers 3 through 6, rotations 0/90/180/270. */
const FIXTURE = 'tooling/engine-conformance/fixtures/rotated_pages.pdf';

interface MutateCase {
  readonly name: string;
  readonly covers: string;
  readonly move?: { readonly pages: number[]; readonly destIndex: number };
  readonly rotate?: { readonly pages: number[]; readonly rotation: number };
  readonly delete?: { readonly pages: number[] };
  /** Create blank pages. */
  readonly insertBlank?: {
    readonly size: { readonly width: number; readonly height: number };
    readonly count?: number;
    readonly destIndex?: number;
  };
  /** Insert every page of another PDF, named by fixture path. */
  readonly insert?: { readonly source: string; readonly destIndex?: number };
  /** Flatten pages, which runs against its own fixture. */
  readonly flatten?: {
    readonly fixture: string;
    readonly pages: number[];
    readonly usage: 'display' | 'print';
  };
  readonly refused?: true;
}

const CASES: readonly MutateCase[] = [
  {
    name: 'move-one-forward',
    covers:
      'One page moved to the end. The destination is a position in the POST-REMOVAL index space, which is the only frame where the answer is unambiguous.',
    move: { pages: [3], destIndex: 3 },
  },
  {
    name: 'move-one-backward',
    covers: 'And the other way, to the front.',
    move: { pages: [6], destIndex: 0 },
  },
  {
    name: 'move-block-preserves-caller-order',
    covers:
      'Two pages named in an order that is not their document order. They land as one contiguous block in the order GIVEN, not in the order they were in.',
    move: { pages: [6, 4], destIndex: 0 },
  },
  {
    name: 'move-to-same-place',
    covers: 'A move that changes nothing, which is a move rather than a refusal.',
    move: { pages: [3], destIndex: 0 },
  },
  {
    name: 'move-past-the-end-refused',
    covers:
      'A destination past the end of the post-removal list. Three pages remain when one is detached, so index 4 names nowhere.',
    move: { pages: [3], destIndex: 4 },
    refused: true,
  },
  {
    name: 'move-duplicate-refused',
    covers:
      'The same page twice. Deduplicating would silently change what the caller asked for, and the destination would be ambiguous.',
    move: { pages: [3, 3], destIndex: 1 },
    refused: true,
  },
  {
    name: 'move-nothing-refused',
    covers: 'An empty list, which names no pages to move and no move to make.',
    move: { pages: [], destIndex: 0 },
    refused: true,
  },
  {
    name: 'rotate-nothing-refused',
    covers: 'The same on the other two operations.',
    rotate: { pages: [], rotation: 90 },
    refused: true,
  },
  {
    name: 'delete-nothing-refused',
    covers: 'And on the delete.',
    delete: { pages: [] },
    refused: true,
  },
  {
    name: 'move-unknown-page-refused',
    covers: 'An object number no page has.',
    move: { pages: [99], destIndex: 0 },
    refused: true,
  },
  {
    name: 'rotate-one',
    covers: 'A page rotated to an angle it did not have.',
    rotate: { pages: [3], rotation: 180 },
  },
  {
    name: 'rotate-to-zero',
    covers:
      'Back to upright, which is a rotation rather than a removal: /Rotate 0 is a value the page carries.',
    rotate: { pages: [4], rotation: 0 },
  },
  {
    name: 'rotate-several',
    covers: 'Several pages to one angle, which flattens four different rotations into one.',
    rotate: { pages: [3, 4, 5, 6], rotation: 90 },
  },
  {
    name: 'rotate-unknown-page-refused',
    covers:
      'A list naming one page that does not exist. Nothing is rotated: the whole list resolves before anything is written.',
    rotate: { pages: [3, 99], rotation: 90 },
    refused: true,
  },
  {
    name: 'insert-blank-at-end',
    covers:
      'A blank page appended, which is what an absent destination means. Its object number is fresh and is resolved from the list AFTER the insert: it does not exist before.',
    insertBlank: { size: { width: 612, height: 792 } },
  },
  {
    name: 'insert-blank-at-front',
    covers: 'Three blank pages at the front, which pushes every existing page down by three.',
    insertBlank: { size: { width: 200, height: 400 }, count: 3, destIndex: 0 },
  },
  {
    name: 'insert-blank-in-the-middle',
    covers: 'And in the middle, where the new pages land contiguously.',
    insertBlank: { size: { width: 300, height: 300 }, count: 2, destIndex: 2 },
  },
  {
    name: 'insert-blank-bad-size-refused',
    covers: 'A page with no area is not a page.',
    insertBlank: { size: { width: 0, height: 792 } },
    refused: true,
  },
  {
    name: 'insert-blank-too-many-refused',
    covers:
      'Past the batch ceiling. A larger batch is a loop at the call site; the number is shared with the server so a client never finds a lower one by surprise.',
    insertBlank: { size: { width: 612, height: 792 }, count: 101 },
    refused: true,
  },
  {
    name: 'insert-blank-past-the-end-refused',
    covers:
      'A destination past the end. PDFium would CLAMP this to an append, and the guard is what keeps that leniency out of the contract.',
    insertBlank: { size: { width: 612, height: 792 }, destIndex: 5 },
    refused: true,
  },
  {
    name: 'insert-document',
    covers:
      'Every page of another PDF, deep-copied in. The copy is not a full detach: an imported stream still reads through the source, so the source has to outlive every future save.',
    insert: { source: 'tooling/engine-conformance/fixtures/rich_metadata.pdf', destIndex: 1 },
  },
  {
    name: 'insert-document-at-end',
    covers: 'The same with no destination, which appends.',
    insert: { source: 'tooling/engine-conformance/fixtures/rich_metadata.pdf' },
  },
  {
    name: 'flatten-annotated-page',
    covers:
      'A page with annotations turned into content. Irreversible, and it is the page that changes rather than the page list.',
    flatten: {
      fixture: 'tooling/engine-conformance/fixtures/annotation_appearances.pdf',
      pages: [3],
      usage: 'display',
    },
  },
  {
    name: 'flatten-for-print',
    covers:
      'The other usage. An annotation may carry a different stream for each, and a stamp hidden on screen that prints is what makes the distinction matter.',
    flatten: {
      fixture: 'tooling/engine-conformance/fixtures/annotation_appearances.pdf',
      pages: [3],
      usage: 'print',
    },
  },
  {
    name: 'flatten-page-with-nothing-on-it',
    covers:
      'A page with no annotations, which is UNCHANGED rather than failed: a page with nothing to flatten is already flat.',
    flatten: {
      fixture: 'tooling/engine-conformance/fixtures/rich_metadata.pdf',
      pages: [3],
      usage: 'display',
    },
  },
  {
    name: 'flatten-duplicate-refused',
    covers: 'The same page twice. The second would flatten what the first already did.',
    flatten: {
      fixture: 'tooling/engine-conformance/fixtures/annotation_appearances.pdf',
      pages: [3, 3],
      usage: 'display',
    },
    refused: true,
  },
  {
    name: 'delete-one',
    covers:
      'A page removed. Its object number is RETIRED rather than freed, so per-page state a caller kept can never attach to an unrelated future page.',
    delete: { pages: [4] },
  },
  {
    name: 'delete-several',
    covers: 'Several at once, which leaves the survivors in their original relative order.',
    delete: { pages: [3, 5] },
  },
  {
    name: 'delete-every-page-refused',
    covers: 'A document with no pages is not a document.',
    delete: { pages: [3, 4, 5, 6] },
    refused: true,
  },
  {
    name: 'delete-unknown-page-refused',
    covers: 'An object number no page has. Nothing is deleted.',
    delete: { pages: [3, 99] },
    refused: true,
  },
];

async function readCase(entry: MutateCase): Promise<unknown> {
  // A flatten needs a page with annotations on it, which the page-order
  // fixture has none of, so those cases name their own.
  const bytes = new Uint8Array(
    await readFile(join(repoRoot, entry.flatten?.fixture ?? FIXTURE)),
  );
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
      { kind: 'bytes', id: `pages-mutate:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      const run = async () => {
        if (entry.move) {
          return (await doc.pages.move(entry.move.pages as never, entry.move.destIndex)).layout;
        }
        if (entry.rotate) {
          return (
            await doc.pages.rotate(entry.rotate.pages as never, entry.rotate.rotation as never)
          ).layout;
        }
        if (entry.insertBlank) {
          const result = await doc.pages.insertBlank(
            { size: entry.insertBlank.size, count: entry.insertBlank.count } as never,
            entry.insertBlank.destIndex,
          );
          return { layout: result.layout, inserted: result.insertedPageObjectNumbers };
        }
        if (entry.flatten) {
          const result = await doc.pages.flatten(
            entry.flatten.pages as never,
            entry.flatten.usage,
          );
          // The whole result, not just the per-page outcomes: it echoes
          // the request back, and an engine that lost the echo would still
          // agree about what happened.
          const { meta: _meta, ...flattened } = result as Record<string, unknown>;
          return { flattened };
        }
        if (entry.insert) {
          const source = new Uint8Array(await readFile(join(repoRoot, entry.insert.source)));
          const result = await doc.pages.insert(
            source.buffer as ArrayBuffer,
            entry.insert.destIndex,
          );
          return { layout: result.layout, inserted: result.insertedPageObjectNumbers };
        }
        return (await doc.pages.delete(entry.delete!.pages as never)).layout;
      };

      if (entry.refused) {
        let code: string | null = null;
        try {
          await run();
        } catch (error) {
          code = String((error as { code?: unknown })?.code ?? 'unknown');
        }
        if (code === null) throw new Error(`${entry.name}: expected a refusal`);
        return { ...entry, refusedWith: code };
      }
      const produced = (await run()) as Record<string, unknown>;
      // The insert operations report which pages are new alongside the
      // layout; the rest report the layout alone.
      if ('flattened' in produced) return { ...entry, flattened: produced.flattened };
      return 'layout' in produced
        ? { ...entry, layout: produced.layout, inserted: produced.inserted }
        : { ...entry, layout: produced };
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
}

function serialize(cases: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'pages.mutate', fixture: FIXTURE },
    null,
    2,
  ).replace(/\n}$/, ',');
  const body = cases.map((entry) => `  ${JSON.stringify(entry)}`).join(',\n');
  return `${head}\n  "cases": [\n${body}\n  ]\n}\n`;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const cases: unknown[] = [];
  for (const entry of CASES) {
    try {
      cases.push(await readCase(entry));
    } catch (error) {
      process.stderr.write(`  case ${entry.name}: ${String((error as Error)?.message ?? error)}\n`);
      throw error;
    }
  }
  const text = serialize(cases);
  const absolute = join(vectorsRoot, 'mutate.json');
  if (check) {
    const current = await readFile(absolute, 'utf8').catch(() => '');
    if (current !== text) {
      process.stderr.write('  STALE   pages/mutate.json\n');
      process.exitCode = 1;
      return;
    }
    process.stdout.write('  ok      pages/mutate.json\n');
    return;
  }
  await mkdir(vectorsRoot, { recursive: true });
  await writeFile(absolute, text, 'utf8');
  process.stdout.write('  written pages/mutate.json\n');
}

main().catch((error) => {
  process.stderr.write(`generate-pages-mutate-vectors: ${String(error?.stack ?? error)}\n`);
  process.exitCode = 1;
});
