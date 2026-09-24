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
  /**
   * Binary payloads the draft names by key. A draft is JSON and a file is
   * not, so the recorded draft keeps the key and the bytes are recorded
   * beside it as a number array for the Rust runner to rebuild.
   */
  readonly resources?: Readonly<Record<string, readonly number[]>>;
}

/**
 * Turn every `{ resource: key }` in a draft into the inline bytes the public
 * API takes. The recorded draft keeps the key: that is what the Rust engine
 * parses, and its own write context carries the same payloads.
 */
function materialize(value: unknown, resources: Record<string, readonly number[]>): unknown {
  if (Array.isArray(value)) return value.map((entry) => materialize(entry, resources));
  if (value === null || typeof value !== 'object') return value;
  const entries = Object.entries(value as Record<string, unknown>);
  const ref = entries.find(([key]) => key === 'resource');
  if (ref && typeof ref[1] === 'string') {
    const bytes = resources[ref[1]];
    if (!bytes) throw new Error(`no payload for resource '${ref[1]}'`);
    return Object.fromEntries([
      ['data', Uint8Array.from(bytes)],
      ...entries.filter(([key]) => key !== 'resource'),
    ]);
  }
  return Object.fromEntries(entries.map(([key, entry]) => [key, materialize(entry, resources)]));
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
    name: 'square-rotated',
    covers:
      'A rotated box. `/Rect` is the enclosing axis-aligned box of the turned square; the pair under /EMBD_Metadata is what makes the generator bake a turned appearance rather than an upright one.',
    draft: {
      subtype: 'square',
      rect: { left: 90, bottom: 390, right: 210, top: 510 },
      unrotatedRect: { left: 110, bottom: 410, right: 190, top: 490 },
      rotation: 45,
      color: { r: 0, g: 128, b: 0 },
      strokeWidth: 3,
    },
  },
  {
    name: 'square-zero-rotation',
    covers:
      'A box whose logical box is its rect and whose rotation is zero. Zero degrees clears the key rather than writing it, so the annotation carries a box and no angle.',
    draft: {
      subtype: 'square',
      rect: { left: 240, bottom: 390, right: 340, top: 470 },
      unrotatedRect: { left: 240, bottom: 390, right: 340, top: 470 },
      rotation: 0,
    },
  },
  {
    name: 'square-zero-rotation-no-box',
    covers:
      'Zero degrees with no logical box, which is not the unsatisfiable pair. Nothing is being rotated, so there is nothing for a box to be the pre-rotation form of.',
    draft: {
      subtype: 'square',
      rect: { left: 350, bottom: 390, right: 430, top: 470 },
      rotation: 0,
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
    name: 'text-defaults',
    covers:
      'A sticky note with nothing named. The icon and the yellow fill are written explicitly rather than left to the file, so a read reports the same thing a draft that named them would.',
    draft: {
      subtype: 'text',
      rect: { left: 72, bottom: 600, right: 92, top: 620 },
    },
  },
  {
    name: 'text-reviewed',
    covers:
      'An icon other than the default, and the review pair. `/State` and `/StateModel` are dictionary-only, so a note that has been accepted is drawn exactly like one that has not.',
    draft: {
      subtype: 'text',
      rect: { left: 110, bottom: 600, right: 130, top: 620 },
      icon: 'help',
      color: { r: 0, g: 160, b: 255 },
      opacity: 0.8,
      state: 'accepted',
      stateModel: 'review',
      contents: 'Looks right to me.',
    },
  },
  {
    name: 'text-model-alone',
    covers:
      'A state model with no state, which §12.5.6.3 allows: the model implies its own default. The reverse is refused, and that refusal is a draft-only rule.',
    draft: {
      subtype: 'text',
      rect: { left: 150, bottom: 600, right: 170, top: 620 },
      stateModel: 'marked',
    },
  },
  {
    name: 'text-foreign-state',
    covers:
      'A model nobody standardized. Acrobat lets a workflow register one, and the reader passes it through, so the writer has to be able to write it back rather than normalize it into a standard name.',
    draft: {
      subtype: 'text',
      rect: { left: 190, bottom: 600, right: 210, top: 620 },
      stateModel: 'LegalReview',
      state: 'AwaitingCounsel',
    },
  },
  {
    name: 'caret-defaults',
    covers: 'A caret with only a rect. The red mark is the engine-wide default.',
    draft: {
      subtype: 'caret',
      rect: { left: 72, bottom: 540, right: 84, top: 560 },
    },
  },
  {
    name: 'caret-replace',
    covers:
      'The caret half of a replace-text edit, with the inset of the drawn symbol from its box.',
    draft: {
      subtype: 'caret',
      rect: { left: 110, bottom: 540, right: 130, top: 570 },
      intent: 'replace',
      color: { r: 0, g: 0, b: 255 },
      opacity: 0.5,
      rectDifferences: { left: 2, bottom: 3, right: 4, top: 5 },
    },
  },
  {
    name: 'caret-rotated',
    covers:
      'A caret is a BOX kind, so its rotation is the pair and not the lone advisory scalar the vertex families carry.',
    draft: {
      subtype: 'caret',
      rect: { left: 150, bottom: 530, right: 180, top: 570 },
      rotation: 90,
      unrotatedRect: { left: 155, bottom: 535, right: 175, top: 565 },
    },
  },
  {
    name: 'free-text-plain',
    covers:
      'A plain text box. The three colours are three different entries: /DA draws the border and the text, /C is the background, and a TextColor override is absent here so the text follows /DA.',
    draft: {
      subtype: 'free-text',
      intent: 'free-text',
      rect: { left: 72, bottom: 400, right: 260, top: 470 },
      fontFamily: 'helvetica',
      fontSize: 14,
      textAlign: 'left',
      contents: 'A note in a box.',
    },
  },
  {
    name: 'free-text-three-colours',
    covers:
      'All three at once, with a text colour that overrides the /DA one. A writer that wrote /DA after the override would put the border colour back over the text.',
    draft: {
      subtype: 'free-text',
      intent: 'free-text',
      rect: { left: 280, bottom: 400, right: 470, top: 470 },
      fontFamily: 'times-bold',
      fontSize: 18,
      textAlign: 'center',
      color: { r: 0, g: 0, b: 200 },
      fontColor: { r: 200, g: 0, b: 0 },
      interiorColor: { r: 240, g: 240, b: 200 },
      opacity: 0.9,
      strokeWidth: 2,
      borderStyle: 'dashed',
      dashArray: [4, 2],
      rectDifferences: { left: 3, bottom: 3, right: 3, top: 3 },
      contents: 'Red text, blue border, cream box.',
    },
  },
  {
    name: 'free-text-callout',
    covers:
      'A callout: the same annotation with a /CL leader and an ending on the called-out end. The other end touches the text box, where an arrowhead would be drawn into the box it belongs to.',
    draft: {
      subtype: 'free-text',
      intent: 'free-text-callout',
      rect: { left: 300, bottom: 300, right: 470, top: 360 },
      fontFamily: 'helvetica-oblique',
      fontSize: 11,
      textAlign: 'right',
      calloutLine: [
        { x: 200, y: 250 },
        { x: 260, y: 300 },
        { x: 300, y: 330 },
      ],
      lineEnding: 'open-arrow',
      contents: 'This bit here.',
    },
  },
  {
    name: 'free-text-rotated',
    covers:
      'A box kind, so the rotation is the pair. A callout would rotate its text box alone and leave the leader in page space.',
    draft: {
      subtype: 'free-text',
      intent: 'free-text',
      rect: { left: 72, bottom: 300, right: 190, top: 380 },
      fontFamily: 'courier',
      fontSize: 10,
      textAlign: 'left',
      rotation: 15,
      unrotatedRect: { left: 80, bottom: 310, right: 180, top: 370 },
    },
  },
  {
    name: 'redact-area',
    covers:
      'An area redaction: no quads, so /Rect is the region. Nothing here removes anything; the marking declares what apply would do.',
    draft: {
      subtype: 'redact',
      rect: { left: 72, bottom: 200, right: 260, top: 240 },
    },
  },
  {
    name: 'redact-text-with-label',
    covers:
      'A text redaction with quads, the /IC fill painted after apply, and a tiled label. The /DA is emitted because a label obliges one.',
    draft: {
      subtype: 'redact',
      // Deliberately wider than the quads enclose: a marking gets a margin,
      // and a writer that derived the box from the quads the way the
      // text-markup family does would lose it.
      rect: { left: 276, bottom: 196, right: 474, top: 244 },
      quadPoints: [
        {
          p1: { x: 280, y: 240 },
          p2: { x: 470, y: 240 },
          p3: { x: 280, y: 220 },
          p4: { x: 470, y: 220 },
        },
        {
          p1: { x: 280, y: 218 },
          p2: { x: 400, y: 218 },
          p3: { x: 280, y: 200 },
          p4: { x: 400, y: 200 },
        },
      ],
      color: { r: 255, g: 128, b: 0 },
      interiorColor: { r: 0, g: 0, b: 0 },
      opacity: 0.6,
      overlayText: 'REDACTED',
      repeat: true,
      fontFamily: 'helvetica-bold',
      fontSize: 9,
      fontColor: { r: 255, g: 255, b: 255 },
      textAlign: 'center',
    },
  },
  {
    name: 'redact-autofit-label',
    covers:
      'A label at size zero, which for a redaction means auto-fit to the region rather than nothing. Free text refuses the same number.',
    draft: {
      subtype: 'redact',
      rect: { left: 72, bottom: 150, right: 260, top: 190 },
      overlayText: 'gone',
      fontSize: 0,
    },
  },
  {
    name: 'redact-empty-label',
    covers:
      'An empty overlay text, which is not a label. A writer that wrote it would put an /OverlayText on the annotation that draws nothing and obliges a /DA beside it.',
    draft: {
      subtype: 'redact',
      rect: { left: 280, bottom: 150, right: 470, top: 190 },
      overlayText: '',
    },
  },
  {
    name: 'file-attachment',
    covers:
      'A marker and the file it names. PDFium derives the size, the checksum and the creation date from the bytes, so the read back proves the stream was written and not just the filespec.',
    resources: { r0: [116, 104, 101, 32, 113, 117, 105, 99, 107, 32, 98, 114, 111, 119, 110, 32, 102, 111, 120, 10] },
    draft: {
      subtype: 'file-attachment',
      rect: { left: 72, bottom: 100, right: 92, top: 120 },
      file: {
        resource: 'r0',
        name: 'notes.txt',
        mimeType: 'text/plain',
        description: 'A short note.',
      },
      icon: 'tag',
      color: { r: 0, g: 200, b: 120 },
    },
  },
  {
    name: 'file-attachment-defaults',
    covers:
      'The writer default icon is the paperclip, which is NOT what an absent /Name reads as. The key is always written, so the two never disagree about one file.',
    resources: { r0: [] },
    draft: {
      subtype: 'file-attachment',
      rect: { left: 110, bottom: 100, right: 130, top: 120 },
      file: { resource: 'r0', name: 'empty.bin' },
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
      const draft = materialize(entry.draft, { ...(entry.resources ?? {}) }) as never;
      const result = await page.annotations.create(draft);

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

/**
 * See `generate-annotation-read-vectors.ts`. `creationDate` joins the list
 * for the same reason `modified` is dropped: PDFium stamps it from the clock
 * when the attachment's bytes are written, so the two engines record
 * different instants for the same file. Its PRESENCE is what matters, and
 * the Rust runner asserts that.
 */
function normalizeSessionIds(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(normalizeSessionIds);
  if (value === null || typeof value !== 'object') return value;
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>).map(([key, entry]) => {
      if (key === 'docSessionId') return [key, '<session>'];
      if (key === 'creationDate' && typeof entry === 'string') return [key, '<stamped>'];
      return [key, normalizeSessionIds(entry)];
    }),
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
