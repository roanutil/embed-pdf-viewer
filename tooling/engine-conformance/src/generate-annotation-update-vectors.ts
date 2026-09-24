/**
 * Emits `vectors/annotation/update.json`: what `annotations.update` leaves in
 * the document and what it says about the appearance.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-update
 *   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-update:check
 *
 * `crates/epdf-engine/tests/annotation_update.rs` replays it.
 *
 * Each case CREATES its subject first, from a draft, and then patches it. The
 * alternative is patching a fixture annotation, which would make every case
 * depend on how that fixture happened to be written; creating the subject
 * means each case says what it starts from.
 *
 * The appearance verdict is recorded alongside the DTO, because it is the
 * half a field comparison cannot see: preserving a foreign stream and
 * re-baking an identical-looking one produce the same dictionary and
 * different documents.
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

const FIXTURE = 'tooling/engine-conformance/fixtures/rich_metadata.pdf';

const QUAD = {
  p1: { x: 72, y: 720 },
  p2: { x: 200, y: 720 },
  p3: { x: 72, y: 700 },
  p4: { x: 200, y: 700 },
};
const SHIFTED_QUAD = {
  p1: { x: 102, y: 700 },
  p2: { x: 230, y: 700 },
  p3: { x: 102, y: 680 },
  p4: { x: 230, y: 680 },
};
const SECOND_QUAD = {
  p1: { x: 72, y: 690 },
  p2: { x: 160, y: 690 },
  p3: { x: 72, y: 670 },
  p4: { x: 160, y: 670 },
};

const SQUARE_RECT = { left: 300, bottom: 600, right: 400, top: 660 };

interface UpdateCase {
  readonly name: string;
  readonly covers: string;
  readonly draft: Record<string, unknown>;
  readonly patch: Record<string, unknown>;
}

const CASES: readonly UpdateCase[] = [
  {
    name: 'inert-patch',
    covers:
      'A patch that repeats what is already there. Nothing changed value, so the appearance is preserved and a client is told not to re-raster.',
    draft: { subtype: 'highlight', quadPoints: [QUAD], color: { r: 255, g: 255, b: 0 } },
    patch: { subtype: 'highlight', color: { r: 255, g: 255, b: 0 } },
  },
  {
    name: 'metadata-only',
    covers:
      'Contents and flags, neither of which is painted on a highlight. Still inert, which is what stops editing a comment from destroying a shape.',
    draft: { subtype: 'highlight', quadPoints: [QUAD] },
    patch: { subtype: 'highlight', contents: 'a note', flags: { hidden: true } },
  },
  {
    name: 'colour-change',
    covers: 'A semantic edit. The appearance is re-baked and the client is told so.',
    draft: { subtype: 'highlight', quadPoints: [QUAD] },
    patch: { subtype: 'highlight', color: { r: 0, g: 0, b: 255 } },
  },
  {
    name: 'quad-translation',
    covers:
      "A text markup whose quads all move by one delta, which re-bakes rather than preserving. A markup's /Rect is DERIVED from its quads rather than patched, so the patch carries no rect, and the rigid-translation check requires one: it has no way to know the box moved with them. The shape family, whose rect IS the patch, takes the other road.",
    draft: { subtype: 'highlight', quadPoints: [QUAD] },
    patch: { subtype: 'highlight', quadPoints: [SHIFTED_QUAD] },
  },
  {
    name: 'quad-growth',
    covers:
      'A second quad added to a one-quad markup. PDFium can grow the list and not shrink it, so this is the direction that works, and /Rect has to enclose both afterwards.',
    draft: { subtype: 'highlight', quadPoints: [QUAD] },
    patch: { subtype: 'highlight', quadPoints: [QUAD, SECOND_QUAD] },
  },
  {
    name: 'clearing-contents',
    covers:
      'A tri-state clear of a value that was there. This is where writing an empty string instead of removing the key becomes visible: the read side reports absent and empty differently.',
    draft: { subtype: 'highlight', quadPoints: [QUAD], contents: 'written at creation' },
    patch: { subtype: 'highlight', contents: null },
  },
  {
    name: 'flags-preserve-what-they-do-not-mention',
    covers:
      'The annotation is created printing and the patch hides it. Hiding must not also stop it printing.',
    draft: { subtype: 'highlight', quadPoints: [QUAD], flags: { print: true } },
    patch: { subtype: 'highlight', flags: { hidden: true } },
  },
  {
    name: 'square-rect-translation',
    covers:
      'A shape moved without resizing, which DOES preserve: the patch carries the rect, so the translation can be verified. Read beside quad-translation, which cannot be.',
    draft: { subtype: 'square', rect: SQUARE_RECT },
    patch: {
      subtype: 'square',
      rect: { left: 330, bottom: 585, right: 430, top: 645 },
    },
  },
  {
    name: 'square-resize',
    covers: 'A shape resized. The pixels would be scaled rather than shifted, so it re-bakes.',
    draft: { subtype: 'square', rect: SQUARE_RECT },
    patch: { subtype: 'square', rect: { left: 300, bottom: 600, right: 500, top: 660 } },
  },
  {
    name: 'square-border-width-only',
    covers:
      'A width with no style. /BS is one native call taking both, so the style has to be read rather than defaulted, or a patch that never mentioned it would reset it.',
    draft: { subtype: 'square', rect: SQUARE_RECT, borderStyle: 'dashed', dashArray: [4, 2] },
    patch: { subtype: 'square', strokeWidth: 5 },
  },
  {
    name: 'square-clear-interior',
    covers: 'A tri-state clear of a fill that was there.',
    draft: {
      subtype: 'square',
      rect: SQUARE_RECT,
      interiorColor: { r: 0, g: 255, b: 0 },
    },
    patch: { subtype: 'square', interiorColor: null },
  },
  {
    name: 'square-clear-cloudy',
    covers: 'A tri-state clear of a cloudy border that was there.',
    draft: { subtype: 'square', rect: SQUARE_RECT, cloudyIntensity: 2 },
    patch: { subtype: 'square', cloudyIntensity: null },
  },
  {
    name: 'strikeout-intent',
    covers: 'The /IT a strikeout can carry, set by a patch rather than at creation.',
    draft: { subtype: 'strikeout', quadPoints: [QUAD] },
    patch: { subtype: 'strikeout', intent: 'strikeout-text-edit' },
  },
];

async function readCase(entry: UpdateCase): Promise<unknown> {
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
      { kind: 'bytes', id: `update:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      const listing = await doc.pages.list();
      const first = listing.pages[0];
      if (!first) throw new Error(`${entry.name}: the fixture has no pages`);
      const page = doc.page(first.pageObjectNumber);

      const created = await page.annotations.create(entry.draft as never);
      const result = await page.annotations.update(
        created.created.ref,
        entry.patch as never,
      );

      const { modified, ...updated } = result.updated as Record<string, unknown>;
      return {
        ...entry,
        pageObjectNumber: first.pageObjectNumber,
        updated,
        hasModified: typeof modified === 'string' && modified.length > 0,
        appearance: result.appearance,
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
    { schemaVersion: SCHEMA_VERSION, kind: 'annotation.update', fixture: FIXTURE },
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
  const absolute = join(vectorsRoot, 'update.json');

  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write('  ok      annotation/update.json\n');
    return;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS annotation/update.json${current === undefined ? ' (missing)' : ''}\n`,
    );
    process.stderr.write(
      '\nAnnotation update vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run annotation-update\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write('  written annotation/update.json\n');
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-annotation-update-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
