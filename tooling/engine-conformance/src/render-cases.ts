/**
 * The render-geometry corpus: how a PDF region becomes a device raster.
 *
 * G4's gate is mostly pixels, and pixels are checked against a fixture. This
 * covers the part before the pixels, which is pure arithmetic and the part
 * most likely to diverge invisibly: a matrix that is subtly wrong produces a
 * picture that is subtly wrong, and nobody notices by looking.
 *
 * Every case runs through `pdfRectToDisplayRect`, then `deviceSize`, then
 * `displayRectToDeviceMatrix`, because that is the order `rasterize` calls
 * them and the display flip changes what the other two see.
 */

import type { PdfRect } from '../../../packages/engine/core/src/geometry/primitives';

export interface RenderCase {
  readonly name: string;
  readonly covers: string;
  /** The region, in PDF user space, already normalized. */
  readonly rect: PdfRect;
  /** The page's height, which the display flip needs. */
  readonly pageHeight: number;
  readonly rotation: 0 | 90 | 180 | 270;
  readonly viewport:
    | { readonly kind: 'scale'; readonly scale: number }
    | { readonly kind: 'width'; readonly width: number };
}

/** US Letter, the page every case below is a region of unless it says otherwise. */
const LETTER = { width: 612, height: 792 };

const wholePage: PdfRect = { left: 0, bottom: 0, right: LETTER.width, top: LETTER.height };

export const RENDER_CASES: readonly RenderCase[] = [
  {
    name: 'whole-page-unrotated',
    covers: 'The dominant case, and the one every other is a deviation from.',
    rect: wholePage,
    pageHeight: LETTER.height,
    rotation: 0,
    viewport: { kind: 'scale', scale: 1 },
  },
  {
    name: 'whole-page-scaled-up',
    covers: 'A scale above one, where rounding can push a dimension either way.',
    rect: wholePage,
    pageHeight: LETTER.height,
    rotation: 0,
    viewport: { kind: 'scale', scale: 2.5 },
  },
  {
    name: 'whole-page-scaled-down',
    covers: 'A scale below one.',
    rect: wholePage,
    pageHeight: LETTER.height,
    rotation: 0,
    viewport: { kind: 'scale', scale: 0.25 },
  },
  {
    name: 'scale-that-rounds-to-half-a-pixel',
    covers: 'A scale chosen so both dimensions land on .5 and the rounding rule shows.',
    rect: { left: 0, bottom: 0, right: 101, top: 101 },
    pageHeight: 101,
    rotation: 0,
    viewport: { kind: 'scale', scale: 1.5 },
  },
  {
    name: 'scale-so-small-the-page-is-one-pixel',
    covers: 'Both dimensions clamp to one rather than to zero.',
    rect: wholePage,
    pageHeight: LETTER.height,
    rotation: 0,
    viewport: { kind: 'scale', scale: 0.0001 },
  },

  // Rotation. Quarter turns swap the axes, which changes both the size and
  // the matrix, and a half turn changes only the matrix.
  {
    name: 'whole-page-rotated-90',
    covers: 'A quarter turn swaps width for height.',
    rect: wholePage,
    pageHeight: LETTER.height,
    rotation: 90,
    viewport: { kind: 'scale', scale: 1 },
  },
  {
    name: 'whole-page-rotated-180',
    covers: 'A half turn keeps the size and negates the matrix.',
    rect: wholePage,
    pageHeight: LETTER.height,
    rotation: 180,
    viewport: { kind: 'scale', scale: 1 },
  },
  {
    name: 'whole-page-rotated-270',
    covers: 'The other quarter turn, which is not the 90 matrix negated.',
    rect: wholePage,
    pageHeight: LETTER.height,
    rotation: 270,
    viewport: { kind: 'scale', scale: 1 },
  },

  // Width viewports. Height comes from the ratio, so rotation changes it.
  {
    name: 'width-viewport-unrotated',
    covers: 'An exact width, with height from the aspect ratio.',
    rect: wholePage,
    pageHeight: LETTER.height,
    rotation: 0,
    viewport: { kind: 'width', width: 1000 },
  },
  {
    name: 'width-viewport-rotated-90',
    covers: 'The same width against the swapped aspect ratio.',
    rect: wholePage,
    pageHeight: LETTER.height,
    rotation: 90,
    viewport: { kind: 'width', width: 1000 },
  },
  {
    name: 'width-viewport-fractional',
    covers: 'A width that does not round to itself.',
    rect: wholePage,
    pageHeight: LETTER.height,
    rotation: 0,
    viewport: { kind: 'width', width: 333.7 },
  },
  {
    name: 'width-viewport-of-one',
    covers: 'The narrowest request, where the height ratio also collapses.',
    rect: wholePage,
    pageHeight: LETTER.height,
    rotation: 0,
    viewport: { kind: 'width', width: 1 },
  },

  // Sub-rects. The display flip means a region near the top of the page is
  // near the bottom in display space, which is where an axis mistake shows.
  {
    name: 'top-left-quadrant',
    covers: 'A region whose display-space bottom is not zero.',
    rect: { left: 0, bottom: 396, right: 306, top: 792 },
    pageHeight: LETTER.height,
    rotation: 0,
    viewport: { kind: 'scale', scale: 1 },
  },
  {
    name: 'bottom-right-quadrant',
    covers: 'The opposite corner, so a sign error cannot pass both.',
    rect: { left: 306, bottom: 0, right: 612, top: 396 },
    pageHeight: LETTER.height,
    rotation: 0,
    viewport: { kind: 'scale', scale: 1 },
  },
  {
    name: 'centre-strip-rotated-90',
    covers: 'An off-origin region under a quarter turn, which is where the two get confused.',
    rect: { left: 100, bottom: 300, right: 500, top: 500 },
    pageHeight: LETTER.height,
    rotation: 90,
    viewport: { kind: 'scale', scale: 2 },
  },
  {
    name: 'centre-strip-rotated-270',
    covers: 'The same region the other way round.',
    rect: { left: 100, bottom: 300, right: 500, top: 500 },
    pageHeight: LETTER.height,
    rotation: 270,
    viewport: { kind: 'scale', scale: 2 },
  },
  {
    name: 'sub-rect-with-a-width-viewport',
    covers: 'A width viewport applied to a region rather than the page.',
    rect: { left: 50, bottom: 50, right: 250, top: 150 },
    pageHeight: LETTER.height,
    rotation: 0,
    viewport: { kind: 'width', width: 800 },
  },
  {
    name: 'region-taller-than-it-is-wide',
    covers: 'A width viewport on a narrow region, where the height multiplies up.',
    rect: { left: 0, bottom: 0, right: 10, top: 700 },
    pageHeight: LETTER.height,
    rotation: 0,
    viewport: { kind: 'width', width: 100 },
  },
  {
    name: 'fractional-rect-bounds',
    covers: 'PDF boxes are not integers, and the matrix must not assume they are.',
    rect: { left: 12.34, bottom: 56.78, right: 400.5, top: 700.25 },
    pageHeight: 792.5,
    rotation: 0,
    viewport: { kind: 'scale', scale: 1.7 },
  },
  {
    name: 'region-flush-with-the-page-top',
    covers: 'Display-space bottom of exactly zero, which is the flip boundary.',
    rect: { left: 0, bottom: 692, right: 612, top: 792 },
    pageHeight: LETTER.height,
    rotation: 0,
    viewport: { kind: 'scale', scale: 1 },
  },
] as const;
