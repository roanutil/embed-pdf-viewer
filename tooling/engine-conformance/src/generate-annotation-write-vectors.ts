/**
 * Emits `vectors/annotation/write.json`: the pure appearance decision G11's
 * mutators rest on.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-write
 *   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-write:check
 *
 * `crates/epdf-engine/tests/annotation_write.rs` replays it.
 *
 * It is pure and silent when wrong, which is why it is pinned before the
 * mutators that use it. A wrong appearance verdict either destroys an
 * appearance stream a foreign producer wrote, which cannot be recomputed, or
 * keeps a stale one that no longer matches the dictionary. There used to be
 * a second decision, the mutation impact; it went upstream with weak refs
 * (see below).
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { appearanceImpactOf } from '../../../packages/engine/core/src/annotation/appearance';
import { pdfResolveRectCommand } from '../../../packages/engine/core/src/annotation/resolve/rectCommand';
// `computeMutationImpact` and `revision/PageState` were deleted upstream in
// bc0057f3a ("Remove NM and implement BaseID"). Annotation names no longer
// change, so there are no weak index refs, no `weakRefsInvalidated` and no
// `shouldRefetch`: a mutation's meta is now `annotationMutationMeta`, which
// only lists the page and the refs it touched. The `mutations` section's
// subject is gone; its cases stay below, commented out.
// import { computeMutationImpact } from '../../../packages/engine/services/src/features/annotations/internal/mutations/computeMutationImpact';
// import type { PageState } from '../../../packages/engine/core/src/revision/PageState';

import { IMPACT_CASES, MUTATION_CASES } from './annotation-write-cases';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const vectorsRoot = join(packageRoot, 'vectors', 'annotation');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

/*
 * Commented out with `computeMutationImpact` (bc0057f3a): every case in
 * MUTATION_CASES asks how a mutation on a page with weak (index-addressed)
 * annotations invalidates refs, and upstream removed weak refs entirely.
 */
/** The page both sides of a mutation case describe. Still in the header. */
const PAGE_OBJECT_NUMBER = 3;
// const SESSION = '<session>';
//
// function pageState(
//   generation: number,
//   weak: MutationCase['weakBefore'],
// ): PageState {
//   return {
//     pageObjectNumber: PAGE_OBJECT_NUMBER as PageState['pageObjectNumber'],
//     revision: {
//       docSessionId: SESSION,
//       pageObjectNumber: PAGE_OBJECT_NUMBER as PageState['pageObjectNumber'],
//       generation,
//     },
//     weakAnnotationState: weak as PageState['weakAnnotationState'],
//   };
// }
//
// function runMutationCase(entry: MutationCase): unknown {
//   try {
//     return {
//       ...entry,
//       meta: computeMutationImpact({
//         mutation: entry.mutation,
//         pageStateBefore: pageState(entry.generationBefore, entry.weakBefore),
//         pageStateAfter: pageState(entry.generationAfter, entry.weakAfter),
//         changed: entry.changed as never,
//       }),
//     };
//   } catch (error) {
//     // The TypeScript THROWS for an index-shifting mutation with an unknown
//     // weak state. Recorded rather than left to crash the generator, so the
//     // Rust side can pin that the same input is refused.
//     return { ...entry, threw: error instanceof Error ? error.message : String(error) };
//   }
// }

function serialize(): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'annotation.write', pageObjectNumber: PAGE_OBJECT_NUMBER },
    null,
    2,
  ).replace(/\n}$/, ',');
  const section = (label: string, entries: readonly unknown[]): string =>
    `  ${JSON.stringify(label)}: [\n${entries.map((e) => `    ${JSON.stringify(e)}`).join(',\n')}\n  ]`;
  // The update resolves a drawn kind's patch `rect` into its shape before it
  // classifies (`pdfResolveRectCommand`, 9d8ebf857); this does the same, and
  // records the resolved patch where that changed it.
  const impacts = IMPACT_CASES.map((entry) => {
    const resolved = pdfResolveRectCommand(entry.current as never, entry.patch as never);
    const changed = JSON.stringify(resolved) !== JSON.stringify(entry.patch);
    return {
      ...entry,
      ...(changed ? { resolvedPatch: resolved } : {}),
      impact: appearanceImpactOf(entry.current as never, resolved),
    };
  });
  return `${head}\n${[
    section('appearance', impacts),
    // section('mutations', MUTATION_CASES.map(runMutationCase)),
  ].join(',\n')}\n}\n`;
}

async function main(): Promise<void> {
  const check = process.argv.includes('--check');
  const names = new Set<string>();
  for (const entry of [...IMPACT_CASES, ...MUTATION_CASES]) {
    if (names.has(entry.name)) throw new Error(`duplicate case name ${entry.name}`);
    names.add(entry.name);
  }

  const text = serialize();
  const absolute = join(vectorsRoot, 'write.json');
  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write('  ok      annotation/write.json\n');
    return;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS annotation/write.json${current === undefined ? ' (missing)' : ''}\n`,
    );
    process.stderr.write(
      '\nAnnotation write vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run annotation-write\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write('  written annotation/write.json\n');
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-annotation-write-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
