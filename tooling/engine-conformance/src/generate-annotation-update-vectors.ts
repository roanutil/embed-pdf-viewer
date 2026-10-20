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
import type {
  AnnotationDraft,
  AnnotationPatch,
} from '../../../packages/engine/core/src/annotation/kinds';
import type {
  AnnotationResourceRole,
  AnnotationResources,
} from '../../../packages/engine/core/src/annotation/resources';

import { normalizeWriteStamps } from './annotation-write-stamps';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');
const vectorsRoot = join(packageRoot, 'vectors', 'annotation');

/** Bump when the envelope changes shape. The Rust runner asserts it. */
const SCHEMA_VERSION = 1;

const FIXTURE = 'tooling/engine-conformance/fixtures/rich_metadata.pdf';

/*
 * The drafts and patches are what the engine takes since upstream's Engine
 * API v3, the request a replay sends as it is: page space from the page's
 * top-left (1a243431a), colours as '#rrggbb' (c053c3d33), named quad corners
 * (00d2c8760), top-level flags (c78997346), `box` and a clockwise `rotation`
 * for the box kinds with their `rect` the engine's (2bdc74fbf), no `rect` on
 * the point and markup kinds (6153c181e), and destinations naming their page
 * by ref. Each case was ported from the pre-v3 spelling to describe the same
 * annotation and the same edit; `polygon-degenerate` and `line-endpoints`
 * lost the `rect` their patches moved, which the engine now derives. Page 3
 * of the fixture is 612 by 792 with its crop box at the origin. The
 * commented-out cases below are in the pre-v3 spelling they were written in.
 */
const QUAD = {
  upperLeft: { x: 72, y: 72 },
  upperRight: { x: 200, y: 72 },
  lowerLeft: { x: 72, y: 92 },
  lowerRight: { x: 200, y: 92 },
};
const SHIFTED_QUAD = {
  upperLeft: { x: 102, y: 92 },
  upperRight: { x: 230, y: 92 },
  lowerLeft: { x: 102, y: 112 },
  lowerRight: { x: 230, y: 112 },
};
const SECOND_QUAD = {
  upperLeft: { x: 72, y: 102 },
  upperRight: { x: 160, y: 102 },
  lowerLeft: { x: 72, y: 122 },
  lowerRight: { x: 160, y: 122 },
};

type Resources = Readonly<Partial<Record<AnnotationResourceRole, readonly number[]>>>;

interface UpdateCase {
  readonly name: string;
  readonly covers: string;
  readonly draft: Record<string, unknown>;
  readonly patch: Record<string, unknown>;
  /** Bytes the create carries beside the draft, by role. See the create generator. */
  readonly draftResources?: Resources;
  /** Bytes the update carries beside the patch, by role. */
  readonly patchResources?: Resources;
  /** The engine refuses this patch; the case records the error code. */
  readonly refused?: true;
}

/** A case's resources as the bytes the write option takes. */
function resourcesOf(
  resources: Resources | undefined,
): { resources: AnnotationResources } | Record<string, never> {
  if (!resources) return {};
  return {
    resources: Object.fromEntries(
      Object.entries(resources).map(([role, bytes]) => [role, Uint8Array.from(bytes!)]),
    ),
  };
}

/** A tiny 4x2 PNG, for the stamp cases. */
const STAMP_PNG = [137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 2, 8, 2, 0, 0, 0, 240, 202, 234, 52, 0, 0, 0, 16, 73, 68, 65, 84, 120, 156, 99, 184, 163, 161, 1, 71, 12, 200, 28, 0, 127, 154, 9, 97, 122, 180, 22, 59, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130];

const CASES: readonly UpdateCase[] = [
  {
    name: 'inert-patch',
    covers:
      'A patch that repeats what is already there. Nothing changed value, so the appearance is preserved and a client is told not to re-raster.',
    draft: { subtype: 'highlight', quadPoints: [QUAD], color: '#ffff00' },
    patch: { subtype: 'highlight', color: '#ffff00' },
  },
  {
    name: 'metadata-only',
    covers:
      'Contents and flags, neither of which is painted on a highlight. Still inert, which is what stops editing a comment from destroying a shape.',
    draft: { subtype: 'highlight', quadPoints: [QUAD] },
    patch: { subtype: 'highlight', contents: 'a note', hidden: true },
  },
  {
    name: 'colour-change',
    covers: 'A semantic edit. The appearance is re-baked and the client is told so.',
    draft: { subtype: 'highlight', quadPoints: [QUAD] },
    patch: { subtype: 'highlight', color: '#0000ff' },
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
    draft: { subtype: 'highlight', quadPoints: [QUAD], print: true },
    patch: { subtype: 'highlight', hidden: true },
  },
  {
    name: 'square-rect-translation',
    covers:
      'A shape moved without resizing, which DOES preserve: the patch carries the rect, so the translation can be verified. Read beside quad-translation, which cannot be.',
    draft: { subtype: 'square', box: { x: 300, y: 132, width: 100, height: 60 } },
    patch: { subtype: 'square', box: { x: 330, y: 147, width: 100, height: 60 } },
  },
  {
    name: 'square-resize',
    covers: 'A shape resized. The pixels would be scaled rather than shifted, so it re-bakes.',
    draft: { subtype: 'square', box: { x: 300, y: 132, width: 100, height: 60 } },
    patch: { subtype: 'square', box: { x: 300, y: 132, width: 200, height: 60 } },
  },
  {
    name: 'square-border-width-only',
    covers:
      'A width with no style. /BS is one native call taking both, so the style has to be read rather than defaulted, or a patch that never mentioned it would reset it.',
    draft: {
      subtype: 'square',
      box: { x: 300, y: 132, width: 100, height: 60 },
      borderStyle: 'dashed',
      dashArray: [4, 2],
    },
    patch: { subtype: 'square', strokeWidth: 5 },
  },
  {
    name: 'square-clear-interior',
    covers: 'A tri-state clear of a fill that was there.',
    draft: {
      subtype: 'square',
      box: { x: 300, y: 132, width: 100, height: 60 },
      interiorColor: '#00ff00',
    },
    patch: { subtype: 'square', interiorColor: null },
  },
  {
    name: 'square-clear-cloudy',
    covers: 'A tri-state clear of a cloudy border that was there.',
    draft: {
      subtype: 'square',
      box: { x: 300, y: 132, width: 100, height: 60 },
      cloudyIntensity: 2,
    },
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
      vertices: [{ x: 300, y: 492 }, { x: 420, y: 492 }, { x: 420, y: 412 }, { x: 300, y: 412 }],
    },
    patch: {
      subtype: 'polygon',
      vertices: [{ x: 300, y: 492 }, { x: 420, y: 492 }, { x: 360, y: 412 }],
    },
  },
  {
    name: 'polygon-degenerate',
    covers:
      'A polygon flattened onto a line. PDFium has no appearance to bake for one, so the verdict is a third thing: the edit landed and the stream could not be rebuilt. A client is told rather than shown a stale picture.',
    draft: {
      subtype: 'polygon',
      vertices: [{ x: 300, y: 492 }, { x: 420, y: 492 }, { x: 360, y: 412 }],
    },
    patch: { subtype: 'polygon', vertices: [{ x: 300, y: 492 }, { x: 420, y: 492 }] },
    // Refused since 404b79b3e ("Engine API v3 drawing details": too few
    // points to draw is InvalidArg); the case records the refusal code.
    refused: true,
  },
  {
    name: 'polygon-clear-cloudy',
    covers: 'A polygon is the one vertex kind with a /BE, and it clears on the same terms a shape does.',
    draft: {
      subtype: 'polygon',
      vertices: [{ x: 300, y: 492 }, { x: 420, y: 492 }, { x: 360, y: 412 }],
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
      vertices: [{ x: 300, y: 492 }, { x: 420, y: 492 }, { x: 360, y: 412 }],
      interiorColor: '#00ff00',
    },
    patch: { subtype: 'polygon', interiorColor: null },
  },
  {
    name: 'polygon-width-only',
    covers:
      'A border width with no style. /BS takes both in one call, so the writer has to READ the style rather than default it: defaulting would reset a dashed border nobody mentioned.',
    draft: {
      subtype: 'polygon',
      vertices: [{ x: 300, y: 492 }, { x: 420, y: 492 }, { x: 360, y: 412 }],
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
      vertices: [{ x: 300, y: 492 }, { x: 420, y: 492 }, { x: 360, y: 412 }],
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
      vertices: [{ x: 300, y: 492 }, { x: 420, y: 492 }, { x: 360, y: 412 }],
      rotation: 330,
    },
    patch: { subtype: 'polygon', rotation: null },
  },
  {
    name: 'polyline-endings',
    covers: 'The /LE pair, changed after creation. The two ends differ, so a writer that wrote one twice is visible.',
    draft: {
      subtype: 'polyline',
      vertices: [{ x: 300, y: 492 }, { x: 420, y: 492 }, { x: 360, y: 412 }],
      lineEndings: { start: 'none', end: 'none' },
    },
    patch: { subtype: 'polyline', lineEndings: { start: 'diamond', end: 'open-arrow' } },
  },
  {
    name: 'line-endpoints',
    covers: 'Moving a line is moving its two points, and its /Rect is the caller arithmetic over them.',
    draft: { subtype: 'line', linePoints: { start: { x: 450, y: 492 }, end: { x: 560, y: 392 } } },
    patch: { subtype: 'line', linePoints: { start: { x: 450, y: 452 }, end: { x: 520, y: 492 } } },
  },
  {
    name: 'ink-replace-strokes',
    covers:
      'A new pen path. There is no way to replace one stroke, so the whole list goes and the new one is appended; half of that leaves the old strokes under the new ones.',
    draft: {
      subtype: 'ink',
      inkList: [
        [{ x: 72, y: 592 }, { x: 120, y: 512 }, { x: 160, y: 592 }],
        [{ x: 180, y: 572 }, { x: 240, y: 492 }],
      ],
    },
    patch: { subtype: 'ink', inkList: [[{ x: 80, y: 582 }, { x: 130, y: 532 }]] },
  },
  {
    name: 'text-state-alone',
    covers:
      'A /State with no /StateModel, which a DRAFT refuses. A patch touches what it states and the model is already on the annotation, so the rule is draft-only.',
    draft: {
      subtype: 'text',
      rect: { x: 72, y: 172, width: 20, height: 20 },
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
      rect: { x: 72, y: 172, width: 20, height: 20 },
      stateModel: 'review',
      state: 'accepted',
    },
    patch: { subtype: 'text', state: null },
  },
  {
    name: 'text-icon',
    covers: 'The /Name icon, changed after creation. The picture is the generator\'s, baked from /C and /Name.',
    draft: { subtype: 'text', rect: { x: 72, y: 172, width: 20, height: 20 } },
    patch: { subtype: 'text', icon: 'help', color: '#00a0ff' },
  },
  // `rectDifferences` is gone from every kind since 2bdc74fbf ("round 5b:
  // clockwise rotation, box and the engine's rect"): a box kind's shape is
  // its `box`, and the engine draws inside it. This case was only about
  // clearing that inset, so it stays here, commented out.
  // {
  //   name: 'caret-clear-rect-differences',
  //   covers: 'A tri-state clear of the inset of the drawn caret from its box.',
  //   draft: {
  //     subtype: 'caret',
  //     rect: CARET_RECT,
  //     rectDifferences: { left: 2, bottom: 3, right: 4, top: 5 },
  //   },
  //   patch: { subtype: 'caret', rectDifferences: null },
  // },
  {
    name: 'caret-rotate-existing-box',
    covers:
      'A rotation with no unrotatedRect in the patch, which a DRAFT refuses. The box is already on the annotation, so the pair is satisfied and this is what rotating a box a second time looks like.',
    draft: { subtype: 'caret', box: { x: 155, y: 227, width: 20, height: 30 }, rotation: 330 },
    patch: { subtype: 'caret', rotation: 300 },
  },
  {
    name: 'free-text-size-only',
    covers:
      'A /DA size with no font and no colour. /DA is one string, so the writer READS the other two rather than defaulting them: defaulting would reset a font nobody mentioned.',
    draft: {
      subtype: 'free-text',
      intent: 'free-text',
      box: { x: 72, y: 322, width: 188, height: 70 },
      fontFamily: 'times-bold',
      fontSize: 18,
      textAlign: 'center',
      color: '#0000c8',
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
      box: { x: 72, y: 322, width: 188, height: 70 },
      fontFamily: 'courier',
      fontSize: 20,
      textAlign: 'left',
    },
    patch: { subtype: 'free-text', color: '#c80000' },
  },
  {
    name: 'free-text-clear-background',
    covers:
      'A tri-state clear of /C, which on a free text is the BOX BACKGROUND rather than the mark.',
    draft: {
      subtype: 'free-text',
      intent: 'free-text',
      box: { x: 72, y: 322, width: 188, height: 70 },
      fontFamily: 'helvetica',
      fontSize: 12,
      textAlign: 'left',
      interiorColor: '#f0f0c8',
    },
    patch: { subtype: 'free-text', interiorColor: null },
  },
  {
    name: 'redact-clear-label',
    covers:
      'A tri-state clear of /OverlayText. There is no call that removes the entry, so an empty string is how a label goes away.',
    draft: {
      subtype: 'redact',
      rect: { x: 72, y: 552, width: 188, height: 40 },
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
      rect: { x: 72, y: 552, width: 188, height: 40 },
      overlayText: 'DRAFT',
      fontFamily: 'helvetica-bold',
      fontSize: 9,
    },
    patch: { subtype: 'redact', fontColor: '#ffffff' },
  },
  {
    name: 'redact-relabel',
    covers: 'A new label and a new alignment on an existing marking.',
    draft: {
      subtype: 'redact',
      rect: { x: 72, y: 552, width: 188, height: 40 },
      overlayText: 'DRAFT',
    },
    patch: {
      subtype: 'redact',
      overlayText: 'FINAL',
      repeat: true,
      textAlign: 'center',
      fontColor: '#ffffff',
    },
  },
  {
    name: 'redact-more-quads',
    covers:
      'A text redaction gaining a region, with no rect in the patch. The box follows the quads here, unlike on a draft: a patch that moves the regions and says nothing about the box is asking for the box to follow.',
    draft: {
      subtype: 'redact',
      rect: { x: 276, y: 548, width: 198, height: 48 },
      quadPoints: [
        {
          upperLeft: { x: 280, y: 552 },
          upperRight: { x: 470, y: 552 },
          lowerLeft: { x: 280, y: 572 },
          lowerRight: { x: 470, y: 572 },
        },
      ],
    },
    patch: {
      subtype: 'redact',
      quadPoints: [
        {
          upperLeft: { x: 280, y: 552 },
          upperRight: { x: 470, y: 552 },
          lowerLeft: { x: 280, y: 572 },
          lowerRight: { x: 470, y: 572 },
        },
        {
          upperLeft: { x: 280, y: 574 },
          upperRight: { x: 400, y: 574 },
          lowerLeft: { x: 280, y: 592 },
          lowerRight: { x: 400, y: 592 },
        },
      ],
    },
  },
  {
    name: 'redact-quads-and-rect',
    covers:
      'The other branch: a patch that supplies both. It has already decided what the box is, so the quads do not get to decide again, and the margin survives.',
    draft: {
      subtype: 'redact',
      rect: { x: 276, y: 548, width: 198, height: 48 },
      quadPoints: [
        {
          upperLeft: { x: 280, y: 552 },
          upperRight: { x: 470, y: 552 },
          lowerLeft: { x: 280, y: 572 },
          lowerRight: { x: 470, y: 572 },
        },
      ],
    },
    patch: {
      subtype: 'redact',
      rect: { x: 276, y: 548, width: 198, height: 48 },
      quadPoints: [
        {
          upperLeft: { x: 280, y: 552 },
          upperRight: { x: 470, y: 552 },
          lowerLeft: { x: 280, y: 572 },
          lowerRight: { x: 470, y: 572 },
        },
        {
          upperLeft: { x: 280, y: 574 },
          upperRight: { x: 400, y: 574 },
          lowerLeft: { x: 280, y: 592 },
          lowerRight: { x: 400, y: 592 },
        },
      ],
    },
  },
  {
    name: 'link-retarget',
    covers: 'A link pointed somewhere else. The action is replaced rather than merged.',
    draft: {
      subtype: 'link',
      rect: { x: 72, y: 132, width: 128, height: 20 },
      target: { kind: 'uri', uri: 'https://example.invalid/old' },
    },
    patch: {
      subtype: 'link',
      target: {
        kind: 'goto',
        destination: { kind: 'fit', page: { kind: 'objectNumber', objectNumber: 3 } },
      },
    },
  },
  {
    name: 'link-kill-target',
    covers:
      'A dead link. Both /A and /Dest go, because removing only the action would resurrect a stale direct destination as the live target.',
    draft: {
      subtype: 'link',
      rect: { x: 72, y: 132, width: 128, height: 20 },
      target: { kind: 'uri', uri: 'https://example.invalid/old' },
    },
    patch: { subtype: 'link', target: null },
  },
  {
    name: 'file-attachment-icon',
    covers:
      'A marker restyled. The FILE is not patchable: bytes enter once, and replacing them is a different attachment.',
    draft: {
      subtype: 'file-attachment',
      rect: { x: 72, y: 172, width: 20, height: 20 },
      file: { name: 'notes.txt' },
    },
    draftResources: { file: [104, 105] },
    patch: { subtype: 'file-attachment', icon: 'graph', color: '#00c878' },
  },
  {
    name: 'stamp-move-only',
    covers:
      'A stamp moved with no new bytes. The existing appearance is re-fitted into the new box, which is what makes moving one cheap.',
    draft: { subtype: 'stamp', box: { x: 72, y: 312, width: 80, height: 80 } },
    draftResources: { appearance: STAMP_PNG },
    patch: { subtype: 'stamp', box: { x: 200, y: 312, width: 160, height: 80 } },
  },
  {
    name: 'stamp-replace-source',
    covers:
      'New bytes. The existing appearance objects are dropped and the picture is authored again; painting over them would leave the old one underneath.',
    draft: { subtype: 'stamp', box: { x: 72, y: 312, width: 80, height: 80 } },
    draftResources: { appearance: STAMP_PNG },
    patch: { subtype: 'stamp', fit: 'fill' },
    patchResources: { appearance: STAMP_PNG },
  },
  // The annotation verbs refuse a widget since 1233d1349 ("Read and write
  // widgets through the form"): a widget is created onto a field through
  // the form and restyled by `doc.forms.updateWidget`. These two cases made
  // and patched a bare widget through the annotation verbs, so they stay
  // here, commented out.
  // {
  //   name: 'widget-restyle',
  //   covers:
  //     'The widget plane, patched. The colours are /MK, and a clear is an explicit no-colour rather than the absence a create leaves.',
  //   draft: {
  //     subtype: 'widget',
  //     rect: WIDGET_RECT,
  //     color: { r: 30, g: 30, b: 120 },
  //     interiorColor: { r: 245, g: 245, b: 230 },
  //     strokeWidth: 2,
  //   },
  //   patch: { subtype: 'widget', interiorColor: null, strokeWidth: 4 },
  // },
  // {
  //   name: 'widget-font-size-only',
  //   covers:
  //     'A /DA size with no family, over a widget that HAS one. Unlike free text, the other two members are defaulted rather than read: one writer serves this and the forms placement path, so a widget style is a whole description and the times-roman goes away.',
  //   draft: {
  //     subtype: 'widget',
  //     rect: WIDGET_RECT,
  //     fontFamily: 'times-roman',
  //     fontSize: 11,
  //     fontColor: { r: 20, g: 20, b: 20 },
  //   },
  //   patch: { subtype: 'widget', fontSize: 16 },
  // },
  {
    name: 'ink-intent',
    covers: 'The only /IT ink carries, set after creation.',
    draft: { subtype: 'ink', inkList: [[{ x: 72, y: 592 }, { x: 240, y: 492 }]] },
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
      // Pages are addressed by `PageRef` (9f13cea3c, before this catch-up).
      const page = doc.page(first.ref);
      const created = await page.annotations.create(
        entry.draft as unknown as AnnotationDraft,
        resourcesOf(entry.draftResources),
      );
      let result;
      try {
        result = await page.annotations.update(
          created.annotation.ref,
          entry.patch as unknown as AnnotationPatch,
          resourcesOf(entry.patchResources),
        );
      } catch (error) {
        // Only the code is recorded: the engines word refusals differently.
        if (!entry.refused) throw error;
        const code = String((error as { code?: unknown })?.code ?? 'unknown');
        return { ...entry, pageObjectNumber: first.ref.objectNumber, refusedWith: code };
      }
      if (entry.refused) throw new Error(`${entry.name}: expected a refusal`);

      // The results name the annotation `annotation` and the timestamp
      // `modifiedAt` since 83f6598d2 and fd175ba50; `weakRefsInvalidated` and
      // `shouldRefetch` went in bc0057f3a ("Remove NM and implement BaseID").
      const { modifiedAt, ...updated } = result.annotation as unknown as Record<string, unknown>;
      return {
        ...entry,
        pageObjectNumber: first.ref.objectNumber,
        updated,
        hasModified: typeof modifiedAt === 'string' && modifiedAt.length > 0,
        appearance: result.appearance,
        meta: {
          changed: result.meta.changed,
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
          : normalizeWriteStamps(key, entry) !== undefined
            ? [key, normalizeWriteStamps(key, entry)]
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
