/**
 * The annotation create and update cases are written in the annotation shape
 * the engine took before upstream's Engine API v3. They keep that spelling,
 * and the vectors echo it. This module turns one into what the engine takes
 * now, so the case reaches the engine unchanged in meaning.
 *
 * What changed, and where:
 *
 * - Colours are `'#rrggbb'` strings, not `{ r, g, b }` (c053c3d33, "Page
 *   space, forms and measure; colors as hex").
 * - A quad's corners are `upperLeft`, `upperRight`, `lowerLeft` and
 *   `lowerRight`, the old `p1` to `p4` in order (00d2c8760, "Quads: one
 *   shape").
 * - Flags are top-level booleans, not a `flags` object (c78997346, "One
 *   annotation shape per kind, declared once").
 * - Square, circle, caret, free text and stamp take their shape as `box`
 *   (the old `unrotatedRect`, or the old `rect` when there was none) and
 *   `rotation`; their `rect` is the engine's, `rectDifferences` is gone, and
 *   `rotation` is degrees clockwise where it was the file's counterclockwise
 *   angle (2bdc74fbf, "round 5b: clockwise rotation, box and the engine's
 *   rect"). Every text markup, polygon, polyline, line and ink annotation's
 *   `rect` is the engine's too (6153c181e, "upright points, icons at their
 *   corner, the engine's rect"): a write that names one is refused. The same
 *   commit made a vertex kind's points upright, turned by its `rotation`, so
 *   a case that gives points as drawn and a rotation beside them describes a
 *   differently drawn annotation now; those cases are left as written.
 * - A stamp's image and an attachment's file travel beside the data, as the
 *   `appearance` and `file` resources of the write's options, instead of
 *   inside it (11a1259d9, "Annotation bytes travel beside the data").
 * - A destination names its page as a `PageRef`, not a `pageObjectNumber`
 *   (9f13cea3c, "Address pages by PageRef everywhere", which predates this
 *   catch-up).
 * - The cases' coordinates are PDF space, and the engine takes page space
 *   (1a243431a, "Page space, annotations"), so the result is converted
 *   through the page's visible box the way the engine converts back.
 */
import {
  pageAnnotationDraftOf,
  pageAnnotationPatchOf,
  type VisibleBoxOf,
} from '../../../packages/engine/core/src/pageSpace';
import type {
  AnnotationDraft,
  AnnotationPatch,
} from '../../../packages/engine/core/src/annotation/kinds';
import type { AnnotationResources } from '../../../packages/engine/core/src/annotation/resources';
import type { PdfCoordinates } from '../../../packages/engine/core/src/pageSpace/coordinates';
import type { PdfRect } from '../../../packages/engine/core/src/geometry/primitives';

/** Kinds whose shape is a box that can turn. */
const BOX_KINDS = new Set(['square', 'circle', 'caret', 'free-text', 'stamp']);

/** Kinds whose `rect` the engine derives from their points or quads. */
const DERIVED_RECT_KINDS = new Set([
  'highlight',
  'underline',
  'squiggly',
  'strikeout',
  'polygon',
  'polyline',
  'line',
  'ink',
]);

export interface V3Write<T> {
  readonly write: T;
  readonly resources?: AnnotationResources;
}

export interface PageFrame {
  /** The page's visible box in PDF space (`PageLayout.pdfCropBox`). */
  readonly visible: PdfRect;
  /** Every page's visible box, for destinations on other pages. */
  readonly boxOf: VisibleBoxOf;
}

function hex(channel: unknown): string {
  return Math.round(Number(channel)).toString(16).padStart(2, '0');
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

/** Colours, quads and destination pages, wherever they sit in a value. */
function v3Value(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(v3Value);
  if (!isRecord(value)) return value;
  const keys = Object.keys(value).sort().join(',');
  if (keys === 'b,g,r') return `#${hex(value.r)}${hex(value.g)}${hex(value.b)}`;
  if (keys === 'p1,p2,p3,p4') {
    return {
      upperLeft: v3Value(value.p1),
      upperRight: v3Value(value.p2),
      lowerLeft: v3Value(value.p3),
      lowerRight: v3Value(value.p4),
    };
  }
  return Object.fromEntries(
    Object.entries(value).map(([key, entry]) =>
      key === 'pageObjectNumber'
        ? ['page', { kind: 'objectNumber', objectNumber: entry }]
        : [key, v3Value(entry)],
    ),
  );
}

/** The file's counterclockwise angle as the API's clockwise one. */
function clockwise(degrees: number): number {
  return (((-degrees % 360) + 360) % 360) + 0;
}

function bytesOf(
  resource: unknown,
  payloads: Readonly<Record<string, readonly number[]>>,
): Uint8Array {
  const bytes = typeof resource === 'string' ? payloads[resource] : undefined;
  if (!bytes) throw new Error(`no payload for resource '${String(resource)}'`);
  return Uint8Array.from(bytes);
}

/** One old draft or patch as the engine's own fields, still in PDF space. */
function v3Fields(
  old: Record<string, unknown>,
  payloads: Readonly<Record<string, readonly number[]>>,
): V3Write<Record<string, unknown>> {
  const subtype = String(old.subtype);
  const write: Record<string, unknown> = {};
  const resources: AnnotationResources = {};
  for (const [key, value] of Object.entries(old)) {
    switch (key) {
      case 'flags':
        Object.assign(write, value);
        break;
      case 'rectDifferences':
        break;
      case 'rect':
        if (BOX_KINDS.has(subtype)) write.box = old.unrotatedRect ?? value;
        else if (!DERIVED_RECT_KINDS.has(subtype)) write.rect = value;
        break;
      case 'unrotatedRect':
        if (BOX_KINDS.has(subtype) && !('rect' in old)) write.box = value;
        break;
      case 'rotation':
        write.rotation = typeof value === 'number' ? clockwise(value) : value;
        break;
      case 'source': {
        resources.appearance = bytesOf(isRecord(value) ? value.resource : undefined, payloads);
        break;
      }
      case 'file': {
        if (isRecord(value) && 'resource' in value) {
          const { resource, ...info } = value;
          resources.file = bytesOf(resource, payloads);
          write.file = info;
        } else {
          write.file = value;
        }
        break;
      }
      default:
        write[key] = v3Value(value);
    }
  }
  return Object.keys(resources).length > 0 ? { write, resources } : { write };
}

/** An old-spelling draft as the page-space draft `annotations.create` takes. */
export function v3Draft(
  old: Record<string, unknown>,
  payloads: Readonly<Record<string, readonly number[]>>,
  frame: PageFrame,
): V3Write<AnnotationDraft> {
  const { write, resources } = v3Fields(old, payloads);
  return {
    write: pageAnnotationDraftOf(
      write as unknown as AnnotationDraft<PdfCoordinates>,
      frame.visible,
      frame.boxOf,
    ),
    ...(resources ? { resources } : {}),
  };
}

/** An old-spelling patch as the page-space patch `annotations.update` takes. */
export function v3Patch(
  old: Record<string, unknown>,
  payloads: Readonly<Record<string, readonly number[]>>,
  frame: PageFrame,
): V3Write<AnnotationPatch> {
  const { write, resources } = v3Fields(old, payloads);
  return {
    write: pageAnnotationPatchOf(
      write as unknown as AnnotationPatch<PdfCoordinates>,
      frame.visible,
      frame.boxOf,
    ),
    ...(resources ? { resources } : {}),
  };
}

/** The frame of the first page of a listing, which every case writes on. */
export function frameOf(
  pages: readonly { ref: { objectNumber: number }; pdfCropBox: PdfRect }[],
): PageFrame {
  const first = pages[0];
  if (!first) throw new Error('the fixture has no pages');
  const boxes = new Map(pages.map((page) => [page.ref.objectNumber, page.pdfCropBox]));
  return {
    visible: first.pdfCropBox,
    boxOf: (page) => {
      const box = boxes.get(page.objectNumber);
      if (!box) throw new Error(`no page ${page.objectNumber} in the listing`);
      return box;
    },
  };
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/**
 * The values a v3 write stamps from the clock or a random source, as
 * placeholders. The engine gives a new annotation an `/NM` when the draft
 * names none (a UUID, bc0057f3a) and stamps `createdAt` on it and on an
 * attached file's info (fd175ba50 named the field), so a verbatim recording
 * would differ on every run. What matters is that each is there.
 */
export function normalizeWriteStamps(key: string, value: unknown): unknown {
  if (key === 'createdAt' && typeof value === 'string') return '<stamped>';
  if (key === 'nm' && typeof value === 'string' && UUID.test(value)) return '<stamped>';
  return undefined;
}
