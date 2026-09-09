/**
 * The viewer's BUILT-IN stamp set — the classic rubber stamps, drawn here
 * rather than shipped as files.
 *
 * Why drawn: `armStamp` needs real bytes (the engine's own name-only standard
 * stamps are a rendering path, not an authoring one), and a viewer that has to
 * fetch a `stamps.pdf` before its stamp panel works is a viewer with a network
 * dependency in its chrome. A canvas render is ~80 lines, has no assets to
 * host, and doubles as the gallery thumbnail (a raster asset's preview is its
 * own bytes).
 *
 * Seeding is LAZY — nothing is drawn until the stamps panel is opened for the
 * first time — and idempotent per workspace (see `seedDefaultStamps`).
 */
import type { StampCapability } from '@embedpdf/react/stamp';

/** `/Name` values from the PDF standard stamp set, with this viewer's colors. */
interface StampSpec {
  /** `/Name`-style id, also the asset name shown under the thumbnail. */
  name: string;
  /** What the stamp reads. */
  label: string;
  color: string;
}

const GREEN = '#0f8a3c';
const RED = '#c62828';
const BLUE = '#1f5fb4';

const DEFAULT_STAMPS: readonly StampSpec[] = [
  { name: 'Approved', label: 'APPROVED', color: GREEN },
  { name: 'NotApproved', label: 'NOT APPROVED', color: RED },
  { name: 'Draft', label: 'DRAFT', color: BLUE },
  { name: 'Final', label: 'FINAL', color: GREEN },
  { name: 'Completed', label: 'COMPLETED', color: GREEN },
  { name: 'Confidential', label: 'CONFIDENTIAL', color: RED },
  { name: 'TopSecret', label: 'TOP SECRET', color: RED },
  { name: 'Void', label: 'VOID', color: RED },
  { name: 'Expired', label: 'EXPIRED', color: RED },
  { name: 'Rejected', label: 'REJECTED', color: RED },
  { name: 'Accepted', label: 'ACCEPTED', color: GREEN },
  { name: 'AsIs', label: 'AS IS', color: BLUE },
  { name: 'Experimental', label: 'EXPERIMENTAL', color: BLUE },
  { name: 'ForComment', label: 'FOR COMMENT', color: BLUE },
  { name: 'ForPublicRelease', label: 'FOR PUBLIC RELEASE', color: GREEN },
  { name: 'NotForPublicRelease', label: 'NOT FOR PUBLIC RELEASE', color: RED },
  { name: 'PreliminaryResults', label: 'PRELIMINARY RESULTS', color: BLUE },
  { name: 'InformationOnly', label: 'INFORMATION ONLY', color: BLUE },
  { name: 'Departmental', label: 'DEPARTMENTAL', color: BLUE },
  { name: 'Sold', label: 'SOLD', color: BLUE },
  { name: 'SignHere', label: 'SIGN HERE', color: GREEN },
  { name: 'InitialHere', label: 'INITIAL HERE', color: GREEN },
  { name: 'Witness', label: 'WITNESS', color: GREEN },
];

/** Geometry in PDF POINTS — the size the stamp is placed at. */
const HEIGHT_PT = 44;
const FONT_PT = 21;
const PAD_X_PT = 18;
const STROKE_PT = 2.5;
const RADIUS_PT = 7;
const MIN_WIDTH_PT = 96;
/** Device px per point: the raster is drawn oversampled so it stays crisp
 *  when a reader zooms past 100%. */
const SCALE = 4;

const FONT_STACK =
  '"Helvetica Neue", Helvetica, Arial, "Liberation Sans", "Segoe UI", system-ui, sans-serif';

const roundedRectPath = (
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void => {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
};

const toBlob = (canvas: HTMLCanvasElement): Promise<Blob> =>
  new Promise((resolve, reject) =>
    canvas.toBlob(
      (blob) => (blob ? resolve(blob) : reject(new Error('[stamps] canvas.toBlob returned null'))),
      'image/png',
    ),
  );

/** One stamp: transparent PNG at {@link SCALE}×, plus its size in points. */
async function drawStamp(
  spec: StampSpec,
): Promise<{ source: Blob; size: { width: number; height: number } }> {
  const measure = document.createElement('canvas').getContext('2d');
  if (!measure) throw new Error('[stamps] no 2d canvas context');
  const font = `bold ${FONT_PT * SCALE}px ${FONT_STACK}`;
  measure.font = font;
  const textPx = measure.measureText(spec.label).width;
  const widthPt = Math.max(MIN_WIDTH_PT, Math.round(textPx / SCALE) + PAD_X_PT * 2);

  const canvas = document.createElement('canvas');
  canvas.width = widthPt * SCALE;
  canvas.height = HEIGHT_PT * SCALE;
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('[stamps] no 2d canvas context');
  ctx.scale(SCALE, SCALE);

  const inset = STROKE_PT / 2;
  ctx.strokeStyle = spec.color;
  ctx.lineWidth = STROKE_PT;
  roundedRectPath(ctx, inset, inset, widthPt - STROKE_PT, HEIGHT_PT - STROKE_PT, RADIUS_PT);
  ctx.stroke();

  ctx.fillStyle = spec.color;
  ctx.font = `bold ${FONT_PT}px ${FONT_STACK}`;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  // +1pt optical nudge: cap-height text sits high against a geometric box.
  ctx.fillText(spec.label, widthPt / 2, HEIGHT_PT / 2 + 1);

  return { source: await toBlob(canvas), size: { width: widthPt, height: HEIGHT_PT } };
}

/** The built-in library's id — excluded from persistence (it is drawn again
 *  on every first open, in the locale of that moment). */
export const DEFAULT_LIBRARY_ID = 'embedpdf-standard';

/** Workspaces whose default library has already been seeded — the panel
 *  mounts and unmounts with the sidebar, so the guard cannot live in state. */
const seeded = new WeakSet<StampCapability>();

/**
 * Add the built-in stamps as one named library. A no-op after the first call
 * per workspace, and after the user has libraries of their own (so deleting
 * the defaults sticks, and an embedder that seeds its own set is left alone).
 */
export async function seedDefaultStamps(
  stamp: StampCapability,
  libraryName: string,
): Promise<void> {
  if (seeded.has(stamp)) return;
  seeded.add(stamp);
  if (stamp.libraries().length > 0) return;
  if (typeof document === 'undefined') return;

  const libraryId = await stamp.createLibrary(libraryName, {
    id: DEFAULT_LIBRARY_ID,
    categories: ['standard'],
  });
  for (const spec of DEFAULT_STAMPS) {
    const { source, size } = await drawStamp(spec);
    // `name` is the standard /Name every placement writes; the label is
    // what the stamp reads.
    await stamp.addAsset({ libraryId, name: spec.name, label: spec.label, source, size });
  }
}
