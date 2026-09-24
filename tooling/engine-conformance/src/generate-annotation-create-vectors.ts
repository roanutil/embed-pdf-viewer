/**
 * Emits `vectors/annotation/create.json`: what `annotations.create` puts in
 * the document, read back through the ordinary reader.
 *
 *   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-create
 *   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-create:check
 *
 * G11's first mutator gate. `crates/epdf-engine/tests/annotation_create.rs`
 * replays it.
 *
 * # The read is the assertion
 *
 * What is recorded is the DTO the create RETURNED, which the mutator obtains
 * by reading the new annotation back through the same reader a listing uses.
 * So the comparison is between two documents rather than between two
 * descriptions of an intent: anything PDFium normalized on the way in shows
 * up on both sides or on neither.
 *
 * `modified` is dropped. It is `/M`, stamped from the clock at creation, and
 * the two engines run at different instants. Its PRESENCE is asserted by the
 * Rust runner instead.
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

/** Every draft is created on this page of this fixture. */
const FIXTURE = 'tooling/engine-conformance/fixtures/rich_metadata.pdf';

const QUAD = {
  p1: { x: 72, y: 720 },
  p2: { x: 200, y: 720 },
  p3: { x: 72, y: 700 },
  p4: { x: 200, y: 700 },
};
const SECOND_QUAD = {
  p1: { x: 72, y: 690 },
  p2: { x: 160, y: 690 },
  p3: { x: 72, y: 670 },
  p4: { x: 160, y: 670 },
};

interface CreateCase {
  readonly name: string;
  readonly covers: string;
  readonly draft: Record<string, unknown>;
}

const CASES: readonly CreateCase[] = [
  {
    name: 'highlight-defaults',
    covers:
      'A highlight with nothing but its quads. Every default the writer supplies is visible here: the family colour, the opacity it writes rather than omits, and the /Rect derived from the quads.',
    draft: { subtype: 'highlight', quadPoints: [QUAD] },
  },
  {
    name: 'highlight-two-quads',
    covers:
      'Two quads, so /Rect has to enclose both. A writer that used only the first would produce a box that is right for one line of a two-line selection.',
    draft: { subtype: 'highlight', quadPoints: [QUAD, SECOND_QUAD] },
  },
  {
    name: 'highlight-everything',
    covers:
      'Colour, opacity, contents, subject, an /NM the caller chose, and flags. The whole base surface at once.',
    draft: {
      subtype: 'highlight',
      quadPoints: [QUAD],
      color: { r: 0, g: 128, b: 255 },
      opacity: 0.4,
      contents: 'a note',
      subject: 'a subject',
      nm: 'caller-chosen-id',
      flags: { print: true, noZoom: true },
    },
  },
  {
    name: 'underline-default-colour',
    covers:
      'The other text-markup default. Highlight falls back to yellow and the rest to black, so a writer with one fallback for all four agrees with three.',
    draft: { subtype: 'underline', quadPoints: [QUAD] },
  },
  {
    name: 'squiggly',
    covers: 'The third of the family, to prove the subtype code reaches the document.',
    draft: { subtype: 'squiggly', quadPoints: [QUAD] },
  },
  {
    name: 'strikeout-plain',
    covers: 'A standalone strikeout, which has no /IT at all.',
    draft: { subtype: 'strikeout', quadPoints: [QUAD] },
  },
  {
    name: 'strikeout-text-edit',
    covers:
      'The subordinate half of a replace-text edit. /IT is written as StrikeOutTextEdit, with the capital O that neither the subtype nor the wire word has.',
    draft: { subtype: 'strikeout', quadPoints: [QUAD], intent: 'strikeout-text-edit' },
  },
  {
    name: 'square-defaults',
    covers:
      'A square with nothing but its rect. The resolved border a reader reports for an annotation with no /BS is what the writer has to produce.',
    draft: { subtype: 'square', rect: { left: 300, bottom: 600, right: 400, top: 660 } },
  },
  {
    name: 'square-decorated',
    covers:
      'Every optional shape entry: a dashed border with a pattern, a fill, a cloudy intensity and an /RD inset.',
    draft: {
      subtype: 'square',
      rect: { left: 300, bottom: 500, right: 420, top: 580 },
      color: { r: 255, g: 0, b: 0 },
      interiorColor: { r: 0, g: 255, b: 0 },
      opacity: 0.8,
      strokeWidth: 3,
      borderStyle: 'dashed',
      dashArray: [4, 2],
      cloudyIntensity: 2,
      rectDifferences: { left: 1, top: 4, right: 3, bottom: 2 },
    },
  },
  {
    name: 'square-degenerate-cloudy',
    covers:
      'A cloudy intensity of zero. The wire domain is positive-or-null, so the writer has to clear rather than write a value the reader would then normalize away.',
    draft: {
      subtype: 'square',
      rect: { left: 300, bottom: 400, right: 400, top: 460 },
      cloudyIntensity: 0,
    },
  },
  {
    name: 'polygon',
    covers:
      'A closed vertex path with a fill and a cloudy border. Only a polygon takes /BE: an open path has no interior for one to enclose.',
    draft: {
      subtype: 'polygon',
      rect: { left: 300, bottom: 300, right: 420, top: 380 },
      vertices: [
        { x: 300, y: 300 },
        { x: 420, y: 300 },
        { x: 360, y: 380 },
      ],
      color: { r: 0, g: 0, b: 255 },
      interiorColor: { r: 255, g: 255, b: 0 },
      cloudyIntensity: 1,
    },
  },
  {
    name: 'polyline-with-endings',
    covers:
      'An open vertex path with a /LE pair, which the polygon family does not have. The two endings differ, so a writer that wrote one twice is visible.',
    draft: {
      subtype: 'polyline',
      rect: { left: 300, bottom: 200, right: 420, top: 280 },
      vertices: [
        { x: 300, y: 200 },
        { x: 360, y: 280 },
        { x: 420, y: 200 },
      ],
      lineEndings: { start: 'open-arrow', end: 'diamond' },
    },
  },
  {
    name: 'polyline-defaults',
    covers:
      'The same without a /LE, so the pair that draws nothing is what a reader gets back rather than an absent entry it has to interpret.',
    draft: {
      subtype: 'polyline',
      rect: { left: 450, bottom: 200, right: 560, top: 280 },
      vertices: [
        { x: 450, y: 200 },
        { x: 560, y: 280 },
      ],
    },
  },
  {
    name: 'line',
    covers: 'Two endpoints and a pair of endings, which is the whole of a line annotation.',
    draft: {
      subtype: 'line',
      rect: { left: 450, bottom: 300, right: 560, top: 400 },
      linePoints: { start: { x: 450, y: 300 }, end: { x: 560, y: 400 } },
      lineEndings: { start: 'circle', end: 'closed-arrow' },
      color: { r: 255, g: 0, b: 255 },
      strokeWidth: 2,
    },
  },
  {
    name: 'ink-two-strokes',
    covers:
      'Two pen strokes, so a writer that appended only the first is visible, with the highlighter intent that is the only /IT ink carries.',
    draft: {
      subtype: 'ink',
      rect: { left: 72, bottom: 200, right: 240, top: 300 },
      inkList: [
        [
          { x: 72, y: 200 },
          { x: 120, y: 280 },
          { x: 160, y: 200 },
        ],
        [
          { x: 180, y: 220 },
          { x: 240, y: 300 },
        ],
      ],
      intent: 'ink-highlight',
      opacity: 0.3,
    },
  },
  {
    name: 'polygon-zero-cloud',
    covers:
      'A cloudy intensity of zero, which is not a cloud. A writer that passed it through writes a /BE that says the border is cloudy with no waviness, which is a different dictionary from the one a plain border has.',
    draft: {
      subtype: 'polygon',
      rect: { left: 450, bottom: 450, right: 560, top: 540 },
      vertices: [
        { x: 450, y: 450 },
        { x: 560, y: 450 },
        { x: 505, y: 540 },
      ],
      cloudyIntensity: 0,
    },
  },
  {
    name: 'polyline-zero-rotation',
    covers:
      'A rotation of zero. Zero degrees is not a rotation, so the key is cleared rather than stamped, and a file that had none keeps none.',
    draft: {
      subtype: 'polyline',
      rect: { left: 300, bottom: 450, right: 420, top: 540 },
      vertices: [
        { x: 300, y: 450 },
        { x: 420, y: 540 },
      ],
      rotation: 0,
    },
  },
  {
    name: 'ink-advisory-rotation',
    covers:
      'A rotation scalar on a vertex kind. The strokes are already rotated, so this records the angle and never drives the generator; it also never carries a logical box, which is what separates it from the shape family.',
    draft: {
      subtype: 'ink',
      rect: { left: 72, bottom: 100, right: 240, top: 180 },
      inkList: [[{ x: 72, y: 100 }, { x: 240, y: 180 }]],
      rotation: 45,
    },
  },
  {
    name: 'highlight-cleared-strings',
    covers:
      'A draft that explicitly clears contents and subject. Setting a null value writes an EMPTY string rather than removing the key, so a writer that used the setter would produce a field that reads back as present-and-empty instead of absent.',
    draft: { subtype: 'highlight', quadPoints: [QUAD], contents: null, subject: null },
  },
  {
    name: 'highlight-empty-nm',
    covers:
      'An /NM the caller supplied as an empty string. An empty name is not an identity, so it is not written: two annotations could both carry it and neither would be addressable.',
    draft: { subtype: 'highlight', quadPoints: [QUAD], nm: '' },
  },
  {
    name: 'square-null-interior',
    covers:
      'A shape whose fill is explicitly null, beside the default case where it is simply absent. Both mean a shape with no fill.',
    draft: {
      subtype: 'square',
      rect: { left: 450, bottom: 400, right: 550, top: 460 },
      interiorColor: null,
    },
  },
  {
    name: 'circle-defaults',
    covers: 'The other shape subtype, which shares every field and differs only in its code.',
    draft: { subtype: 'circle', rect: { left: 450, bottom: 600, right: 550, top: 660 } },
  },
];

async function readCase(entry: CreateCase): Promise<unknown> {
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
      { kind: 'bytes', id: `create:${entry.name}`, bytes },
      { scope: ['*'] },
    );
    try {
      const listing = await doc.pages.list();
      const first = listing.pages[0];
      if (!first) throw new Error(`${entry.name}: the fixture has no pages`);
      const page = doc.page(first.pageObjectNumber);
      const result = await page.annotations.create(entry.draft as never);

      // The whole created DTO minus the timestamp, plus the envelope's own
      // conclusions, which are what the impact classifier decided.
      const { modified, ...created } = result.created as Record<string, unknown>;
      return {
        ...entry,
        pageObjectNumber: first.pageObjectNumber,
        created,
        hasModified: typeof modified === 'string' && modified.length > 0,
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
    { schemaVersion: SCHEMA_VERSION, kind: 'annotation.create', fixture: FIXTURE },
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
  const absolute = join(vectorsRoot, 'create.json');

  let current: string | undefined;
  try {
    current = await readFile(absolute, 'utf8');
  } catch {
    current = undefined;
  }
  if (current === text) {
    process.stdout.write('  ok      annotation/create.json\n');
    return;
  }
  if (check) {
    process.stdout.write(
      `  DIFFERS annotation/create.json${current === undefined ? ' (missing)' : ''}\n`,
    );
    process.stderr.write(
      '\nAnnotation create vectors differ. Regenerate with:\n' +
        '  pnpm --filter @embedpdf/tooling-engine-conformance run annotation-create\n' +
        'and commit the diff.\n',
    );
    process.exitCode = 1;
    return;
  }
  await mkdir(dirname(absolute), { recursive: true });
  await writeFile(absolute, text);
  process.stdout.write('  written annotation/create.json\n');
}

main().catch((error: unknown) => {
  process.stderr.write(
    `generate-annotation-create-vectors: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
