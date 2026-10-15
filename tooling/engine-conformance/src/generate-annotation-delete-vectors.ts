/**
 * Emits `vectors/annotation/delete.json`: what `annotations.delete` reports
 * and what the page looks like afterwards.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-delete
 *   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-delete:check
 *
 * `crates/epdf-engine/tests/annotation_delete.rs` replays it.
 *
 * Every case deletes from `annotation_base.pdf`, which is the fixture with
 * one annotation of every identity shape: an indirect one, a direct one with
 * `/NM`, a direct one without, and replies pointing at the first. So the
 * three reference kinds are exercised against annotations that really differ
 * rather than against three spellings of one.
 *
 * What is recorded is the reported id, the envelope, and the /NM of every
 * annotation left on the page IN ORDER. The last one is what catches a
 * delete that removed the wrong entry: every other field would look right.
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

/** How the case addresses the annotation it deletes. */
type Target =
  | { kind: 'objectNumber'; nm: string }
  | { kind: 'nm'; nm: string }
  | { kind: 'index'; index: number };

interface DeleteCase {
  readonly name: string;
  readonly covers: string;
  readonly target: Target;
}

const CASES: readonly DeleteCase[] = [
  {
    name: 'by-object-number',
    covers:
      'The ordinary case: an indirect annotation addressed by its object number. Two replies point at it, so this also shows what a delete does NOT clean up.',
    target: { kind: 'objectNumber', nm: 'the-anchor' },
  },
  {
    name: 'by-nm',
    covers:
      'A direct annotation with an /NM, which has no object number at all. Its only address is its name.',
    target: { kind: 'nm', nm: 'direct-but-named' },
  },
  {
    name: 'by-index-onto-a-durable-annotation',
    covers:
      'An index reference that lands on an annotation with an object number. The reported id is the one it turned out to have, not the one it was asked for.',
    target: { kind: 'index', index: 0 },
  },
  {
    name: 'by-index-onto-a-weak-annotation',
    covers:
      'An index reference onto the annotation with no object number and no /NM. Nothing durable to report, so the result is null and the caller learns it must re-read.',
    target: { kind: 'index', index: 3 },
  },
  {
    name: 'by-index-onto-an-empty-nm',
    covers:
      'The annotation whose /NM is present and empty. An empty name is not an identity, so this is null too rather than a name two annotations could share.',
    target: { kind: 'index', index: 4 },
  },
  {
    name: 'the-last-annotation',
    covers:
      'Deleting the final entry, which shifts nothing. The envelope still reports a refetch, because the page had weak annotations and an old snapshot can hold an index from before they were strengthened.',
    target: { kind: 'index', index: 8 },
  },
];

async function readCase(entry: DeleteCase): Promise<unknown> {
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
      { kind: 'bytes', id: `delete:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      const listing = await doc.pages.list();
      const first = listing.pages[0];
      if (!first) throw new Error(`${entry.name}: the fixture has no pages`);
      const pon = first.ref.pageObjectNumber;
      const before = await doc.annotations.listRaw(first.ref);

      // Resolve the case's description of its target into a real ref.
      let ref;
      if (entry.target.kind === 'index') {
        ref = { kind: 'index' as const, page: first.ref, index: entry.target.index,
          revision: before.pageState.revision };
      } else {
        const found = before.annotations.find((a) => a.nm === (entry.target as { nm: string }).nm);
        if (!found) throw new Error(`${entry.name}: no annotation named ${JSON.stringify(entry.target)}`);
        ref = found.ref;
        if (ref.kind !== entry.target.kind) {
          throw new Error(
            `${entry.name}: expected a ${entry.target.kind} ref and the annotation has a ${ref.kind} one`,
          );
        }
      }

      const page = doc.page(first.ref);
      const result = await page.annotations.delete(ref as never);
      const after = await doc.annotations.listRaw(first.ref);

      return {
        ...entry,
        pageObjectNumber: pon,
        ref,
        deleted: result.deleted,
        meta: {
          changed: result.meta.changed,
          weakRefsInvalidated: result.meta.weakRefsInvalidated,
          shouldRefetch: result.meta.shouldRefetch,
        },
        /** What is left, in order. `null` for an annotation with no /NM. */
        remaining: after.annotations.map((a) => a.nm),
      };
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
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
    { schemaVersion: SCHEMA_VERSION, kind: 'annotation.delete', fixture: FIXTURE },
    null,
    2,
  ).replace(/\n}$/, ',');
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
  const absolute = join(vectorsRoot, 'delete.json');

  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write('  ok      annotation/delete.json\n');
    return;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS annotation/delete.json${current === undefined ? ' (missing)' : ''}\n`,
    );
    process.stderr.write(
      '\nAnnotation delete vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run annotation-delete\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write('  written annotation/delete.json\n');
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-annotation-delete-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
