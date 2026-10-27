/**
 * Emits `vectors/redaction/apply.json`: what `redaction.apply` removes.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run redaction
 *   pnpm --filter @embedpdf/tooling-engine-conformance run redaction:check
 *
 * G13's destructive gate. `crates/epdf-engine/tests/redaction_apply.rs`
 * replays it.
 *
 * # What is compared, and what cannot be
 *
 * The per-page status and the collateral count, plus the annotations left on
 * the page afterwards. What is NOT compared is the page content: the two
 * engines call the same sanitizer, so comparing its output would compare it
 * to itself, and the bytes it writes are not part of this port's contract.
 */
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  type createLocalEngine,
  toPageRef,
  type Annotation,
  type AnnotationRef,
} from '../../../packages/engine/main/src/index';
import type { RedactionApplyScope } from '../../../packages/engine/core/src/mutation/RedactionApplyResult';

import { createRecordingEngine } from './recording-engine';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'redaction');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

/** Three redaction markings on one page, alongside every other kind. */
const FIXTURE = 'tooling/engine-conformance/fixtures/annotation_kinds.pdf';
const PAGE = 3;

interface RedactionCase {
  readonly name: string;
  readonly covers: string;
  readonly scope: Record<string, unknown>;
  readonly refused?: true;
}

const CASES: readonly RedactionCase[] = [
  {
    name: 'whole-page',
    covers:
      'Every marking on a page. The collateral count is what went with them, not how many markings there were.',
    scope: { kind: 'pages', pageObjectNumbers: [PAGE] },
  },
  {
    name: 'one-marking-by-name',
    covers:
      'One marking, addressed durably. The other two survive, which is what separates this from the page scope.',
    scope: {
      kind: 'annotations',
      refs: [{ kind: 'nm', pageObjectNumber: PAGE, nm: 'a-text-redaction' }],
    },
  },
  {
    name: 'two-markings-by-name',
    covers:
      'Two in one call. Applying one shifts every index after it, and a durable reference survives that; a positional one would not.',
    scope: {
      kind: 'annotations',
      refs: [
        { kind: 'nm', pageObjectNumber: PAGE, nm: 'a-text-redaction' },
        { kind: 'nm', pageObjectNumber: PAGE, nm: 'an-area-redaction' },
      ],
    },
  },
  {
    name: 'page-with-no-markings',
    covers:
      'A page with nothing to redact, which is UNCHANGED rather than failed. PDFium answers a page with no markings the same way it answers a failure, so the engine looks first.',
    scope: { kind: 'pages', pageObjectNumbers: [PAGE] },
  },
  {
    name: 'no-pages-refused',
    covers: 'An empty scope names nothing to redact.',
    scope: { kind: 'pages', pageObjectNumbers: [] },
    refused: true,
  },
  {
    name: 'no-refs-refused',
    covers: 'And the same on the other scope.',
    scope: { kind: 'annotations', refs: [] },
    refused: true,
  },
  {
    name: 'duplicate-page-refused',
    covers: 'The same page twice. The second would redact what the first already did.',
    scope: { kind: 'pages', pageObjectNumbers: [PAGE, PAGE] },
    refused: true,
  },
  {
    name: 'non-redact-ref-refused',
    covers:
      'A reference to an annotation that is not a marking. The WHOLE call is refused, not just that page: a caller that named a highlight has confused two annotations.',
    scope: {
      kind: 'annotations',
      refs: [{ kind: 'nm', pageObjectNumber: PAGE, nm: 'an-approved-stamp' }],
    },
    refused: true,
  },
  // Index refs went in bc0057f3a ("Remove NM and implement BaseID"): every
  // annotation's ref is its name for life, so no ref shifts when another
  // is applied. This case was about refusing two positional refs, so it
  // stays here, commented out.
  // {
  //   name: 'batched-index-refs-refused',
  //   covers:
  //     'Two positional references on one page. Applying the first shifts the second, so by the time it resolves it names something else.',
  //   scope: {
  //     kind: 'annotations',
  //     refs: [
  //       { kind: 'index', pageObjectNumber: PAGE, index: 0, revision: { docSessionId: 'x', pageObjectNumber: PAGE, generation: 0 } },
  //       { kind: 'index', pageObjectNumber: PAGE, index: 1, revision: { docSessionId: 'x', pageObjectNumber: PAGE, generation: 0 } },
  //     ],
  //   },
  //   refused: true,
  // },
  {
    name: 'unknown-page-refused',
    covers: 'A page no document has.',
    scope: { kind: 'pages', pageObjectNumbers: [999] },
    refused: true,
  },
];

/** The fixture with its three markings stripped, for the unchanged case. */
function needsStrippedFixture(entry: RedactionCase): boolean {
  return entry.name === 'page-with-no-markings';
}

type OpenedDocument = Awaited<ReturnType<ReturnType<typeof createLocalEngine>['open']>>;

/**
 * A case's scope as `redaction.apply` takes it. The cases keep the old
 * spelling and are translated here. The scope lost its `kind` and names
 * pages and annotations by ref (`{ pages }` or `{ annotations }`) since
 * a9434c0e2 ("Documents: v3 result shapes, attachments, redaction,
 * security"). The `nm` ref went in bc0057f3a ("Remove NM and implement
 * BaseID"): a case that names a marking by its /NM gets that annotation's
 * own ref from the page's listing, which is the durable address it meant.
 */
async function v3Scope(
  doc: OpenedDocument,
  scope: Record<string, unknown>,
): Promise<RedactionApplyScope> {
  if (scope.kind === 'pages') {
    return { pages: (scope.pageObjectNumbers as number[]).map(toPageRef) };
  }
  const refs = scope.refs as { pageObjectNumber: number; nm: string }[];
  const listed = new Map<number, Annotation[]>();
  const annotations: AnnotationRef[] = [];
  for (const ref of refs) {
    let onPage = listed.get(ref.pageObjectNumber);
    if (!onPage) {
      onPage = (await doc.page(toPageRef(ref.pageObjectNumber)).annotations.list()).annotations;
      listed.set(ref.pageObjectNumber, onPage);
    }
    const found = onPage.find((annotation) => annotation.nm === ref.nm);
    if (!found) throw new Error(`no annotation named ${ref.nm} on page ${ref.pageObjectNumber}`);
    annotations.push(found.ref);
  }
  return { annotations };
}

async function readCase(entry: RedactionCase): Promise<unknown> {
  const bytes = new Uint8Array(await readFile(join(repoRoot, FIXTURE)));
  const { engine, capture } = createRecordingEngine({
    prefer: 'wasm',
    wasm: {
      print: (message: string) => process.stderr.write(`${message}\n`),
      printErr: (message: string) => process.stderr.write(`${message}\n`),
    },
  });
  try {
    const doc = await engine.open(
      { kind: 'bytes', id: `redaction:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      if (needsStrippedFixture(entry)) {
        // Apply everything first, so the second apply has nothing left.
        await doc.redaction.apply({ pages: [toPageRef(PAGE)] });
      }
      const scope = await v3Scope(doc, entry.scope);
      const run = () => doc.redaction.apply(scope);
      // `request` is the wire request the case's old spelling became, which
      // the Rust runner replays (see `recording-engine.ts`).
      const sent = await capture(run);
      if (entry.refused) {
        if (sent.ok) throw new Error(`${entry.name}: expected a refusal`);
        const code = String((sent.error as { code?: unknown })?.code ?? 'unknown');
        return { ...entry, request: sent.request, refusedWith: code };
      }
      if (!sent.ok) throw sent.error;
      const result = sent.value;
      // What is left on the page afterwards, which is how a caller sees
      // that the markings went and the rest did not.
      const page = doc.page(toPageRef(PAGE));
      const remaining = (await page.annotations.list()).annotations.map(
        (annotation) => ({
          subtype: annotation.subtype,
          nm: annotation.nm,
        }),
      );
      return {
        ...entry,
        request: sent.request,
        results: result.results,
        removedAnnotationCount: result.removedAnnotationCount,
        remaining,
      };
    } finally {
      await doc.close();
    }
  } finally {
    await engine.destroy();
  }
}

function serialize(cases: readonly unknown[]): string {
  const head = JSON.stringify(
    { schemaVersion: SCHEMA_VERSION, kind: 'redaction.apply', fixture: FIXTURE },
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
  const absolute = join(vectorsRoot, 'apply.json');
  if (check) {
    const current = await readFile(absolute, 'utf8').catch(() => '');
    if (current !== text) {
      process.stderr.write('  STALE   redaction/apply.json\n');
      process.exitCode = 1;
      return;
    }
    process.stdout.write('  ok      redaction/apply.json\n');
    return;
  }
  await mkdir(vectorsRoot, { recursive: true });
  await writeFile(absolute, text, 'utf8');
  process.stdout.write('  written redaction/apply.json\n');
}

main().catch((error) => {
  process.stderr.write(`generate-redaction-vectors: ${String(error?.stack ?? error)}\n`);
  process.exitCode = 1;
});
