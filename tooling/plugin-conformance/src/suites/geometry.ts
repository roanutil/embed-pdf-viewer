/**
 * `packages/core/geometry`. Replayed by
 * `crates/plugins/geometry/tests/conformance.rs`.
 *
 * Pure functions, so every file is a function suite. `pageTransform` returns
 * closures; its entry here is a wrapper that builds the transform and calls
 * each method on the probe points and rects the case supplies.
 */
import * as g from '../../../../packages/core/geometry/src/index';
import type { Point, Quad, Rect, TextQuad } from '../../../../packages/core/geometry/src/index';

import type { FunctionCase, Suite } from '../vectors';

type Fn = (...args: never[]) => unknown;

/** A case whose name is `<fn>/<label>`. */
const c = (fn: string, label: string, ...args: unknown[]): FunctionCase => ({ name: `${fn}/${label}`, fn, args });

const pt = (x: number, y: number): Point => ({ x, y });
const rc = (x: number, y: number, width: number, height: number): Rect => ({ x, y, width, height });
const quad = (p1: Point, p2: Point, p3: Point, p4: Point): Quad => ({ p1, p2, p3, p4 });

const ROTATIONS = [0, 90, 180, 270];
const ODD_ROTATIONS = [45, -90, 360, 450, 90.5, NaN, Infinity];

// ── rects, sizes, rotations, rounding ───────────────────────────────────────

const basicsFns: Record<string, Fn> = {
  NO_FRAME: () => g.NO_FRAME,
  boundsOfRects: g.boundsOfRects,
  intersectRects: g.intersectRects,
  rectFromCorners: g.rectFromCorners,
  rectContains: g.rectContains,
  rectsOverlap: g.rectsOverlap,
  isQuarterTurn: g.isQuarterTurn,
  addRotations: g.addRotations,
  snapToDevice: g.snapToDevice,
  displaySize: g.displaySize,
  deviceHeightForWidth: g.deviceHeightForWidth,
};

const basics: FunctionCase[] = [
  c('NO_FRAME', 'zero-on-every-side'),

  c('boundsOfRects', 'empty-is-null', []),
  c('boundsOfRects', 'one-rect-by-value', [rc(5, 10, 20, 30)]),
  c('boundsOfRects', 'disjoint', [rc(0, 0, 10, 10), rc(30, 40, 10, 10)]),
  c('boundsOfRects', 'containment-and-negative', [rc(-10, -5, 100, 50), rc(0, 0, 10, 10)]),
  c('boundsOfRects', 'multi-line-hits', [rc(200, 100, 150, 12), rc(40, 114, 90, 12)]),
  c('boundsOfRects', 'second-rect-extends-min-only', [rc(10, 10, 10, 10), rc(0, 0, 1, 1)]),
  c('boundsOfRects', 'second-rect-extends-max-only', [rc(0, 0, 1, 1), rc(0, 0, 50, 60)]),
  c('boundsOfRects', 'zero-size-rects', [rc(3, 4, 0, 0), rc(7, 9, 0, 0)]),
  c('boundsOfRects', 'inverted-rect', [rc(10, 10, -5, -5)]),
  c('boundsOfRects', 'inverted-and-normal', [rc(10, 10, -5, -5), rc(0, 0, 2, 2)]),
  c('boundsOfRects', 'nan-x-poisons-x', [rc(NaN, 0, 1, 1), rc(0, 0, 1, 1)]),
  c('boundsOfRects', 'nan-height-poisons-height', [rc(0, 0, 1, 1), rc(0, 0, 1, NaN)]),
  c('boundsOfRects', 'nan-width-late', [rc(0, 0, 1, 1), rc(5, 5, NaN, 1)]),
  c('boundsOfRects', 'infinite-extent', [rc(0, 0, Infinity, 1), rc(-Infinity, 0, 1, 1)]),
  c('boundsOfRects', 'huge-and-tiny', [rc(1e300, 1e-300, 1e300, 5e-324), rc(-1e300, 0, 1, 1)]),
  c('boundsOfRects', 'three-rects', [rc(1, 2, 3, 4), rc(-1, 5, 2, 2), rc(0, -3, 10, 1)]),

  c('intersectRects', 'overlap', rc(0, 0, 10, 10), rc(5, 5, 10, 10)),
  c('intersectRects', 'disjoint-is-zero-sized', rc(0, 0, 10, 10), rc(20, 20, 5, 5)),
  c('intersectRects', 'contained', rc(0, 0, 10, 10), rc(-5, -5, 30, 30)),
  c('intersectRects', 'touching-edges', rc(0, 0, 10, 10), rc(10, 0, 10, 10)),
  c('intersectRects', 'disjoint-before', rc(20, 20, 5, 5), rc(0, 0, 10, 10)),
  c('intersectRects', 'disjoint-in-x-only', rc(0, 0, 10, 10), rc(20, 2, 5, 5)),
  c('intersectRects', 'disjoint-in-y-only', rc(0, 0, 10, 10), rc(2, 20, 5, 5)),
  c('intersectRects', 'identical', rc(1.5, 2.5, 3.25, 4.75), rc(1.5, 2.5, 3.25, 4.75)),
  c('intersectRects', 'zero-size', rc(0, 0, 0, 0), rc(0, 0, 0, 0)),
  c('intersectRects', 'inverted', rc(10, 10, -5, -5), rc(0, 0, 20, 20)),
  c('intersectRects', 'negative-coordinates', rc(-30, -20, 25, 25), rc(-10, -10, 100, 100)),
  c('intersectRects', 'nan-in-a', rc(NaN, 0, 10, 10), rc(0, 0, 10, 10)),
  c('intersectRects', 'nan-in-b', rc(0, 0, 10, 10), rc(0, 0, 10, NaN)),
  c('intersectRects', 'nan-width-in-a', rc(0, 0, NaN, 10), rc(0, 0, 10, 10)),
  c('intersectRects', 'nan-y-in-b', rc(0, 0, 10, 10), rc(0, NaN, 10, 10)),
  c('intersectRects', 'infinite', rc(-Infinity, -Infinity, Infinity, Infinity), rc(0, 0, 10, 10)),
  c('intersectRects', 'infinite-extent', rc(0, 0, Infinity, Infinity), rc(5, 5, 10, 10)),

  c('rectFromCorners', 'ordered', pt(1, 2), pt(11, 22)),
  c('rectFromCorners', 'reversed', pt(11, 22), pt(1, 2)),
  c('rectFromCorners', 'mixed', pt(11, 2), pt(1, 22)),
  c('rectFromCorners', 'same-point', pt(5, 5), pt(5, 5)),
  c('rectFromCorners', 'negative', pt(-3, -4), pt(-10, 6)),
  c('rectFromCorners', 'nan', pt(NaN, 0), pt(1, 1)),
  c('rectFromCorners', 'infinite', pt(-Infinity, 0), pt(1, Infinity)),

  c('rectContains', 'inside', rc(0, 0, 10, 10), pt(5, 5)),
  c('rectContains', 'top-left-corner', rc(0, 0, 10, 10), pt(0, 0)),
  c('rectContains', 'bottom-right-corner', rc(0, 0, 10, 10), pt(10, 10)),
  c('rectContains', 'left-of', rc(0, 0, 10, 10), pt(-0.001, 5)),
  c('rectContains', 'right-of', rc(0, 0, 10, 10), pt(10.001, 5)),
  c('rectContains', 'above', rc(0, 0, 10, 10), pt(5, -0.001)),
  c('rectContains', 'below', rc(0, 0, 10, 10), pt(5, 10.001)),
  c('rectContains', 'inverted-rect', rc(10, 10, -5, -5), pt(7, 7)),
  c('rectContains', 'nan-point', rc(0, 0, 10, 10), pt(NaN, 5)),
  c('rectContains', 'zero-size-on-point', rc(3, 3, 0, 0), pt(3, 3)),

  c('rectsOverlap', 'overlap', rc(0, 0, 10, 10), rc(5, 5, 10, 10)),
  c('rectsOverlap', 'touching-edge', rc(0, 0, 10, 10), rc(10, 0, 10, 10)),
  c('rectsOverlap', 'touching-bottom', rc(0, 0, 10, 10), rc(0, 10, 10, 10)),
  c('rectsOverlap', 'disjoint', rc(0, 0, 10, 10), rc(20, 20, 5, 5)),
  c('rectsOverlap', 'contained', rc(0, 0, 10, 10), rc(2, 2, 1, 1)),
  c('rectsOverlap', 'zero-width', rc(0, 0, 10, 10), rc(2, 2, 0, 5)),
  c('rectsOverlap', 'zero-height', rc(0, 0, 10, 10), rc(2, 2, 5, 0)),
  c('rectsOverlap', 'nan', rc(0, 0, 10, 10), rc(NaN, 0, 5, 5)),

  ...[...ROTATIONS, ...ODD_ROTATIONS].map((r) => c('isQuarterTurn', `r${r}`, r)),

  ...ROTATIONS.flatMap((a) => ROTATIONS.map((b) => c('addRotations', `${a}+${b}`, a, b))),
  c('addRotations', 'negative-keeps-sign', -90, 0),
  c('addRotations', 'negative-sum', -270, 90),
  c('addRotations', 'out-of-range-45', 90, 45),
  c('addRotations', 'fraction', 90.5, 270),
  c('addRotations', 'large', 720, 450),
  c('addRotations', 'nan', NaN, 90),
  c('addRotations', 'infinity', Infinity, 90),

  c('snapToDevice', 'integer', 10, 1),
  c('snapToDevice', 'dpr-2-half', 10.25, 2),
  c('snapToDevice', 'dpr-2-tie-up', 10.75, 2),
  c('snapToDevice', 'tie-rounds-up', 2.5, 1),
  c('snapToDevice', 'negative-tie-rounds-toward-plus-infinity', -2.5, 1),
  c('snapToDevice', 'negative-tie-small', -0.5, 1),
  c('snapToDevice', 'negative-tie-odd', -1.5, 1),
  c('snapToDevice', 'just-under-half', 0.49999999999999994, 1),
  c('snapToDevice', 'just-under-half-negative', -0.49999999999999994, 1),
  c('snapToDevice', 'beyond-2^52', 4503599627370497, 1),
  c('snapToDevice', 'huge', 1e300, 1),
  c('snapToDevice', 'fractional-dpr', 13.37, 1.5),
  c('snapToDevice', 'dpr-3', 100 / 3, 3),
  c('snapToDevice', 'dpr-0', 5, 0),
  c('snapToDevice', 'negative-dpr', 5.3, -2),
  c('snapToDevice', 'nan-value', NaN, 1),
  c('snapToDevice', 'infinite-value', Infinity, 2),
  c('snapToDevice', 'infinite-dpr', 1, Infinity),
  c('snapToDevice', 'zero', 0, 1),
  c('snapToDevice', 'negative', -7.3, 2),

  ...[...ROTATIONS, ...ODD_ROTATIONS].map((r) => c('displaySize', `r${r}`, { width: 600, height: 800 }, r)),
  c('displaySize', 'zero-size-quarter-turn', { width: 0, height: 5 }, 90),
  c('displaySize', 'nan-size', { width: NaN, height: 5 }, 270),

  c('deviceHeightForWidth', 'letter-739', { width: 612, height: 792 }, 739),
  c('deviceHeightForWidth', 'exact', { width: 100, height: 200 }, 100),
  c('deviceHeightForWidth', 'tie-rounds-up', { width: 2, height: 1 }, 1),
  c('deviceHeightForWidth', 'tie-rounds-up-3', { width: 2, height: 5 }, 1),
  c('deviceHeightForWidth', 'clamped-to-one', { width: 1000, height: 1 }, 10),
  c('deviceHeightForWidth', 'clamp-boundary', { width: 2, height: 1 }, 3),
  c('deviceHeightForWidth', 'zero-width-device', { width: 100, height: 100 }, 0),
  c('deviceHeightForWidth', 'negative-device', { width: 100, height: 100 }, -50),
  c('deviceHeightForWidth', 'zero-page-width', { width: 0, height: 100 }, 10),
  c('deviceHeightForWidth', 'zero-by-zero', { width: 0, height: 0 }, 10),
  c('deviceHeightForWidth', 'nan-height', { width: 10, height: NaN }, 10),
  c('deviceHeightForWidth', 'infinite-height', { width: 10, height: Infinity }, 10),
  c('deviceHeightForWidth', 'huge', { width: 1, height: 1e300 }, 1e10),
  c('deviceHeightForWidth', 'landscape', { width: 792, height: 612 }, 1000),
];

// ── affine matrices ─────────────────────────────────────────────────────────

const affineFns: Record<string, Fn> = {
  identity: g.identity,
  compose: g.compose,
  invert: g.invert,
  applyPoint: g.applyPoint,
  applyRect: g.applyRect,
  applyQuad: g.applyQuad,
  matrixToCss: g.matrixToCss,
  translate: g.translate,
  scale: g.scale,
  rotate: g.rotate,
  rotateAbout: g.rotateAbout,
  scaleAbout: g.scaleAbout,
  angleOf: g.angleOf,
};

const shear = [1, 0, 0.5, 1, 0, 0];
const skew = [1.2, 0.3, -0.7, 0.9, 11, -4];
const m1 = [2, 0, 0, 2, 5, 7];
const m2 = [0, 1, -1, 0, 3, 4];
const m3 = [1, 0, 0, -1, 10, 20];

const cssNumbers: [string, number][] = [
  ['integers', 1],
  ['negative-zero', -0],
  ['tenth', 0.1],
  ['sum-imprecise', 0.1 + 0.2],
  ['third', 1 / 3],
  ['micro', 0.000001],
  ['below-micro', 0.0000001],
  ['small-sci', 1.5e-10],
  ['smallest-subnormal', 5e-324],
  ['min-normal', 2.2250738585072014e-308],
  ['e20', 1e20],
  ['e21', 1e21],
  ['big-digits', 123456789012345680000],
  ['big-sci', 1.2345e25],
  ['max', 1.7976931348623157e308],
  ['nan', NaN],
  ['infinity', Infinity],
  ['negative-infinity', -Infinity],
  ['negative-fraction', -12.5],
  ['cos-half-pi', 6.123233995736766e-17],
  ['precise', 0.30000000000000004],
  ['long-int', 9007199254740993],
  ['mixed', 1234.5678],
  ['e-7-digits', 1.2345678e-7],
  ['e21-digits', 1.5e21],
  ['e-6-digits', 0.0000012345],
];

const affine: FunctionCase[] = [
  c('identity', 'six-numbers'),

  c('translate', 'basic', 5, -3),
  c('translate', 'zero', 0, 0),
  c('translate', 'nan', NaN, Infinity),
  c('scale', 'basic', 2, 3),
  c('scale', 'zero', 0, -1),
  c('rotate', 'zero', 0),
  c('rotate', 'half-pi', Math.PI / 2),
  c('rotate', 'pi', Math.PI),
  c('rotate', '0.7', 0.7),
  c('rotate', 'negative', -1.2345),
  c('rotate', 'large', 1000),
  c('rotate', 'nan', NaN),
  c('rotate', 'infinity', Infinity),
  c('rotateAbout', 'pivot', pt(10, 20), 1.2345),
  c('rotateAbout', 'origin', pt(0, 0), Math.PI / 2),
  c('rotateAbout', 'negative-pivot', pt(-3, 4), -0.7),
  c('rotateAbout', 'zero-angle', pt(5, 5), 0),
  c('scaleAbout', 'anchor', pt(10, 10), 2, 2),
  c('scaleAbout', 'anisotropic', pt(-4, 7), 0.5, 3),
  c('scaleAbout', 'mirror', pt(3, 3), -1, 1),
  c('scaleAbout', 'collapse', pt(1, 2), 0, 0),

  c('angleOf', 'identity', [1, 0, 0, 1, 0, 0]),
  c('angleOf', 'rotate-0.7', g.rotate(0.7) as unknown as number[]),
  c('angleOf', 'quarter', [0, 1, -1, 0, 0, 0]),
  c('angleOf', 'half', [-1, 0, 0, -1, 0, 0]),
  c('angleOf', 'three-quarter', [0, -1, 1, 0, 0, 0]),
  c('angleOf', 'zero-matrix', [0, 0, 0, 0, 0, 0]),
  c('angleOf', 'scaled', [2, 2, -2, 2, 9, 9]),
  c('angleOf', 'nan', [NaN, 1, 0, 1, 0, 0]),
  c('angleOf', 'negative-a', [-1, -0.0001, 0, 1, 0, 0]),

  c('compose', 'identity-left', [1, 0, 0, 1, 0, 0], m1),
  c('compose', 'identity-right', m1, [1, 0, 0, 1, 0, 0]),
  c('compose', 'm1-m2', m1, m2),
  c('compose', 'm2-m1', m2, m1),
  c('compose', 'm2-m3', m2, m3),
  c('compose', 'skew-skew', skew, skew),
  c('compose', 'skew-shear', skew, shear),
  c('compose', 'shear-skew', shear, skew),
  c('compose', 'every-slot', [2, 3, 5, 7, 11, 13], [17, 19, 23, 29, 31, 37]),
  c('compose', 'every-slot-reversed', [17, 19, 23, 29, 31, 37], [2, 3, 5, 7, 11, 13]),
  c('compose', 'nan-propagates', [NaN, 0, 0, 1, 0, 0], m1),
  c('compose', 'infinity-times-zero', [Infinity, 0, 0, 1, 0, 0], [0, 0, 0, 1, 0, 0]),

  c('invert', 'identity', [1, 0, 0, 1, 0, 0]),
  c('invert', 'translate', [1, 0, 0, 1, 5, -3]),
  c('invert', 'scale-translate', m1),
  c('invert', 'quarter-turn', m2),
  c('invert', 'flip', m3),
  c('invert', 'skew', skew),
  c('invert', 'every-slot', [2, 3, 5, 7, 11, 13]),
  c('invert', 'every-slot-b', [3, -2, 7, 5, -11, 13]),
  c('invert', 'singular-zero', [0, 0, 0, 0, 0, 0]),
  c('invert', 'singular-rank-one', [1, 2, 2, 4, 5, 6]),
  c('invert', 'underflowing-det', [1e-200, 0, 0, 1e-200, 0, 0]),
  c('invert', 'nan-det-does-not-throw', [NaN, 0, 0, 1, 0, 0]),
  c('invert', 'infinite', [Infinity, 0, 0, 1, 0, 0]),
  c('invert', 'tiny-but-invertible', [1e-150, 0, 0, 1e-150, 1, 1]),
  c('invert', 'negative-det', [0, 1, 1, 0, 2, 3]),

  c('applyPoint', 'translate', [1, 0, 0, 1, 5, -3], pt(1, 2)),
  c('applyPoint', 'scale', [2, 0, 0, 3, 0, 0], pt(4, 5)),
  c('applyPoint', 'every-slot', [2, 3, 5, 7, 11, 13], pt(17, 19)),
  c('applyPoint', 'skew', skew, pt(-3.5, 8.25)),
  c('applyPoint', 'nan-point', m1, pt(NaN, 1)),
  c('applyPoint', 'infinite-point', m2, pt(Infinity, 1)),

  c('applyRect', 'translate', [1, 0, 0, 1, 5, -3], rc(1, 2, 3, 4)),
  c('applyRect', 'quarter-turn', m2, rc(0, 0, 100, 200)),
  c('applyRect', 'half-turn', [-1, 0, 0, -1, 0, 0], rc(1, 2, 3, 4)),
  c('applyRect', 'three-quarter-turn', [0, -1, 1, 0, 0, 0], rc(1, 2, 3, 4)),
  c('applyRect', 'flip', m3, rc(-50, -30, 100, 200)),
  c('applyRect', 'shear', shear, rc(0, 0, 10, 10)),
  c('applyRect', 'skew', skew, rc(2, -3, 7, 5)),
  c('applyRect', 'negative-shear', [1, 0.5, -0.5, 1, 0, 0], rc(0, 0, 10, 10)),
  c('applyRect', 'shear-y-only', [1, -0.5, 0, 1, 0, 0], rc(0, 0, 10, 10)),
  c('applyRect', 'inverted-rect', m1, rc(10, 10, -5, -5)),
  c('applyRect', 'nan-rect', m1, rc(0, 0, NaN, 1)),
  c('applyRect', 'nan-y', m1, rc(0, NaN, 1, 1)),
  c('applyRect', 'nan-matrix-offset', [1, 0, 0, 1, 0, NaN], rc(0, 0, 1, 1)),
  c('applyRect', 'infinite', m1, rc(0, 0, Infinity, 1)),

  c('applyQuad', 'quarter-turn', [0, 1, -1, 0, 200, 0], quad(pt(0, 200), pt(100, 200), pt(100, 0), pt(0, 0))),
  c('applyQuad', 'shear', shear, quad(pt(0, 0), pt(10, 0), pt(10, 10), pt(0, 10))),
  c('applyQuad', 'every-slot', [2, 3, 5, 7, 11, 13], quad(pt(1, 2), pt(3, 4), pt(5, 6), pt(7, 8))),

  c('matrixToCss', 'identity', [1, 0, 0, 1, 0, 0]),
  c('matrixToCss', 'quarter-turn', [0, 1, -1, 0, 200, 0]),
  c('matrixToCss', 'mixed', [1.5, -0.25, 3e-7, -0, 1e21, -1234.5]),
  ...cssNumbers.map(([label, n]) => c('matrixToCss', `number-${label}`, [n, -n, n * 3, n / 7, 0, 1])),
];

// ── page matrices and transforms ────────────────────────────────────────────

/** `pageTransform` with its closures exercised on the probes. */
function pageTransformProbe(
  input: Parameters<typeof g.pageTransform>[0],
  probe: { points?: Point[]; rects?: Rect[] } = {},
) {
  const t = g.pageTransform(input);
  const points = probe.points ?? [];
  const rects = probe.rects ?? [];
  return {
    rotation: t.rotation,
    viewWidth: t.viewWidth,
    viewHeight: t.viewHeight,
    contentWidth: t.contentWidth,
    contentHeight: t.contentHeight,
    deviceWidth: t.deviceWidth,
    deviceHeight: t.deviceHeight,
    renderScale: t.renderScale,
    viewScale: t.viewScale,
    baseScale: t.baseScale,
    zoom: t.zoom,
    cssMatrix: t.cssMatrix,
    toPixels: points.map(t.toPixels),
    fromPixels: points.map(t.fromPixels),
    contentToView: points.map(t.contentToView),
    viewToContent: points.map(t.viewToContent),
    contentToViewRect: rects.map(t.contentToViewRect),
    viewToContentRect: rects.map(t.viewToContentRect),
  };
}

/** `pageSpace` with its methods exercised on the probes. */
function pageSpaceProbe(
  crop: g.PdfEdges,
  probe: { points?: Point[]; rects?: Rect[]; edges?: g.PdfEdges[]; quads?: Quad[] } = {},
) {
  const s = g.pageSpace(crop);
  const points = probe.points ?? [];
  const quads = probe.quads ?? [];
  return {
    crop: s.crop,
    width: s.width,
    height: s.height,
    pdfToPage: points.map(s.pdfToPage),
    pageToPdf: points.map(s.pageToPdf),
    pdfRectToPage: (probe.edges ?? []).map(s.pdfRectToPage),
    pageRectToPdf: (probe.rects ?? []).map(s.pageRectToPdf),
    pdfQuadToPage: quads.map(s.pdfQuadToPage),
    pageQuadToPdf: quads.map(s.pageQuadToPdf),
  };
}

const pageFns: Record<string, Fn> = {
  rotateScaleMatrix: g.rotateScaleMatrix,
  pdfToContentMatrix: g.pdfToContentMatrix,
  pageGeometry: g.pageGeometry,
  pageTransform: pageTransformProbe,
  pageSpace: pageSpaceProbe,
  edgesOverlap: g.edgesOverlap,
  edgesOfQuad: g.edgesOfQuad,
};

const probe = {
  points: [pt(0, 0), pt(10, 20), pt(137, 42), pt(610, 791), pt(-5, 3.5)],
  rects: [rc(0, 0, 100, 200), rc(10, 20, 30, 40), rc(-4, 7, 0, 0)],
};
const letter = { width: 612, height: 792 };
const spaceProbe = {
  points: [pt(0, 0), pt(10, 20), pt(-5, 3.5), pt(611, 791)],
  rects: [rc(0, 0, 100, 200), rc(10, 20, 30, 40), rc(-4, 7, 0, 0)],
  edges: [
    { left: 0, bottom: 0, right: 100, top: 200 },
    { left: 10, bottom: 20, right: 40, top: 60 },
    { left: 5, bottom: 5, right: 5, top: 5 },
  ],
  quads: [quad(pt(10, 30), pt(40, 30), pt(10, 20), pt(40, 20)), quad(pt(0, 5), pt(5, 10), pt(10, 5), pt(5, 0))],
};
const crop = { left: 0, bottom: 0, right: 100, top: 200 };

const page: FunctionCase[] = [
  ...[...ROTATIONS, ...ODD_ROTATIONS].map((r) => c('rotateScaleMatrix', `r${r}`, 1.5, 600, 800, r)),
  c('rotateScaleMatrix', 'scale-2-quarter', 2, 1200, 1600, 90),
  c('rotateScaleMatrix', 'zero-scale', 0, 0, 0, 180),
  c('rotateScaleMatrix', 'negative-scale', -1, 10, 20, 270),

  c('pdfToContentMatrix', 'origin', { left: 0, top: 792 }),
  c('pdfToContentMatrix', 'offset', { left: 10, top: 220 }),
  c('pdfToContentMatrix', 'negative', { left: -50, top: 170 }),
  c('pdfToContentMatrix', 'extra-members-ignored', { left: 3, bottom: 1, right: 9, top: 4 }),

  ...ROTATIONS.map((r) => c('pageGeometry', `crop-r${r}`, { crop, rotation: r, userUnit: 1 }, 1)),
  ...ROTATIONS.map((r) =>
    c('pageGeometry', `offset-userunit-r${r}`, { crop: { left: 12, bottom: -8, right: 312, top: 392 }, rotation: r, userUnit: 1.5 }, 1.25),
  ),
  c('pageGeometry', 'negative-origin', { crop: { left: -50, bottom: -30, right: 50, top: 170 }, rotation: 0, userUnit: 1 }, 1),
  c('pageGeometry', 'zoom-1.7-r90', { crop, rotation: 90, userUnit: 1 }, 1.7),
  c('pageGeometry', 'letter-zoom-2-r90', { crop: { left: 0, bottom: 0, right: 612, top: 792 }, rotation: 90, userUnit: 1 }, 2),
  c('pageGeometry', 'odd-rotation', { crop, rotation: 45, userUnit: 1 }, 1),
  c('pageGeometry', 'inverted-crop', { crop: { left: 100, bottom: 200, right: 0, top: 0 }, rotation: 90, userUnit: 1 }, 1),
  c('pageGeometry', 'zero-zoom-throws', { crop, rotation: 0, userUnit: 1 }, 0),
  c('pageGeometry', 'zero-userunit-throws', { crop, rotation: 90, userUnit: 0 }, 2),
  c('pageGeometry', 'zero-area-crop', { crop: { left: 5, bottom: 5, right: 5, top: 5 }, rotation: 180, userUnit: 1 }, 1),
  c('pageGeometry', 'nan-zoom', { crop, rotation: 270, userUnit: 1 }, NaN),

  c('pageTransform', 'identity', { pageSize: { width: 100, height: 200 }, rotation: 0, scale: 1, dpr: 1 }, probe),
  c('pageTransform', 'zoom-2', { pageSize: { width: 100, height: 200 }, rotation: 0, scale: 2, dpr: 1 }, probe),
  c('pageTransform', 'dpr-2', { pageSize: { width: 100, height: 200 }, rotation: 0, scale: 1, dpr: 2 }, probe),
  c('pageTransform', 'r90-dpr-2', { pageSize: { width: 100, height: 200 }, rotation: 90, scale: 1, dpr: 2 }, probe),
  c('pageTransform', 'snaps-device', { pageSize: { width: 100, height: 100 }, rotation: 0, scale: 1.337, dpr: 1 }, probe),
  c('pageTransform', 'engine-width-rule', { pageSize: letter, rotation: 0, scale: 739 / 612, dpr: 1 }, probe),
  ...ROTATIONS.flatMap((r) =>
    [1, 0.5, 1.333].flatMap((s) =>
      [1, 2, 1.25].map((dpr) => c('pageTransform', `letter-r${r}-s${s}-dpr${dpr}`, { pageSize: letter, rotation: r, scale: s, dpr }, probe)),
    ),
  ),
  ...ODD_ROTATIONS.map((r) => c('pageTransform', `odd-rotation-${r}`, { pageSize: letter, rotation: r, scale: 1, dpr: 1 }, probe)),
  c('pageTransform', 'base-scale-96-72', { pageSize: letter, rotation: 0, scale: 96 / 72, baseScale: 96 / 72, dpr: 1 }, probe),
  c('pageTransform', 'base-scale-userunit-5', { pageSize: letter, rotation: 0, scale: (96 / 72) * 5, baseScale: (96 / 72) * 5, dpr: 1 }),
  c('pageTransform', 'base-scale-200-percent', { pageSize: letter, rotation: 90, scale: (96 / 72) * 10, baseScale: (96 / 72) * 5, dpr: 1 }),
  c('pageTransform', 'base-scale-default', { pageSize: letter, rotation: 0, scale: 0.42, dpr: 2 }),
  c('pageTransform', 'base-scale-null', { pageSize: letter, rotation: 0, scale: 0.42, baseScale: null, dpr: 2 }),
  c('pageTransform', 'base-scale-zero', { pageSize: letter, rotation: 0, scale: 1, baseScale: 0, dpr: 1 }),
  c('pageTransform', 'base-scale-negative', { pageSize: letter, rotation: 0, scale: 1, baseScale: -2, dpr: 1 }),
  c('pageTransform', 'base-scale-nan', { pageSize: letter, rotation: 0, scale: 1, baseScale: NaN, dpr: 1 }),
  c('pageTransform', 'base-scale-tiny', { pageSize: letter, rotation: 0, scale: 1, baseScale: 5e-324, dpr: 1 }),
  c('pageTransform', 'scale-zero-clamps-device', { pageSize: letter, rotation: 90, scale: 0, dpr: 1 }, probe),
  c('pageTransform', 'scale-negative', { pageSize: letter, rotation: 180, scale: -1, dpr: 1 }, probe),
  c('pageTransform', 'tiny-page', { pageSize: { width: 0.3, height: 0.2 }, rotation: 270, scale: 1, dpr: 1 }, probe),
  c('pageTransform', 'device-tie-rounds-up', { pageSize: { width: 5, height: 3 }, rotation: 0, scale: 0.5, dpr: 1 }, probe),
  c('pageTransform', 'zero-width-page', { pageSize: { width: 0, height: 100 }, rotation: 0, scale: 1, dpr: 1 }, probe),
  c('pageTransform', 'zero-dpr', { pageSize: letter, rotation: 0, scale: 1, dpr: 0 }, probe),
  c('pageTransform', 'infinite-dpr', { pageSize: letter, rotation: 90, scale: 1, dpr: Infinity }, probe),
  c('pageTransform', 'nan-scale', { pageSize: letter, rotation: 0, scale: NaN, dpr: 1 }, probe),
  c('pageTransform', 'singular-throws', { pageSize: { width: 1e300, height: 1e300 }, rotation: 0, scale: 1e-300, dpr: 1 }, probe),
  c('pageTransform', 'no-probes', { pageSize: letter, rotation: 270, scale: 2, dpr: 3 }),
  ...[
    ['letter', { left: 0, bottom: 0, right: 612, top: 792 }],
    ['offset', { left: 12, bottom: -8, right: 312, top: 392 }],
    ['negative-origin', { left: -50, bottom: -30, right: 50, top: 170 }],
    ['inverted', { left: 100, bottom: 200, right: 0, top: 0 }],
    ['zero-area', { left: 5, bottom: 5, right: 5, top: 5 }],
  ].map(([label, box]) => c('pageSpace', label as string, box, spaceProbe)),
  c('pageSpace', 'no-probes', crop),
  c('pageSpace', 'nan-crop', { left: NaN, bottom: 0, right: 10, top: 10 }, spaceProbe),

  c('edgesOverlap', 'overlap', { left: 0, bottom: 0, right: 10, top: 10 }, { left: 5, bottom: 5, right: 15, top: 15 }),
  c('edgesOverlap', 'touching-x', { left: 0, bottom: 0, right: 10, top: 10 }, { left: 10, bottom: 0, right: 20, top: 10 }),
  c('edgesOverlap', 'touching-y', { left: 0, bottom: 0, right: 10, top: 10 }, { left: 0, bottom: 10, right: 10, top: 20 }),
  c('edgesOverlap', 'disjoint-x', { left: 0, bottom: 0, right: 10, top: 10 }, { left: 20, bottom: 0, right: 30, top: 10 }),
  c('edgesOverlap', 'disjoint-y-only', { left: 0, bottom: 0, right: 10, top: 10 }, { left: 2, bottom: 20, right: 8, top: 30 }),
  c('edgesOverlap', 'contained', { left: 0, bottom: 0, right: 10, top: 10 }, { left: 2, bottom: 2, right: 3, top: 3 }),
  c('edgesOverlap', 'nan', { left: NaN, bottom: 0, right: 10, top: 10 }, { left: 0, bottom: 0, right: 10, top: 10 }),

  c('edgesOfQuad', 'axis-aligned', quad(pt(10, 30), pt(40, 30), pt(10, 20), pt(40, 20))),
  c('edgesOfQuad', 'rotated', quad(pt(0, 5), pt(5, 10), pt(10, 5), pt(5, 0))),
  c('edgesOfQuad', 'scattered', quad(pt(5, -1), pt(-2, 7), pt(9, 3), pt(0, -4))),
  c('edgesOfQuad', 'nan', quad(pt(NaN, 0), pt(1, 0), pt(0, 1), pt(1, 1))),

  c('pageTransform', 'huge-page', { pageSize: { width: 14400, height: 14400 }, rotation: 90, scale: 4, dpr: 3 }, probe),
];

// ── text quads ──────────────────────────────────────────────────────────────

const textQuadFns: Record<string, Fn> = {
  textQuadFromRect: g.textQuadFromRect,
  textQuadPoints: g.textQuadPoints,
  textQuadRing: g.textQuadRing,
  textQuadEdge: g.textQuadEdge,
  textQuadEquals: g.textQuadEquals,
  textQuadBounds: g.textQuadBounds,
  applyTextQuad: g.applyTextQuad,
  positionalQuad: g.positionalQuad,
  normalizeQuad: g.normalizeQuad,
};

const cell = g.textQuadFromRect(rc(100, 200, 60, 16));
const tilted = g.applyTextQuad(g.rotate(0.7), cell);
const tq = (us: Point, ue: Point, ls: Point, le: Point): TextQuad => ({ upperStart: us, upperEnd: ue, lowerStart: ls, lowerEnd: le });

const textQuads: FunctionCase[] = [
  c('textQuadFromRect', 'basic', rc(10, 20, 30, 12)),
  c('textQuadFromRect', 'inverted', rc(10, 20, -30, -12)),
  c('textQuadFromRect', 'nan', rc(NaN, 0, 1, 1)),

  c('textQuadPoints', 'cell', cell),
  c('textQuadRing', 'cell', cell),
  c('textQuadEdge', 'start', cell, 'start'),
  c('textQuadEdge', 'end', cell, 'end'),
  c('textQuadEdge', 'tilted-start', tilted, 'start'),
  c('textQuadEdge', 'tilted-end', tilted, 'end'),

  c('textQuadEquals', 'same', cell, g.textQuadFromRect(rc(100, 200, 60, 16))),
  c('textQuadEquals', 'moved-lower-end-x', cell, { ...cell, lowerEnd: pt(161, 216) }),
  c('textQuadEquals', 'moved-lower-end-y', cell, { ...cell, lowerEnd: pt(160, 217) }),
  c('textQuadEquals', 'moved-upper-start-x', cell, { ...cell, upperStart: pt(99, 200) }),
  c('textQuadEquals', 'moved-upper-start-y', cell, { ...cell, upperStart: pt(100, 201) }),
  c('textQuadEquals', 'moved-upper-end', cell, { ...cell, upperEnd: pt(160, 199) }),
  c('textQuadEquals', 'moved-lower-start', cell, { ...cell, lowerStart: pt(101, 216) }),
  c('textQuadEquals', 'rotated-in-place', cell, g.applyTextQuad(g.rotateAbout(pt(130, 208), Math.PI / 6), cell)),
  c('textQuadEquals', 'nan-never-equal', tq(pt(NaN, 0), pt(1, 0), pt(0, 1), pt(1, 1)), tq(pt(NaN, 0), pt(1, 0), pt(0, 1), pt(1, 1))),
  c('textQuadEquals', 'negative-zero-equal', tq(pt(-0, 0), pt(1, 0), pt(0, 1), pt(1, 1)), tq(pt(0, 0), pt(1, 0), pt(0, 1), pt(1, 1))),

  c('textQuadBounds', 'cell', cell),
  c('textQuadBounds', 'tilted', tilted),
  c('textQuadBounds', 'scattered', tq(pt(5, -1), pt(-2, 7), pt(9, 3), pt(0, -4))),
  c('textQuadBounds', 'nan', tq(pt(NaN, 0), pt(1, 0), pt(0, 1), pt(1, 1))),
  c('textQuadBounds', 'nan-y', tq(pt(0, 0), pt(1, 0), pt(0, NaN), pt(1, 1))),

  c('applyTextQuad', 'quarter-turn', [0, 1, -1, 0, 0, 0], g.textQuadFromRect(rc(0, 0, 10, 4))),
  c('applyTextQuad', 'every-slot', [2, 3, 5, 7, 11, 13], cell),

  c('positionalQuad', 'cell', cell),
  c('positionalQuad', 'tilted', tilted),

  // normalizeQuad: zigzag pass-through.
  c('normalizeQuad', 'rect-round-trip', g.positionalQuad(g.textQuadFromRect(rc(10, 20, 30, 12)))),
  c('normalizeQuad', 'rotated-zigzag', quad(pt(50, 10), pt(50, 40), pt(38, 10), pt(38, 40))),
  c('normalizeQuad', 'tilted-zigzag', g.positionalQuad(tilted)),
  c('normalizeQuad', 'upside-down-zigzag', g.positionalQuad(g.applyTextQuad(g.rotate(Math.PI), cell))),
  c('normalizeQuad', 'mirrored-zigzag', quad(pt(10, 0), pt(0, 0), pt(10, 5), pt(0, 5))),
  // Ring order, repaired.
  c('normalizeQuad', 'ring-order', quad(pt(10, 20), pt(40, 20), pt(40, 32), pt(10, 32))),
  c('normalizeQuad', 'ring-order-tilted', quad(tilted.upperStart, tilted.upperEnd, tilted.lowerEnd, tilted.lowerStart)),
  // Each zigzag rejection, one at a time.
  c('normalizeQuad', 'non-finite-x', quad(pt(NaN, 0), pt(10, 0), pt(0, 10), pt(10, 10))),
  c('normalizeQuad', 'non-finite-y', quad(pt(0, 0), pt(10, Infinity), pt(0, 10), pt(10, 10))),
  c('normalizeQuad', 'non-finite-last', quad(pt(0, 0), pt(10, 0), pt(0, 10), pt(10, -Infinity))),
  c('normalizeQuad', 'zero-upper-edge', quad(pt(0, 0), pt(0, 0), pt(0, 10), pt(10, 10))),
  c('normalizeQuad', 'zero-lower-edge', quad(pt(0, 0), pt(10, 0), pt(5, 10), pt(5, 10))),
  c('normalizeQuad', 'zero-start-side', quad(pt(0, 0), pt(10, 0), pt(0, 0), pt(10, 10))),
  c('normalizeQuad', 'zero-end-side', quad(pt(0, 0), pt(10, 0), pt(0, 10), pt(10, 0))),
  c('normalizeQuad', 'edge-at-epsilon', quad(pt(0, 0), pt(1e-6, 0), pt(0, 10), pt(1e-6, 10))),
  c('normalizeQuad', 'edge-just-above-epsilon', quad(pt(0, 0), pt(1.1e-6, 0), pt(0, 10), pt(1.1e-6, 10))),
  c('normalizeQuad', 'lower-reversed', quad(pt(0, 0), pt(10, 0), pt(10, 10), pt(0, 10))),
  c('normalizeQuad', 'lower-perpendicular', quad(pt(0, 0), pt(10, 0), pt(0, 10), pt(0, 20))),
  c('normalizeQuad', 'sides-opposed', quad(pt(0, 0), pt(10, 0), pt(0, 10), pt(10, -10))),
  c('normalizeQuad', 'sides-perpendicular', quad(pt(0, 0), pt(10, 0), pt(0, 10), pt(20, 0.0000001))),
  c('normalizeQuad', 'collinear', quad(pt(0, 0), pt(10, 0), pt(1, 0), pt(11, 0))),
  c('normalizeQuad', 'end-area-zero', quad(pt(0, 0), pt(10, 0), pt(0, 10), pt(5, 5))),
  c('normalizeQuad', 'winding-mismatch', quad(pt(0, 0), pt(10, 0), pt(0, 10), pt(4, 4))),
  c('normalizeQuad', 'thin-sliver', quad(pt(0, 0), pt(1000, 0), pt(0, 1e-6), pt(1000, 1e-6))),
  c('normalizeQuad', 'thin-sliver-accepted', quad(pt(0, 0), pt(1000, 0), pt(0, 1e-3), pt(1000, 1e-3))),
  // The geometric fallback.
  c('normalizeQuad', 'garbage-self-intersecting', quad(pt(0, 0), pt(10, 12), pt(10, 0), pt(0, 12))),
  c('normalizeQuad', 'all-one-point', quad(pt(3, 3), pt(3, 3), pt(3, 3), pt(3, 3))),
  c('normalizeQuad', 'all-nan', quad(pt(NaN, NaN), pt(NaN, NaN), pt(NaN, NaN), pt(NaN, NaN))),
  c('normalizeQuad', 'one-nan-becomes-origin', quad(pt(NaN, 5), pt(10, 12), pt(10, 0), pt(0, 12))),
  c('normalizeQuad', 'infinite-becomes-origin', quad(pt(Infinity, 5), pt(10, 12), pt(10, 0), pt(-Infinity, 12))),
  c('normalizeQuad', 'start-first-false', quad(pt(-9, 1), pt(10, 1.5), pt(-10, -1), pt(10, 1.6))),
  c('normalizeQuad', 'duplicate-corners', quad(pt(0, 0), pt(10, 0), pt(0, 0), pt(0, 10))),
  c('normalizeQuad', 'two-pairs', quad(pt(0, 0), pt(5, 5), pt(0, 0), pt(5, 5))),
  c('normalizeQuad', 'triangle', quad(pt(0, 0), pt(10, 0), pt(5, 8), pt(5, 8))),
  c('normalizeQuad', 'collinear-horizontal', quad(pt(0, 0), pt(3, 0), pt(1, 0), pt(2, 0))),
  c('normalizeQuad', 'collinear-vertical', quad(pt(0, 0), pt(0, 3), pt(0, 1), pt(0, 2))),
  c('normalizeQuad', 'huge-coordinates', quad(pt(1e308, 1e308), pt(1.5e308, 1e308), pt(1e308, 1.7e308), pt(-1e308, 1e308))),
  c('normalizeQuad', 'diamond-bowtie', quad(pt(0, -5), pt(0, 5), pt(5, 0), pt(-5, 0))),
  c('normalizeQuad', 'tie-on-top-edge', quad(pt(0, 0), pt(10, 10), pt(10, 0), pt(0, 10))),
  c('normalizeQuad', 'tie-rotated-square', quad(pt(0, -1), pt(0, 1), pt(1, 0), pt(-1, 0))),
];

/*
 * normalizeQuad near each of `zigzagWellFormed`'s thresholds, one check
 * decisive at a time. An upright quad the checks wrongly reject still comes
 * out of the geometric fallback with the zigzag's own labeling, so these
 * bases are turned and moved off the origin, where the fallback labels them
 * differently and a wrong verdict shows.
 */
const q4 = (a: [number, number], b: [number, number], c: [number, number], d: [number, number]) =>
  quad(pt(...a), pt(...b), pt(...c), pt(...d));
const thresholdBases: [string, Quad][] = [
  ...[5e-7, 9e-7, 1.5e-6].flatMap((t): [string, Quad][] => [
    [`only-upper-short-${t}`, q4([0, 0], [t, 0], [0, 10], [10, 10])],
    [`only-lower-short-${t}`, q4([0, 0], [10, 0], [0, 10], [t, 10])],
    [`only-start-short-${t}`, q4([0, 0], [10, 0], [0, t], [10, 10])],
    [`only-end-short-${t}`, q4([0, 0], [10, 0], [0, 10], [10, t])],
  ]),
  ['diagonal-upper-below-epsilon', q4([0, 0], [7e-7, 7e-7], [0, 10], [10, 10])],
  ['diagonal-upper-above-epsilon', q4([0, 0], [7.2e-7, 7.2e-7], [0, 10], [10, 10])],
  ...[5e-7, 2e-6].map((t): [string, Quad] => [`start-area-${t}`, q4([0, 0], [100, 0], [10, 10 * t], [120, 50])]),
  ...[-1.3e-4, -3e-5, 3e-5, 1.3e-4].map((d): [string, Quad] => [
    `end-area-${d}`,
    q4([0, 0], [100, 0], [-20, 50], [40 + (d * 50) / 130, 25 + (d * 120) / 130]),
  ]),
  ...[-0.1, 0.1].map((e): [string, Quad] => [`upper-lower-dot-${e}`, q4([0, 0], [10, 0], [0, 10], [e, 20])]),
  ...[-0.1, 0.1].map((e): [string, Quad] => [`side-dot-${e}`, q4([0, 0], [10, 0], [0, 10], [20, e])]),
  ['winding-start-positive', q4([0, 0], [10, 0], [0, 10], [4, 4])],
  ['winding-start-negative', q4([0, 0], [-10, 0], [0, 10], [-4, 4])],
  ['mirrored-both-negative', q4([10, 0], [0, 0], [10, 5], [0, 5])],
  ['mirrored-trapezoid', q4([10, 0], [0, 0], [12, 6], [-3, 6])],
  ['garbage', q4([0, 0], [10, 12], [10, 0], [0, 12])],
  ['kite', q4([0, 0], [6, 1], [1, 7], [9, 9])],
];
const turns: [string, (p: Point) => Point][] = [
  ['r90', (p) => pt(-p.y + 37, p.x - 12)],
  ['r180', (p) => pt(-p.x - 5, -p.y + 80)],
  ['r270', (p) => pt(p.y - 64, -p.x + 3)],
  ['tilt-0.5', (p) => g.applyPoint(g.compose(g.translate(37, -12), g.rotate(0.5)), p)],
  ['tilt-2', (p) => g.applyPoint(g.compose(g.translate(-21, 45), g.rotate(2)), p)],
  ['tilt--2.5', (p) => g.applyPoint(g.compose(g.translate(8, 8), g.rotate(-2.5)), p)],
];
const mapQuad = (q: Quad, f: (p: Point) => Point): Quad => quad(f(q.p1), f(q.p2), f(q.p3), f(q.p4));
textQuads.push(
  ...thresholdBases.flatMap(([label, base]) => [
    c('normalizeQuad', `${label}-upright`, base),
    ...turns.map(([turn, f]) => c('normalizeQuad', `${label}-${turn}`, mapQuad(base, f))),
  ]),
  // Finite corners whose differences overflow: the areas are NaN, and
  // Math.sign(NaN) matches nothing.
  // Edge midpoints near the top of the double range: the fallback's
  // midpoint is a half-sum, which does not overflow where a sum times two
  // would, so the highest edge is still told apart.
  c('normalizeQuad', 'midpoints-near-overflow', q4([0, 5e307], [0, 6e307], [0, 4.6e307], [0, 4.7e307])),
  c('normalizeQuad', 'nan-areas', q4([-1e308, -1e308], [1e308, 1e308], [-1e308, -9e307], [1e308, 1.1e308])),
);

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'core/geometry-basics.json',
    describes: 'packages/core/geometry: rect bounds, intersection, corners, containment and overlap, quarter-turn rotations, device rounding.',
    functions: basicsFns,
    cases: basics,
  },
  {
    kind: 'function',
    file: 'core/geometry-affine.json',
    describes: 'packages/core/geometry: Mat2D compose, invert, apply*, builders and matrixToCss.',
    functions: affineFns,
    cases: affine,
  },
  {
    kind: 'function',
    file: 'core/geometry-page.json',
    describes: 'packages/core/geometry: rotateScaleMatrix, pageGeometry, pageTransform and pageSpace (methods exercised on probes), PdfEdges helpers.',
    functions: pageFns,
    cases: page,
  },
  {
    kind: 'function',
    file: 'core/geometry-text-quad.json',
    describes: 'packages/core/geometry: TextQuad helpers and normalizeQuad.',
    functions: textQuadFns,
    cases: textQuads,
  },
];
