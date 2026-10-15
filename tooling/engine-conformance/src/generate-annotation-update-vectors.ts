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
import { toPageRef } from '../../../packages/engine/core/src/identity/PageRef';

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

const TRIANGLE_RECT = { left: 300, bottom: 300, right: 420, top: 380 };
const NOTE_RECT = { left: 72, bottom: 600, right: 92, top: 620 };
const CARET_RECT = { left: 150, bottom: 530, right: 180, top: 570 };
const FREE_TEXT_RECT = { left: 72, bottom: 400, right: 260, top: 470 };
const REDACT_RECT = { left: 72, bottom: 200, right: 260, top: 240 };
const LINK_RECT = { left: 72, bottom: 640, right: 200, top: 660 };
const STAMP_RECT = { left: 72, bottom: 400, right: 152, top: 480 };
const WIDGET_RECT = { left: 72, bottom: 250, right: 240, top: 274 };
const TRIANGLE = [
  { x: 300, y: 300 },
  { x: 420, y: 300 },
  { x: 360, y: 380 },
];

interface UpdateCase {
  readonly name: string;
  readonly covers: string;
  readonly draft: Record<string, unknown>;
  readonly patch: Record<string, unknown>;
  /** Payloads the draft or the patch names by key. See the create generator. */
  readonly resources?: Readonly<Record<string, readonly number[]>>;
}

/** See `generate-annotation-create-vectors.ts`. */
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

/** A tiny 4x2 PNG, for the stamp cases. */
const STAMP_PNG = [137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 2, 8, 2, 0, 0, 0, 240, 202, 234, 52, 0, 0, 0, 16, 73, 68, 65, 84, 120, 156, 99, 184, 163, 161, 1, 71, 12, 200, 28, 0, 127, 154, 9, 97, 122, 180, 22, 59, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130];

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
  {
    name: 'polygon-fewer-vertices',
    covers:
      'A vertex list that SHRINKS, which /QuadPoints cannot do. SetVertices replaces the array rather than appending to it, so there is no stale tail and no preflight to refuse one.',
    draft: {
      subtype: 'polygon',
      rect: TRIANGLE_RECT,
      vertices: [
        { x: 300, y: 300 },
        { x: 420, y: 300 },
        { x: 420, y: 380 },
        { x: 300, y: 380 },
      ],
    },
    patch: {
      subtype: 'polygon',
      vertices: TRIANGLE,
    },
  },
  {
    name: 'polygon-degenerate',
    covers:
      'A polygon flattened onto a line. PDFium has no appearance to bake for one, so the verdict is a third thing: the edit landed and the stream could not be rebuilt. A client is told rather than shown a stale picture.',
    draft: {
      subtype: 'polygon',
      rect: TRIANGLE_RECT,
      vertices: TRIANGLE,
    },
    patch: {
      subtype: 'polygon',
      rect: { left: 300, bottom: 300, right: 420, top: 300 },
      vertices: [
        { x: 300, y: 300 },
        { x: 420, y: 300 },
      ],
    },
  },
  {
    name: 'polygon-clear-cloudy',
    covers: 'A polygon is the one vertex kind with a /BE, and it clears on the same terms a shape does.',
    draft: {
      subtype: 'polygon',
      rect: TRIANGLE_RECT,
      vertices: TRIANGLE,
      cloudyIntensity: 2,
    },
    patch: { subtype: 'polygon', cloudyIntensity: null },
  },
  {
    name: 'polygon-clear-interior',
    covers:
      'A tri-state clear of a fill that was there. On the create path clearing and not saying are the same thing; here they are not.',
    draft: {
      subtype: 'polygon',
      rect: TRIANGLE_RECT,
      vertices: TRIANGLE,
      interiorColor: { r: 0, g: 255, b: 0 },
    },
    patch: { subtype: 'polygon', interiorColor: null },
  },
  {
    name: 'polygon-width-only',
    covers:
      'A border width with no style. /BS takes both in one call, so the writer has to READ the style rather than default it: defaulting would reset a dashed border nobody mentioned.',
    draft: {
      subtype: 'polygon',
      rect: TRIANGLE_RECT,
      vertices: TRIANGLE,
      borderStyle: 'dashed',
      dashArray: [4, 2],
    },
    patch: { subtype: 'polygon', strokeWidth: 4 },
  },
  {
    name: 'polygon-style-only',
    covers:
      'The other half of the /BS read-modify-write: a style with no width. Defaulting the width would reset a three-point border nobody mentioned.',
    draft: {
      subtype: 'polygon',
      rect: TRIANGLE_RECT,
      vertices: TRIANGLE,
      strokeWidth: 3,
    },
    patch: { subtype: 'polygon', borderStyle: 'dashed', dashArray: [5, 3] },
  },
  {
    name: 'polygon-clear-rotation',
    covers:
      'The advisory scalar, removed. A vertex kind carries an angle and no logical box, so clearing it is the whole of un-rotating one.',
    draft: {
      subtype: 'polygon',
      rect: TRIANGLE_RECT,
      vertices: TRIANGLE,
      rotation: 30,
    },
    patch: { subtype: 'polygon', rotation: null },
  },
  {
    name: 'polyline-endings',
    covers: 'The /LE pair, changed after creation. The two ends differ, so a writer that wrote one twice is visible.',
    draft: {
      subtype: 'polyline',
      rect: TRIANGLE_RECT,
      vertices: TRIANGLE,
      lineEndings: { start: 'none', end: 'none' },
    },
    patch: { subtype: 'polyline', lineEndings: { start: 'diamond', end: 'open-arrow' } },
  },
  {
    name: 'line-endpoints',
    covers: 'Moving a line is moving its two points, and its /Rect is the caller arithmetic over them.',
    draft: {
      subtype: 'line',
      rect: { left: 450, bottom: 300, right: 560, top: 400 },
      linePoints: { start: { x: 450, y: 300 }, end: { x: 560, y: 400 } },
    },
    patch: {
      subtype: 'line',
      rect: { left: 450, bottom: 300, right: 520, top: 340 },
      linePoints: { start: { x: 450, y: 340 }, end: { x: 520, y: 300 } },
    },
  },
  {
    name: 'ink-replace-strokes',
    covers:
      'A new pen path. There is no way to replace one stroke, so the whole list goes and the new one is appended; half of that leaves the old strokes under the new ones.',
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
    },
    patch: {
      subtype: 'ink',
      inkList: [
        [
          { x: 80, y: 210 },
          { x: 130, y: 260 },
        ],
      ],
    },
  },
  {
    name: 'text-state-alone',
    covers:
      'A /State with no /StateModel, which a DRAFT refuses. A patch touches what it states and the model is already on the annotation, so the rule is draft-only.',
    draft: {
      subtype: 'text',
      rect: NOTE_RECT,
      stateModel: 'review',
    },
    patch: { subtype: 'text', state: 'accepted' },
  },
  {
    name: 'text-clear-state',
    covers:
      'A tri-state clear of the review state. Removing the entry is not the same as setting it to none, which is a state a workflow can mean.',
    draft: {
      subtype: 'text',
      rect: NOTE_RECT,
      stateModel: 'review',
      state: 'accepted',
    },
    patch: { subtype: 'text', state: null },
  },
  {
    name: 'text-icon',
    covers: 'The /Name icon, changed after creation. The picture is the generator\'s, baked from /C and /Name.',
    draft: { subtype: 'text', rect: NOTE_RECT },
    patch: { subtype: 'text', icon: 'help', color: { r: 0, g: 160, b: 255 } },
  },
  {
    name: 'caret-clear-rect-differences',
    covers: 'A tri-state clear of the inset of the drawn caret from its box.',
    draft: {
      subtype: 'caret',
      rect: CARET_RECT,
      rectDifferences: { left: 2, bottom: 3, right: 4, top: 5 },
    },
    patch: { subtype: 'caret', rectDifferences: null },
  },
  {
    name: 'caret-rotate-existing-box',
    covers:
      'A rotation with no unrotatedRect in the patch, which a DRAFT refuses. The box is already on the annotation, so the pair is satisfied and this is what rotating a box a second time looks like.',
    draft: {
      subtype: 'caret',
      rect: CARET_RECT,
      rotation: 30,
      unrotatedRect: { left: 155, bottom: 535, right: 175, top: 565 },
    },
    patch: { subtype: 'caret', rotation: 60 },
  },
  {
    name: 'free-text-size-only',
    covers:
      'A /DA size with no font and no colour. /DA is one string, so the writer READS the other two rather than defaulting them: defaulting would reset a font nobody mentioned.',
    draft: {
      subtype: 'free-text',
      intent: 'free-text',
      rect: FREE_TEXT_RECT,
      fontFamily: 'times-bold',
      fontSize: 18,
      textAlign: 'center',
      color: { r: 0, g: 0, b: 200 },
    },
    patch: { subtype: 'free-text', fontSize: 9 },
  },
  {
    name: 'free-text-colour-only',
    covers:
      'A /DA colour with no font and no size, over a text box that has both. The other two are read; defaulting them would shrink the text to twelve points and reset the family.',
    draft: {
      subtype: 'free-text',
      intent: 'free-text',
      rect: FREE_TEXT_RECT,
      fontFamily: 'courier',
      fontSize: 20,
      textAlign: 'left',
    },
    patch: { subtype: 'free-text', color: { r: 200, g: 0, b: 0 } },
  },
  {
    name: 'free-text-clear-background',
    covers:
      'A tri-state clear of /C, which on a free text is the BOX BACKGROUND rather than the mark.',
    draft: {
      subtype: 'free-text',
      intent: 'free-text',
      rect: FREE_TEXT_RECT,
      fontFamily: 'helvetica',
      fontSize: 12,
      textAlign: 'left',
      interiorColor: { r: 240, g: 240, b: 200 },
    },
    patch: { subtype: 'free-text', interiorColor: null },
  },
  {
    name: 'redact-clear-label',
    covers:
      'A tri-state clear of /OverlayText. There is no call that removes the entry, so an empty string is how a label goes away.',
    draft: {
      subtype: 'redact',
      rect: REDACT_RECT,
      overlayText: 'REDACTED',
      fontSize: 9,
    },
    patch: { subtype: 'redact', overlayText: null },
  },
  {
    name: 'redact-label-colour-only',
    covers:
      'A label colour with no size, over a marking whose label is nine points. Unlike free text, a redaction writes /DA whole: the entry belongs to the label rather than to the annotation, so the nine points go back to twelve.',
    draft: {
      subtype: 'redact',
      rect: REDACT_RECT,
      overlayText: 'DRAFT',
      fontFamily: 'helvetica-bold',
      fontSize: 9,
    },
    patch: { subtype: 'redact', fontColor: { r: 255, g: 255, b: 255 } },
  },
  {
    name: 'redact-relabel',
    covers: 'A new label and a new alignment on an existing marking.',
    draft: { subtype: 'redact', rect: REDACT_RECT, overlayText: 'DRAFT' },
    patch: {
      subtype: 'redact',
      overlayText: 'FINAL',
      repeat: true,
      textAlign: 'center',
      fontColor: { r: 255, g: 255, b: 255 },
    },
  },
  {
    name: 'redact-more-quads',
    covers:
      'A text redaction gaining a region, with no rect in the patch. The box follows the quads here, unlike on a draft: a patch that moves the regions and says nothing about the box is asking for the box to follow.',
    draft: {
      subtype: 'redact',
      rect: { left: 276, bottom: 196, right: 474, top: 244 },
      quadPoints: [
        { p1: { x: 280, y: 240 }, p2: { x: 470, y: 240 }, p3: { x: 280, y: 220 }, p4: { x: 470, y: 220 } },
      ],
    },
    patch: {
      subtype: 'redact',
      quadPoints: [
        { p1: { x: 280, y: 240 }, p2: { x: 470, y: 240 }, p3: { x: 280, y: 220 }, p4: { x: 470, y: 220 } },
        { p1: { x: 280, y: 218 }, p2: { x: 400, y: 218 }, p3: { x: 280, y: 200 }, p4: { x: 400, y: 200 } },
      ],
    },
  },
  {
    name: 'redact-quads-and-rect',
    covers:
      'The other branch: a patch that supplies both. It has already decided what the box is, so the quads do not get to decide again, and the margin survives.',
    draft: {
      subtype: 'redact',
      rect: { left: 276, bottom: 196, right: 474, top: 244 },
      quadPoints: [
        { p1: { x: 280, y: 240 }, p2: { x: 470, y: 240 }, p3: { x: 280, y: 220 }, p4: { x: 470, y: 220 } },
      ],
    },
    patch: {
      subtype: 'redact',
      rect: { left: 276, bottom: 196, right: 474, top: 244 },
      quadPoints: [
        { p1: { x: 280, y: 240 }, p2: { x: 470, y: 240 }, p3: { x: 280, y: 220 }, p4: { x: 470, y: 220 } },
        { p1: { x: 280, y: 218 }, p2: { x: 400, y: 218 }, p3: { x: 280, y: 200 }, p4: { x: 400, y: 200 } },
      ],
    },
  },
  {
    name: 'link-retarget',
    covers: 'A link pointed somewhere else. The action is replaced rather than merged.',
    draft: {
      subtype: 'link',
      rect: LINK_RECT,
      target: { kind: 'uri', uri: 'https://example.invalid/old' },
    },
    patch: {
      subtype: 'link',
      target: { kind: 'goto', destination: { kind: 'fit', page: toPageRef(3) } },
    },
  },
  {
    name: 'link-kill-target',
    covers:
      'A dead link. Both /A and /Dest go, because removing only the action would resurrect a stale direct destination as the live target.',
    draft: {
      subtype: 'link',
      rect: LINK_RECT,
      target: { kind: 'uri', uri: 'https://example.invalid/old' },
    },
    patch: { subtype: 'link', target: null },
  },
  {
    name: 'file-attachment-icon',
    covers:
      'A marker restyled. The FILE is not patchable: bytes enter once, and replacing them is a different attachment.',
    resources: { r0: [104, 105] },
    draft: {
      subtype: 'file-attachment',
      rect: NOTE_RECT,
      file: { resource: 'r0', name: 'notes.txt' },
    },
    patch: { subtype: 'file-attachment', icon: 'graph', color: { r: 0, g: 200, b: 120 } },
  },
  {
    name: 'stamp-move-only',
    covers:
      'A stamp moved with no new bytes. The existing appearance is re-fitted into the new box, which is what makes moving one cheap.',
    resources: { r0: STAMP_PNG },
    draft: {
      subtype: 'stamp',
      rect: STAMP_RECT,
      source: { resource: 'r0' },
    },
    patch: { subtype: 'stamp', rect: { left: 200, bottom: 400, right: 360, top: 480 } },
  },
  {
    name: 'stamp-replace-source',
    covers:
      'New bytes. The existing appearance objects are dropped and the picture is authored again; painting over them would leave the old one underneath.',
    resources: { r0: STAMP_PNG, r1: STAMP_PNG },
    draft: {
      subtype: 'stamp',
      rect: STAMP_RECT,
      source: { resource: 'r0' },
    },
    patch: { subtype: 'stamp', source: { resource: 'r1' }, fit: 'fill' },
  },
  {
    name: 'widget-restyle',
    covers:
      'The widget plane, patched. The colours are /MK, and a clear is an explicit no-colour rather than the absence a create leaves.',
    draft: {
      subtype: 'widget',
      rect: WIDGET_RECT,
      color: { r: 30, g: 30, b: 120 },
      interiorColor: { r: 245, g: 245, b: 230 },
      strokeWidth: 2,
    },
    patch: { subtype: 'widget', interiorColor: null, strokeWidth: 4 },
  },
  {
    name: 'widget-font-size-only',
    covers:
      'A /DA size with no family, over a widget that HAS one. Unlike free text, the other two members are defaulted rather than read: one writer serves this and the forms placement path, so a widget style is a whole description and the times-roman goes away.',
    draft: {
      subtype: 'widget',
      rect: WIDGET_RECT,
      fontFamily: 'times-roman',
      fontSize: 11,
      fontColor: { r: 20, g: 20, b: 20 },
    },
    patch: { subtype: 'widget', fontSize: 16 },
  },
  {
    name: 'ink-intent',
    covers: 'The only /IT ink carries, set after creation.',
    draft: {
      subtype: 'ink',
      rect: { left: 72, bottom: 200, right: 240, top: 300 },
      inkList: [[{ x: 72, y: 200 }, { x: 240, y: 300 }]],
    },
    patch: { subtype: 'ink', intent: 'ink-highlight' },
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
      const page = doc.page(first.ref);

      const payloads = { ...(entry.resources ?? {}) };
      const created = await page.annotations.create(
        materialize(entry.draft, payloads) as never,
      );
      const result = await page.annotations.update(
        created.created.ref,
        materialize(entry.patch, payloads) as never,
      );

      const { modified, ...updated } = result.updated as Record<string, unknown>;
      return {
        ...entry,
        pageObjectNumber: first.ref.pageObjectNumber,
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
      key === 'docSessionId'
        ? [key, '<session>']
        : // PDFium stamps an attachment's creation date from the clock when
          // the bytes are written, so the two engines record different
          // instants for the same file. Its PRESENCE is asserted by the
          // Rust runner instead.
          key === 'creationDate' && typeof entry === 'string'
          ? [key, '<stamped>']
          : [key, normalizeSessionIds(entry)],
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
