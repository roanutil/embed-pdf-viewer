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
import type { AnnotationDraft } from '../../../packages/engine/core/src/annotation/kinds';
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

/** Every draft is created on this page of this fixture. */
const FIXTURE = 'tooling/engine-conformance/fixtures/rich_metadata.pdf';

/*
 * The drafts are what the engine takes since upstream's Engine API v3, the
 * request a replay sends as it is: page space from the page's top-left
 * (1a243431a), colours as '#rrggbb' (c053c3d33), named quad corners
 * (00d2c8760), top-level flags (c78997346), `box` and a clockwise `rotation`
 * for the box kinds with their `rect` the engine's (2bdc74fbf), no `rect`
 * on the point and markup kinds (6153c181e), and destinations naming their
 * page by ref. Each case was ported from the pre-v3 spelling to describe the
 * same annotation; `rectDifferences` has no counterpart (2bdc74fbf) and was
 * dropped from `square-decorated`, `caret-replace` and
 * `free-text-three-colours`. Page 3 of the fixture is 612 by 792 with its
 * crop box at the origin.
 */
const QUAD = {
  upperLeft: { x: 72, y: 72 },
  upperRight: { x: 200, y: 72 },
  lowerLeft: { x: 72, y: 92 },
  lowerRight: { x: 200, y: 92 },
};
const SECOND_QUAD = {
  upperLeft: { x: 72, y: 102 },
  upperRight: { x: 160, y: 102 },
  lowerLeft: { x: 72, y: 122 },
  lowerRight: { x: 160, y: 122 },
};

interface CreateCase {
  readonly name: string;
  readonly covers: string;
  readonly draft: Record<string, unknown>;
  /**
   * Bytes the create carries beside the draft, by resource role
   * (`appearance` for a stamp's image, `file` for an attachment), as the
   * write's `resources` option takes them since 11a1259d9 ("Annotation bytes
   * travel beside the data"). Recorded as number arrays for the Rust runner
   * to rebuild.
   */
  readonly resources?: Readonly<Partial<Record<AnnotationResourceRole, readonly number[]>>>;
  /** The engine refuses this draft; the case records the error code. */
  readonly refused?: true;
}

/** A case's resources as the bytes the write option takes. */
function resourcesOf(
  resources: CreateCase['resources'],
): { resources: AnnotationResources } | Record<string, never> {
  if (!resources) return {};
  return {
    resources: Object.fromEntries(
      Object.entries(resources).map(([role, bytes]) => [role, Uint8Array.from(bytes!)]),
    ),
  };
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
      color: '#0080ff',
      opacity: 0.4,
      contents: 'a note',
      subject: 'a subject',
      nm: 'caller-chosen-id',
      print: true,
      noZoom: true,
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
    draft: { subtype: 'square', box: { x: 300, y: 132, width: 100, height: 60 } },
  },
  {
    name: 'square-decorated',
    covers:
      'Every optional shape entry: a dashed border with a pattern, a fill, a cloudy intensity and an /RD inset.',
    draft: {
      subtype: 'square',
      box: { x: 300, y: 212, width: 120, height: 80 },
      color: '#ff0000',
      interiorColor: '#00ff00',
      opacity: 0.8,
      strokeWidth: 3,
      borderStyle: 'dashed',
      dashArray: [4, 2],
      cloudyIntensity: 2,
    },
  },
  {
    name: 'square-degenerate-cloudy',
    covers:
      'A cloudy intensity of zero. The wire domain is positive-or-null, so the writer has to clear rather than write a value the reader would then normalize away.',
    draft: {
      subtype: 'square',
      box: { x: 300, y: 332, width: 100, height: 60 },
      cloudyIntensity: 0,
    },
    // Refused since c78997346 declared a cloudy intensity positive; the
    // case records the refusal code.
    refused: true,
  },
  {
    name: 'square-rotated',
    covers:
      'A rotated box. `/Rect` is the enclosing axis-aligned box of the turned square; the pair under /EMBD_Metadata is what makes the generator bake a turned appearance rather than an upright one.',
    draft: {
      subtype: 'square',
      box: { x: 110, y: 302, width: 80, height: 80 },
      rotation: 315,
      color: '#008000',
      strokeWidth: 3,
    },
  },
  {
    name: 'square-zero-rotation',
    covers:
      'A box whose logical box is its rect and whose rotation is zero. Zero degrees clears the key rather than writing it, so the annotation carries a box and no angle.',
    draft: { subtype: 'square', box: { x: 240, y: 322, width: 100, height: 80 }, rotation: 0 },
  },
  {
    name: 'square-zero-rotation-no-box',
    covers:
      'Zero degrees with no logical box, which is not the unsatisfiable pair. Nothing is being rotated, so there is nothing for a box to be the pre-rotation form of.',
    draft: { subtype: 'square', box: { x: 350, y: 322, width: 80, height: 80 }, rotation: 0 },
  },
  {
    name: 'polygon',
    covers:
      'A closed vertex path with a fill and a cloudy border. Only a polygon takes /BE: an open path has no interior for one to enclose.',
    draft: {
      subtype: 'polygon',
      vertices: [{ x: 300, y: 492 }, { x: 420, y: 492 }, { x: 360, y: 412 }],
      color: '#0000ff',
      interiorColor: '#ffff00',
      cloudyIntensity: 1,
    },
  },
  {
    name: 'polyline-with-endings',
    covers:
      'An open vertex path with a /LE pair, which the polygon family does not have. The two endings differ, so a writer that wrote one twice is visible.',
    draft: {
      subtype: 'polyline',
      vertices: [{ x: 300, y: 592 }, { x: 360, y: 512 }, { x: 420, y: 592 }],
      lineEndings: { start: 'open-arrow', end: 'diamond' },
    },
  },
  {
    name: 'polyline-defaults',
    covers:
      'The same without a /LE, so the pair that draws nothing is what a reader gets back rather than an absent entry it has to interpret.',
    draft: { subtype: 'polyline', vertices: [{ x: 450, y: 592 }, { x: 560, y: 512 }] },
  },
  {
    name: 'line',
    covers: 'Two endpoints and a pair of endings, which is the whole of a line annotation.',
    draft: {
      subtype: 'line',
      linePoints: { start: { x: 450, y: 492 }, end: { x: 560, y: 392 } },
      lineEndings: { start: 'circle', end: 'closed-arrow' },
      color: '#ff00ff',
      strokeWidth: 2,
    },
  },
  {
    name: 'ink-two-strokes',
    covers:
      'Two pen strokes, so a writer that appended only the first is visible, with the highlighter intent that is the only /IT ink carries.',
    draft: {
      subtype: 'ink',
      inkList: [
        [{ x: 72, y: 592 }, { x: 120, y: 512 }, { x: 160, y: 592 }],
        [{ x: 180, y: 572 }, { x: 240, y: 492 }],
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
      vertices: [{ x: 450, y: 342 }, { x: 560, y: 342 }, { x: 505, y: 252 }],
      cloudyIntensity: 0,
    },
    // Refused since c78997346 declared a cloudy intensity positive; the
    // case records the refusal code.
    refused: true,
  },
  {
    name: 'polyline-zero-rotation',
    covers:
      'A rotation of zero. Zero degrees is not a rotation, so the key is cleared rather than stamped, and a file that had none keeps none.',
    draft: {
      subtype: 'polyline',
      vertices: [{ x: 300, y: 342 }, { x: 420, y: 252 }],
      rotation: 0,
    },
  },
  {
    name: 'ink-advisory-rotation',
    covers:
      'A rotation scalar on a vertex kind. The strokes are already rotated, so this records the angle and never drives the generator; it also never carries a logical box, which is what separates it from the shape family.',
    draft: { subtype: 'ink', inkList: [[{ x: 72, y: 692 }, { x: 240, y: 612 }]], rotation: 315 },
  },
  {
    name: 'text-defaults',
    covers:
      'A sticky note with nothing named. The icon and the yellow fill are written explicitly rather than left to the file, so a read reports the same thing a draft that named them would.',
    draft: { subtype: 'text', rect: { x: 72, y: 172, width: 20, height: 20 } },
  },
  {
    name: 'text-reviewed',
    covers:
      'An icon other than the default, and the review pair. `/State` and `/StateModel` are dictionary-only, so a note that has been accepted is drawn exactly like one that has not.',
    draft: {
      subtype: 'text',
      rect: { x: 110, y: 172, width: 20, height: 20 },
      icon: 'help',
      color: '#00a0ff',
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
      rect: { x: 150, y: 172, width: 20, height: 20 },
      stateModel: 'marked',
    },
  },
  {
    name: 'text-foreign-state',
    covers:
      'A model nobody standardized. Acrobat lets a workflow register one, and the reader passes it through, so the writer has to be able to write it back rather than normalize it into a standard name.',
    draft: {
      subtype: 'text',
      rect: { x: 190, y: 172, width: 20, height: 20 },
      stateModel: 'LegalReview',
      state: 'AwaitingCounsel',
    },
  },
  {
    name: 'caret-defaults',
    covers: 'A caret with only a rect. The red mark is the engine-wide default.',
    draft: { subtype: 'caret', box: { x: 72, y: 232, width: 12, height: 20 } },
  },
  {
    name: 'caret-replace',
    covers:
      'The caret half of a replace-text edit, with the inset of the drawn symbol from its box.',
    draft: {
      subtype: 'caret',
      box: { x: 110, y: 222, width: 20, height: 30 },
      intent: 'replace',
      color: '#0000ff',
      opacity: 0.5,
    },
  },
  {
    name: 'caret-rotated',
    covers:
      'A caret is a BOX kind, so its rotation is the pair and not the lone advisory scalar the vertex families carry.',
    draft: { subtype: 'caret', box: { x: 155, y: 227, width: 20, height: 30 }, rotation: 270 },
  },
  {
    name: 'free-text-plain',
    covers:
      'A plain text box. The three colours are three different entries: /DA draws the border and the text, /C is the background, and a TextColor override is absent here so the text follows /DA.',
    draft: {
      subtype: 'free-text',
      intent: 'free-text',
      box: { x: 72, y: 322, width: 188, height: 70 },
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
      box: { x: 280, y: 322, width: 190, height: 70 },
      fontFamily: 'times-bold',
      fontSize: 18,
      textAlign: 'center',
      color: '#0000c8',
      fontColor: '#c80000',
      interiorColor: '#f0f0c8',
      opacity: 0.9,
      strokeWidth: 2,
      borderStyle: 'dashed',
      dashArray: [4, 2],
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
      box: { x: 300, y: 432, width: 170, height: 60 },
      fontFamily: 'helvetica-oblique',
      fontSize: 11,
      textAlign: 'right',
      calloutLine: [{ x: 200, y: 542 }, { x: 260, y: 492 }, { x: 300, y: 462 }],
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
      box: { x: 80, y: 422, width: 100, height: 60 },
      fontFamily: 'courier',
      fontSize: 10,
      textAlign: 'left',
      rotation: 345,
    },
  },
  {
    name: 'redact-area',
    covers:
      'An area redaction: no quads, so /Rect is the region. Nothing here removes anything; the marking declares what apply would do.',
    draft: { subtype: 'redact', rect: { x: 72, y: 552, width: 188, height: 40 } },
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
      color: '#ff8000',
      interiorColor: '#000000',
      opacity: 0.6,
      overlayText: 'REDACTED',
      repeat: true,
      fontFamily: 'helvetica-bold',
      fontSize: 9,
      fontColor: '#ffffff',
      textAlign: 'center',
    },
  },
  {
    name: 'redact-autofit-label',
    covers:
      'A label at size zero, which for a redaction means auto-fit to the region rather than nothing. Free text refuses the same number.',
    draft: {
      subtype: 'redact',
      rect: { x: 72, y: 602, width: 188, height: 40 },
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
      rect: { x: 280, y: 602, width: 190, height: 40 },
      overlayText: '',
    },
  },
  {
    name: 'file-attachment',
    covers:
      'A marker and the file it names. PDFium derives the size, the checksum and the creation date from the bytes, so the read back proves the stream was written and not just the filespec.',
    resources: {
      file: [116, 104, 101, 32, 113, 117, 105, 99, 107, 32, 98, 114, 111, 119, 110, 32, 102, 111, 120, 10],
    },
    draft: {
      subtype: 'file-attachment',
      rect: { x: 72, y: 672, width: 20, height: 20 },
      file: { name: 'notes.txt', mimeType: 'text/plain', description: 'A short note.' },
      icon: 'tag',
      color: '#00c878',
    },
  },
  {
    name: 'file-attachment-defaults',
    covers:
      'The writer default icon is the paperclip, which is NOT what an absent /Name reads as. The key is always written, so the two never disagree about one file.',
    resources: { file: [] },
    draft: {
      subtype: 'file-attachment',
      rect: { x: 110, y: 672, width: 20, height: 20 },
      file: { name: 'empty.bin' },
    },
  },
  {
    name: 'link-dead',
    covers:
      'A link with nowhere to go. This is the create-then-edit flow: the rectangle is dragged first and the destination chosen afterwards, so a target of null is a legitimate state rather than a mistake.',
    draft: { subtype: 'link', rect: { x: 72, y: 132, width: 128, height: 20 }, target: null },
  },
  {
    name: 'link-uri',
    covers: 'A /URI action. The reader normalizes /Dest and /A /GoTo together, so this is the one target shape it cannot confuse with the other.',
    draft: {
      subtype: 'link',
      rect: { x: 210, y: 132, width: 130, height: 20 },
      target: { kind: 'uri', uri: 'https://example.invalid/a?b=c#d' },
    },
  },
  {
    name: 'link-goto-xyz',
    covers:
      'An /XYZ destination with a left and a top and no zoom. Each component is present or retains the current value, so the absent zoom has to come back absent rather than as a zero magnification.',
    draft: {
      subtype: 'link',
      rect: { x: 350, y: 132, width: 120, height: 20 },
      target: {
        kind: 'goto',
        destination: {
          kind: 'xyz',
          page: { kind: 'objectNumber', objectNumber: 3 },
          zoom: null,
          x: 100,
          y: 92,
        },
      },
    },
  },
  {
    name: 'link-goto-fit',
    covers: 'The parameterless fit, which writes an empty array rather than a padded one.',
    draft: {
      subtype: 'link',
      rect: { x: 72, y: 162, width: 128, height: 20 },
      target: {
        kind: 'goto',
        destination: { kind: 'fit', page: { kind: 'objectNumber', objectNumber: 3 } },
      },
    },
  },
  {
    name: 'link-goto-fit-h',
    covers:
      'One positional parameter. A fit type takes its parameters by position, so an absent one can only be absent by shortening the array.',
    draft: {
      subtype: 'link',
      rect: { x: 210, y: 162, width: 130, height: 20 },
      target: {
        kind: 'goto',
        destination: { kind: 'fitH', page: { kind: 'objectNumber', objectNumber: 3 }, y: 292 },
      },
    },
  },
  {
    name: 'link-goto-xyz-retain',
    covers:
      'An /XYZ whose left retains the current value. Zero is a real coordinate at the left edge, so an absent left cannot be written as one.',
    draft: {
      subtype: 'link',
      rect: { x: 72, y: 192, width: 128, height: 20 },
      target: {
        kind: 'goto',
        destination: {
          kind: 'xyz',
          page: { kind: 'objectNumber', objectNumber: 3 },
          zoom: 2,
          x: null,
          y: 392,
        },
      },
    },
  },
  {
    name: 'link-goto-fit-h-retain',
    covers:
      'A /FitH with no top. The parameter is positional, so absent means a shorter array rather than a zero at the bottom of the page.',
    draft: {
      subtype: 'link',
      rect: { x: 210, y: 192, width: 130, height: 20 },
      target: {
        kind: 'goto',
        destination: { kind: 'fitH', page: { kind: 'objectNumber', objectNumber: 3 }, y: null },
      },
    },
  },
  {
    name: 'link-goto-fit-v',
    covers:
      'The vertical twin of /FitH. It takes a left where the other takes a top, and they are different view codes, so a writer with one code for both agrees with one of them.',
    draft: {
      subtype: 'link',
      rect: { x: 350, y: 192, width: 120, height: 20 },
      target: {
        kind: 'goto',
        destination: { kind: 'fitV', page: { kind: 'objectNumber', objectNumber: 3 }, x: 120 },
      },
    },
  },
  {
    name: 'link-goto-fit-bv',
    covers:
      'The bounding-box variants are a separate pair of codes again, fitting the content box rather than the crop box.',
    draft: {
      subtype: 'link',
      rect: { x: 72, y: 222, width: 128, height: 20 },
      target: {
        kind: 'goto',
        destination: { kind: 'fitBV', page: { kind: 'objectNumber', objectNumber: 3 }, x: 90 },
      },
    },
  },
  {
    name: 'link-goto-fit-r',
    covers: 'The one fit type whose four coordinates are all required.',
    draft: {
      subtype: 'link',
      rect: { x: 350, y: 162, width: 120, height: 20 },
      target: {
        kind: 'goto',
        destination: {
          kind: 'fitR',
          page: { kind: 'objectNumber', objectNumber: 3 },
          x: 50,
          y: 192,
          width: 350,
          height: 500,
        },
      },
    },
  },
  {
    name: 'stamp-png-contain',
    covers:
      'A 4x2 image in a square box under the default fit. Aspect is preserved and the leftover space is split evenly, so a writer that pinned the image to a corner is visible in the appearance.',
    resources: {
      appearance: [137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 2, 8, 2, 0, 0, 0, 240, 202, 234, 52, 0, 0, 0, 16, 73, 68, 65, 84, 120, 156, 99, 184, 163, 161, 1, 71, 12, 200, 28, 0, 127, 154, 9, 97, 122, 180, 22, 59, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130],
    },
    draft: { subtype: 'stamp', box: { x: 72, y: 312, width: 80, height: 80 }, name: 'Approved' },
  },
  {
    name: 'stamp-png-cover',
    covers: 'The same bytes filling the box instead, which crops rather than letterboxes.',
    resources: {
      appearance: [137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 2, 8, 2, 0, 0, 0, 240, 202, 234, 52, 0, 0, 0, 16, 73, 68, 65, 84, 120, 156, 99, 184, 163, 161, 1, 71, 12, 200, 28, 0, 127, 154, 9, 97, 122, 180, 22, 59, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130],
    },
    draft: { subtype: 'stamp', box: { x: 170, y: 312, width: 80, height: 80 }, fit: 'cover' },
  },
  {
    name: 'stamp-png-fill',
    covers: 'And distorting it, which is the one policy that discards the aspect ratio.',
    resources: {
      appearance: [137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 2, 8, 2, 0, 0, 0, 240, 202, 234, 52, 0, 0, 0, 16, 73, 68, 65, 84, 120, 156, 99, 184, 163, 161, 1, 71, 12, 200, 28, 0, 127, 154, 9, 97, 122, 180, 22, 59, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130],
    },
    draft: { subtype: 'stamp', box: { x: 270, y: 312, width: 80, height: 80 }, fit: 'fill' },
  },
  {
    name: 'stamp-other-name',
    covers:
      'A second enumerated label, to prove the code reaches the document rather than one label being hardcoded. A label outside the set is refused: the reader reports a custom /Name a file carries, and the writer will not author one.',
    resources: {
      appearance: [137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 2, 8, 2, 0, 0, 0, 240, 202, 234, 52, 0, 0, 0, 16, 73, 68, 65, 84, 120, 156, 99, 184, 163, 161, 1, 71, 12, 200, 28, 0, 127, 154, 9, 97, 122, 180, 22, 59, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130],
    },
    draft: { subtype: 'stamp', box: { x: 370, y: 312, width: 80, height: 80 }, name: 'Draft' },
  },
  {
    name: 'stamp-rotated',
    covers:
      'A rotated stamp, authored in the unrotated frame and turned by an /AP matrix. Fitting into the rotated box instead bakes the letterbox bands in and the closing re-fit shrinks the image a second time.',
    resources: {
      appearance: [137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 2, 8, 2, 0, 0, 0, 240, 202, 234, 52, 0, 0, 0, 16, 73, 68, 65, 84, 120, 156, 99, 184, 163, 161, 1, 71, 12, 200, 28, 0, 127, 154, 9, 97, 122, 180, 22, 59, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130],
    },
    draft: { subtype: 'stamp', box: { x: 75, y: 427, width: 90, height: 60 }, rotation: 330 },
  },
  // `annotations.create` refuses a widget since 1233d1349 ("Read and write
  // widgets through the form"): a widget is written through the form, by
  // `doc.forms.addWidget` onto a field (or `forms.create` with the field),
  // and an inert widget with no field can no longer be made. These six
  // cases created bare widgets through the annotation verb, so they stay
  // here, commented out.
  // {
  //   name: 'widget-bare',
  //   covers:
  //     'An inert widget with only a rect. Every entry it does not name is left off, because an unwritten entry is what the FIELD supplies when one adopts the widget, and a default stamped now would override it.',
  //   draft: {
  //     subtype: 'widget',
  //     rect: { left: 72, bottom: 250, right: 240, top: 274 },
  //   },
  // },
  // {
  //   name: 'widget-styled',
  //   covers:
  //     'The whole widget-plane surface. The colours are /MK rather than /C and /IC, so a writer that used the ordinary colour path leaves a widget whose border and background no reader reports.',
  //   draft: {
  //     subtype: 'widget',
  //     rect: { left: 260, bottom: 250, right: 440, top: 274 },
  //     color: { r: 30, g: 30, b: 120 },
  //     interiorColor: { r: 245, g: 245, b: 230 },
  //     strokeWidth: 2,
  //     borderStyle: 'beveled',
  //     fontFamily: 'times-roman',
  //     fontSize: 11,
  //     fontColor: { r: 20, g: 20, b: 20 },
  //     textAlign: 'center',
  //   },
  // },
  // {
  //   name: 'widget-autosize-text',
  //   covers:
  //     'A /DA size of zero, which for a form control means auto-size to the box. Naming one member of the triple supplies the other two, because /DA is a single string.',
  //   draft: {
  //     subtype: 'widget',
  //     rect: { left: 72, bottom: 210, right: 240, top: 240 },
  //     fontSize: 0,
  //   },
  // },
  // {
  //   name: 'widget-width-only',
  //   covers:
  //     'A border width with no style. /BS is one call taking both, so naming either has to supply the other; saying nothing leaves the entry off.',
  //   draft: {
  //     subtype: 'widget',
  //     rect: { left: 72, bottom: 170, right: 240, top: 200 },
  //     strokeWidth: 3,
  //   },
  // },
  // {
  //   name: 'widget-font-default',
  //   covers:
  //     'A /DA size with no family, so the default family is what lands. The other two members of the triple are supplied because /DA is a single string.',
  //   draft: {
  //     subtype: 'widget',
  //     rect: { left: 260, bottom: 170, right: 440, top: 200 },
  //     fontSize: 14,
  //   },
  // },
  // {
  //   name: 'widget-cleared-colours',
  //   covers:
  //     'An explicit no-colour on both /MK entries, which is a widget that draws no border and has a transparent background. Different from not saying, though not on a new annotation.',
  //   draft: {
  //     subtype: 'widget',
  //     rect: { left: 260, bottom: 210, right: 440, top: 240 },
  //     color: null,
  //     interiorColor: null,
  //     borderStyle: 'dashed',
  //   },
  // },
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
      box: { x: 450, y: 332, width: 100, height: 60 },
      interiorColor: null,
    },
  },
  {
    name: 'circle-defaults',
    covers: 'The other shape subtype, which shares every field and differs only in its code.',
    draft: { subtype: 'circle', box: { x: 450, y: 132, width: 100, height: 60 } },
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
      // Pages are addressed by `PageRef` (9f13cea3c, before this catch-up).
      const page = doc.page(first.ref);
      let result;
      try {
        result = await page.annotations.create(
          entry.draft as unknown as AnnotationDraft,
          resourcesOf(entry.resources),
        );
      } catch (error) {
        // Only the code is recorded: the engines word refusals differently.
        if (!entry.refused) throw error;
        const code = String((error as { code?: unknown })?.code ?? 'unknown');
        return { ...entry, pageObjectNumber: first.ref.objectNumber, refusedWith: code };
      }
      if (entry.refused) throw new Error(`${entry.name}: expected a refusal`);

      // The whole created DTO minus the timestamp. The result names it
      // `annotation` and the timestamp `modifiedAt` since 83f6598d2 and
      // fd175ba50; `weakRefsInvalidated` and `shouldRefetch` went in
      // bc0057f3a ("Remove NM and implement BaseID").
      const { modifiedAt, ...created } = result.annotation as unknown as Record<string, unknown>;
      return {
        ...entry,
        pageObjectNumber: first.ref.objectNumber,
        created,
        hasModified: typeof modifiedAt === 'string' && modifiedAt.length > 0,
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

/**
 * See `generate-annotation-read-vectors.ts`. `creationDate` joins the list
 * for the same reason `modified` is dropped: PDFium stamps it from the clock
 * when the attachment's bytes are written, so the two engines record
 * different instants for the same file. Its PRESENCE is what matters, and
 * the Rust runner asserts that. `normalizeWriteStamps` does the same for
 * what a v3 create stamps.
 */
function normalizeSessionIds(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(normalizeSessionIds);
  if (value === null || typeof value !== 'object') return value;
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>).map(([key, entry]) => {
      if (key === 'docSessionId') return [key, '<session>'];
      if (key === 'creationDate' && typeof entry === 'string') return [key, '<stamped>'];
      const stamped = normalizeWriteStamps(key, entry);
      if (stamped !== undefined) return [key, stamped];
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
