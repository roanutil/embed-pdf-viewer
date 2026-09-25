/**
 * `packages/core/annotation`: the geometry layer (`geometry.ts`,
 * `endings.ts`, `cloudy.ts`). Replayed by
 * `crates/plugins/annotation-core/tests/conformance.rs`.
 *
 * Most functions take a `Geom`, so the per-geom functions are crossed with
 * `GEOMS` (every variant, rotated and degenerate ones included). `geomHit`
 * answers one point at a time; its wrapper `geomHitProbe` asks a list of
 * probe points and returns the answers, so one case pins a whole grid.
 */
import * as cl from '../../../../packages/core/annotation/src/cloudy';
import * as en from '../../../../packages/core/annotation/src/endings';
import * as g from '../../../../packages/core/annotation/src/geometry';
import type { Border, Geom, Rect, Vec } from '../../../../packages/core/annotation/src/types';

import type { FunctionCase, Suite } from '../vectors';
import {
  DASHED,
  ENDINGS,
  GEOMS,
  SOLID,
  c,
  cloudy,
  ends,
  gCaret,
  gInk,
  gLine,
  gPoly,
  gQuads,
  gRect,
  gText,
  geom,
  rc,
  tq,
  tqRot,
  v,
  type Fn,
} from './annotation-core-fixtures';

const HANDLES = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'] as const;
const ROTS = [0, 90, 180, 270];
const ODD_ROTS = [30, 45, -90, -45, 360, 405, 179.5, 1e-9];

// ── basics: constants, small math, rotation helpers, placement helpers ───────

const basicsFns: Record<string, Fn> = {
  RECT_HANDLES: () => g.RECT_HANDLES,
  ROTATE_KNOB_OFFSET: () => g.ROTATE_KNOB_OFFSET,
  DEFAULT_CHROME_GEOM: () => g.DEFAULT_CHROME_GEOM,
  MITER_LIMIT: () => g.MITER_LIMIT,
  rotatedHandleCursor: g.rotatedHandleCursor,
  rectFromPoints: g.rectFromPoints,
  rectsIntersect: g.rectsIntersect,
  unionRect: g.unionRect,
  normalizeDeg: g.normalizeDeg,
  rotatePoint: g.rotatePoint,
  apSizeChanged: g.apSizeChanged,
  rotatedAabb: g.rotatedAabb,
  uprightRotation: g.uprightRotation,
  transposedAboutCenter: g.transposedAboutCenter,
  uprightAnchoredRect: g.uprightAnchoredRect,
  fitStampBox: g.fitStampBox,
  rectHandlesFor: g.rectHandlesFor,
  groupResizeAnchor: g.groupResizeAnchor,
  groupResizeBox: g.groupResizeBox,
  groupResizeFactors: g.groupResizeFactors,
  rotateKnob: g.rotateKnob,
  chordThrough: g.chordThrough,
  placeRotateKnob: g.placeRotateKnob,
  shapeRectFor: g.shapeRectFor,
  pointInQuad: g.pointInQuad,
  quadIntersectsRect: g.quadIntersectsRect,
};

type Corners = [Vec, Vec, Vec, Vec];
const corners = (r: Rect): Corners => [
  v(r.x, r.y),
  v(r.x + r.width, r.y),
  v(r.x + r.width, r.y + r.height),
  v(r.x, r.y + r.height),
];
const turned = (r: Rect, deg: number): Corners => {
  const cx = r.x + r.width / 2;
  const cy = r.y + r.height / 2;
  return corners(r).map((p) => g.rotatePoint(p, v(cx, cy), deg)) as Corners;
};

const PAGE = rc(0, 0, 600, 800);

const basics: FunctionCase[] = [
  c('RECT_HANDLES', 'order'),
  c('ROTATE_KNOB_OFFSET', 'value'),
  c('DEFAULT_CHROME_GEOM', 'value'),
  c('MITER_LIMIT', 'value'),
  ...HANDLES.flatMap((h) =>
    [0, 22.4, 22.5, 45, 90, 135, 180, 225, 270, 315, 337.6, -30, 720, 1000].map((r) =>
      c('rotatedHandleCursor', `${h}-${r}`, h, r),
    ),
  ),

  c('rectFromPoints', 'ordered', v(1, 2), v(5, 9)),
  c('rectFromPoints', 'reversed', v(5, 9), v(1, 2)),
  c('rectFromPoints', 'same', v(3, 3), v(3, 3)),
  c('rectFromPoints', 'mixed', v(10, 0), v(0, 10)),
  c('rectFromPoints', 'nan', v(NaN, 0), v(0, 10)),

  c('rectsIntersect', 'overlap', rc(0, 0, 10, 10), rc(5, 5, 10, 10)),
  c('rectsIntersect', 'touching', rc(0, 0, 10, 10), rc(10, 10, 5, 5)),
  c('rectsIntersect', 'apart-x', rc(0, 0, 10, 10), rc(11, 0, 5, 5)),
  c('rectsIntersect', 'apart-x-before', rc(20, 0, 10, 10), rc(0, 0, 5, 5)),
  c('rectsIntersect', 'apart-y', rc(0, 0, 10, 10), rc(0, 11, 5, 5)),
  c('rectsIntersect', 'apart-y-before', rc(0, 20, 10, 10), rc(0, 0, 5, 5)),
  c('rectsIntersect', 'contained', rc(0, 0, 100, 100), rc(10, 10, 5, 5)),
  c('rectsIntersect', 'zero-size-inside', rc(0, 0, 10, 10), rc(5, 5, 0, 0)),

  c('unionRect', 'empty', []),
  c('unionRect', 'one', [v(3, 4)]),
  c('unionRect', 'many', [v(3, 4), v(-1, 10), v(7, -2)]),
  c('unionRect', 'nan', [v(3, 4), v(NaN, 1)]),
  c('unionRect', 'infinite', [v(-Infinity, 0), v(1, Infinity)]),

  ...[0, 90, -90, 360, 450, -450, 359.999, -0.5, 1e-12, 720.25, NaN, Infinity].map((d) =>
    c('normalizeDeg', `${d}`, d),
  ),

  ...[0, 30, 90, 180, 270, -45, 400].map((d) => c('rotatePoint', `about-origin-${d}`, v(10, 0), v(0, 0), d)),
  c('rotatePoint', 'about-pivot', v(15, 20), v(10, 10), 90),
  c('rotatePoint', 'at-pivot', v(10, 10), v(10, 10), 77),

  c('apSizeChanged', 'moved-rect', gRect(rc(0, 0, 10, 10)), gRect(rc(50, 50, 10, 10))),
  c('apSizeChanged', 'resized-rect', gRect(rc(0, 0, 10, 10)), gRect(rc(0, 0, 10.02, 10))),
  c('apSizeChanged', 'noise-rect', gRect(rc(0, 0, 10, 10)), gRect(rc(0, 0, 10.005, 10.009))),
  c('apSizeChanged', 'height-only', gRect(rc(0, 0, 10, 10)), gRect(rc(0, 0, 10, 10.5))),
  c('apSizeChanged', 'text', gText(rc(0, 0, 10, 10)), gText(rc(3, 3, 10, 10))),
  c('apSizeChanged', 'caret', gCaret(rc(0, 0, 4, 4)), gCaret(rc(0, 0, 5, 4))),
  c('apSizeChanged', 'line-moved', gLine(v(0, 0), v(10, 10)), gLine(v(5, 5), v(15, 15))),
  c('apSizeChanged', 'line-stretched', gLine(v(0, 0), v(10, 10)), gLine(v(0, 0), v(20, 10))),
  c('apSizeChanged', 'poly', gPoly([v(0, 0), v(10, 0), v(5, 5)], true), gPoly([v(0, 0), v(10, 0), v(5, 9)], true)),
  c('apSizeChanged', 'ink', gInk([[v(0, 0), v(4, 4)]]), gInk([[v(1, 1), v(5, 5)]])),
  c('apSizeChanged', 'quads', gQuads([tq(0, 0, 10, 5)]), gQuads([tq(0, 0, 12, 5)])),
  c('apSizeChanged', 'rect-to-line', gRect(rc(0, 0, 10, 10)), gLine(v(0, 0), v(10, 10))),

  ...[0, 30, 90, 45, -90].map((d) => c('rotatedAabb', `deg-${d}`, rc(10, 20, 40, 20), d)),
  c('rotatedAabb', 'nan', rc(10, 20, 40, 20), NaN),

  ...[...ROTS, ...ODD_ROTS].map((r) => c('uprightRotation', `${r}`, r)),

  c('transposedAboutCenter', 'landscape', rc(10, 20, 40, 20)),
  c('transposedAboutCenter', 'square', rc(0, 0, 5, 5)),
  c('transposedAboutCenter', 'zero', rc(3, 3, 0, 8)),

  ...[...ROTS, 45, -90, 450].map((r) => c('uprightAnchoredRect', `r${r}`, v(100, 200), 180, 40, r)),

  c('fitStampBox', 'fits', v(300, 400), { width: 100, height: 50 }, { width: 600, height: 800 }, 0),
  c('fitStampBox', 'too-wide', v(300, 400), { width: 1200, height: 100 }, { width: 600, height: 800 }, 0),
  c('fitStampBox', 'too-tall', v(300, 400), { width: 100, height: 1600 }, { width: 600, height: 800 }, 0),
  c('fitStampBox', 'quarter-transposes', v(300, 400), { width: 1000, height: 100 }, { width: 600, height: 800 }, 90),
  c('fitStampBox', 'quarter-270', v(300, 400), { width: 700, height: 100 }, { width: 600, height: 800 }, 270),
  c('fitStampBox', 'negative-quarter', v(300, 400), { width: 700, height: 100 }, { width: 600, height: 800 }, -90),
  c('fitStampBox', 'half-turn', v(300, 400), { width: 700, height: 100 }, { width: 600, height: 800 }, 180),
  c('fitStampBox', 'tilted-not-quarter', v(300, 400), { width: 700, height: 100 }, { width: 600, height: 800 }, 45),
  c('fitStampBox', 'clamp-left-top', v(10, 10), { width: 100, height: 50 }, { width: 600, height: 800 }, 0),
  c('fitStampBox', 'clamp-right-bottom', v(595, 799), { width: 100, height: 50 }, { width: 600, height: 800 }, 0),
  c('fitStampBox', 'zero-size', v(300, 400), { width: 0, height: 0 }, { width: 600, height: 800 }, 0),
  c('fitStampBox', 'zero-width', v(300, 400), { width: 0, height: 900 }, { width: 600, height: 800 }, 0),
  c('fitStampBox', 'never-scales-up', v(300, 400), { width: 10, height: 10 }, { width: 600, height: 800 }, 0),

  c('rectHandlesFor', 'box', rc(10, 20, 100, 50)),
  c('rectHandlesFor', 'zero', rc(0, 0, 0, 0)),

  ...[...HANDLES, 'bogus', 'v0', 'toString', '__proto__', 'constructor'].map((h) =>
    c('groupResizeAnchor', h, rc(10, 20, 100, 50), h),
  ),

  ...HANDLES.flatMap((h) => [
    c('groupResizeBox', `${h}-aniso`, rc(10, 20, 100, 50), h, v(150, 100), false),
    c('groupResizeBox', `${h}-iso`, rc(10, 20, 100, 50), h, v(150, 100), true),
    c('groupResizeBox', `${h}-iso-shrink`, rc(10, 20, 100, 50), h, v(20, 25), true),
  ]),
  c('groupResizeBox', 'bogus-handle', rc(10, 20, 100, 50), 'zz', v(150, 100), false),
  c('groupResizeBox', 'bogus-handle-iso', rc(10, 20, 100, 50), 'zz', v(150, 100), true),
  c('groupResizeBox', 'crossing-min-size', rc(10, 20, 100, 50), 'se', v(11, 21), false),
  c('groupResizeBox', 'iso-zero-base', rc(10, 20, 0, 0), 'se', v(50, 50), true),
  c('groupResizeBox', 'iso-zero-width', rc(10, 20, 0, 40), 'e', v(50, 50), true),
  c('groupResizeBox', 'iso-flip', rc(10, 20, 100, 50), 'se', v(-200, -100), true),

  c('groupResizeFactors', 'grow', rc(0, 0, 10, 20), rc(0, 0, 30, 10)),
  c('groupResizeFactors', 'zero-base', rc(0, 0, 0, 0), rc(0, 0, 30, 10)),
  c('groupResizeFactors', 'zero-height', rc(0, 0, 10, 0), rc(0, 0, 30, 10)),

  c('rotateKnob', 'upright', corners(rc(100, 100, 80, 60)), 24),
  c('rotateKnob', 'tilted', turned(rc(100, 100, 80, 60), 30), 24),
  c('rotateKnob', 'flat-box', corners(rc(100, 100, 80, 0)), 24),
  c('rotateKnob', 'zero-offset', corners(rc(100, 100, 80, 60)), 0),

  ...[0, 90, 45, 30, 135, -60, 180].map((d) => c('chordThrough', `inside-${d}`, PAGE, v(300, 400), d)),
  c('chordThrough', 'outside-parallel-x', PAGE, v(300, -10), 0),
  c('chordThrough', 'outside-parallel-y', PAGE, v(-10, 400), 90),
  c('chordThrough', 'outside-parallel-x-below', PAGE, v(300, 900), 0),
  c('chordThrough', 'outside-parallel-y-right', PAGE, v(700, 400), 90),
  c('chordThrough', 'misses-diagonal', PAGE, v(-500, -500), 135),
  c('chordThrough', 'corner-touch', PAGE, v(-100, -100), 45),
  c('chordThrough', 'on-edge', PAGE, v(0, 400), 90),
  c('chordThrough', 'zero-box', rc(10, 10, 0, 0), v(10, 10), 45),
  c('chordThrough', 'nan-angle', PAGE, v(300, 400), NaN),

  c('placeRotateKnob', 'no-page', corners(rc(100, 100, 80, 60)), 24),
  c('placeRotateKnob', 'inside', corners(rc(100, 100, 80, 60)), 24, PAGE),
  c('placeRotateKnob', 'flip-bottom', corners(rc(100, 10, 80, 60)), 24, PAGE),
  c('placeRotateKnob', 'flip-tilted', turned(rc(100, 10, 80, 60), 20), 24, PAGE),
  c('placeRotateKnob', 'clamp', corners(rc(0, 5, 600, 790)), 24, PAGE),
  c('placeRotateKnob', 'clamp-offpage', corners(rc(-100, -100, 50, 950)), 24, PAGE),
  c('placeRotateKnob', 'flat-box-flip', corners(rc(100, 5, 80, 0)), 24, PAGE),
  c('placeRotateKnob', 'flat-box-offpage', corners(rc(100, -5, 80, 0)), 24, PAGE),
  c('placeRotateKnob', 'on-page-edge', corners(rc(100, 24, 80, 60)), 24, PAGE),

  ...[SOLID, DASHED, cloudy(1), cloudy(0), cloudy(2.5)].flatMap((b, i) => [
    c('shapeRectFor', `rect-border${i}`, rc(10, 10, 100, 50), false, { color: '#000', interiorColor: null, strokeWidth: 2, opacity: 1, blendMode: 'normal', border: b }),
    c('shapeRectFor', `ellipse-border${i}`, rc(10, 10, 100, 50), true, { color: '#000', interiorColor: null, strokeWidth: 3, opacity: 1, blendMode: 'normal', border: b }),
  ]),

  c('pointInQuad', 'inside', v(5, 5), corners(rc(0, 0, 10, 10))),
  c('pointInQuad', 'outside', v(15, 5), corners(rc(0, 0, 10, 10))),
  c('pointInQuad', 'left-edge', v(0, 5), corners(rc(0, 0, 10, 10))),
  c('pointInQuad', 'right-edge', v(10, 5), corners(rc(0, 0, 10, 10))),
  c('pointInQuad', 'top-edge', v(5, 0), corners(rc(0, 0, 10, 10))),
  c('pointInQuad', 'bottom-edge', v(5, 10), corners(rc(0, 0, 10, 10))),
  c('pointInQuad', 'tilted-inside', v(140, 130), turned(rc(100, 100, 80, 60), 30)),
  c('pointInQuad', 'tilted-corner-gap', v(101, 101), turned(rc(100, 100, 80, 60), 45)),
  c('pointInQuad', 'degenerate', v(5, 0), corners(rc(0, 0, 10, 0))),

  c('quadIntersectsRect', 'overlap', corners(rc(0, 0, 10, 10)), rc(5, 5, 10, 10)),
  c('quadIntersectsRect', 'touching', corners(rc(0, 0, 10, 10)), rc(10, 0, 10, 10)),
  c('quadIntersectsRect', 'apart', corners(rc(0, 0, 10, 10)), rc(11, 0, 10, 10)),
  c('quadIntersectsRect', 'apart-before', corners(rc(20, 0, 10, 10)), rc(0, 0, 10, 10)),
  c('quadIntersectsRect', 'tilted-miss-in-aabb', turned(rc(100, 100, 100, 10), 45), rc(100, 100, 20, 20)),
  c('quadIntersectsRect', 'tilted-hit', turned(rc(100, 100, 100, 10), 45), rc(140, 90, 20, 20)),
  c('quadIntersectsRect', 'degenerate-quad', corners(rc(5, 5, 0, 0)), rc(0, 0, 10, 10)),
  c('quadIntersectsRect', 'degenerate-quad-outside', corners(rc(50, 50, 0, 0)), rc(0, 0, 10, 10)),
  c('quadIntersectsRect', 'flat-quad', corners(rc(0, 5, 20, 0)), rc(5, 0, 5, 10)),
  c('quadIntersectsRect', 'quad-axis-gap', [v(0, 0), v(10, 10), v(0, 20), v(-10, 10)], rc(8, 0, 4, 4)),
];

// ── per-geom functions, crossed with GEOMS ───────────────────────────────────

const shapeFns: Record<string, Fn> = {
  geomBounds: g.geomBounds,
  geomVisualBounds: g.geomVisualBounds,
  selectionBounds: g.selectionBounds,
  selectionQuad: g.selectionQuad,
  selectionCenter: g.selectionCenter,
  obbFromGeom: g.obbFromGeom,
  centroidOf: g.centroidOf,
  geomRotation: g.geomRotation,
  isRotatableGeom: g.isRotatableGeom,
  geomHandles: g.geomHandles,
  geomScene: g.geomScene,
  geomTranslate: g.geomTranslate,
  geomRotateAbout: g.geomRotateAbout,
  geomResetRotation: g.geomResetRotation,
  geomScaleAbout: g.geomScaleAbout,
  geomDragHandle: g.geomDragHandle,
  calloutLinePoints: g.calloutLinePoints,
};

const BORDERS: [string, Border | undefined][] = [
  ['none', undefined],
  ['solid', SOLID],
  ['dashed', DASHED],
  ['cloudy1', cloudy(1)],
  ['cloudy0', cloudy(0)],
  ['cloudy2.5', cloudy(2.5)],
];

const shapes: FunctionCase[] = [];
for (const [name, gm] of GEOMS) {
  shapes.push(
    c('geomBounds', name, gm),
    c('centroidOf', name, gm),
    c('geomRotation', name, gm),
    c('isRotatableGeom', name, gm),
    c('geomHandles', name, gm),
    c('geomTranslate', name, gm, v(12.5, -7)),
    c('geomResetRotation', name, gm),
    c('selectionCenter', `${name}-sw2`, gm, 2),
    c('geomRotateAbout', `${name}-zero`, gm, v(0, 0), 0),
    c('geomRotateAbout', `${name}-30-origin`, gm, v(0, 0), 30),
    c('geomRotateAbout', `${name}-90-self`, gm, g.centroidOf(gm), 90),
    c('geomRotateAbout', `${name}-neg45`, gm, v(100, 100), -45),
    c('geomScaleAbout', `${name}-iso`, gm, v(0, 0), 2, 2),
    c('geomScaleAbout', `${name}-aniso`, gm, v(50, 50), 0.5, 3),
    c('geomScaleAbout', `${name}-flip`, gm, v(50, 50), -1, 1),
    c('geomScaleAbout', `${name}-tiny`, gm, v(50, 50), 0.01, 0.01),
  );
  for (const sw of [0, 1, 4]) {
    for (const [bn, b] of BORDERS) {
      if (sw !== 4 && bn !== 'none' && bn !== 'cloudy1') continue;
      // A border only changes the answer for boxes and polygons.
      if (bn !== 'none' && gm.t !== 'rect' && gm.t !== 'poly') continue;
      // A cloudy ellipse at zero stroke places its last curl by comparing an
      // accumulated perimeter with no tolerance: the last bit of sin/cos
      // decides, and V8's fdlibm and the platform libm may differ there.
      if (sw === 0 && bn.startsWith('cloudy') && gm.t === 'rect' && gm.ellipse) continue;
      shapes.push(
        c('geomVisualBounds', `${name}-sw${sw}-${bn}`, gm, sw, b),
        c('selectionBounds', `${name}-sw${sw}-${bn}`, gm, sw, b),
        c('selectionQuad', `${name}-sw${sw}-${bn}`, gm, sw, b),
        c('obbFromGeom', `${name}-sw${sw}-${bn}`, gm, sw, b),
        c('geomScene', `${name}-sw${sw}-${bn}`, gm, sw, b),
      );
    }
  }
  if (gm.t === 'text') shapes.push(c('calloutLinePoints', name, gm));
}
shapes.push(c('geomScene', 'rect-default-width', geom('rect')));
shapes.push(c('geomScene', 'cloudy-small-box', gRect(rc(0, 0, 10, 10)), 2, cloudy(2)));
shapes.push(c('geomScene', 'cloudy-just-fits', gRect(rc(0, 0, 11, 30)), 1, cloudy(1)));
shapes.push(c('geomScene', 'cloudy-ellipse-small', gRect(rc(0, 0, 10, 10), true), 2, cloudy(2)));
shapes.push(c('geomScene', 'cloudy-poly-two', gPoly([v(0, 0), v(40, 40)], true), 1, cloudy(1)));
shapes.push(c('geomScene', 'cloudy-open-poly', gPoly([v(0, 0), v(40, 40), v(80, 0)], false), 1, cloudy(1)));
shapes.push(c('geomScene', 'callout-leader-zero', gText(rc(100, 100, 120, 40), { tip: v(220, 120), ending: 'open-arrow' }), 2));
shapes.push(c('geomVisualBounds', 'callout-rot-sw3', gText(rc(100, 100, 120, 40), { tip: v(20, 20), ending: 'closed-arrow' }, 90), 3));

// Drag every handle a geom offers, plus the bogus ones, to one target.
const dragTo = v(210, 185);
for (const [name, gm] of GEOMS) {
  const ids = [...g.geomHandles(gm).map((h) => h.id), 'bogus', 'v', 'v99', 'v-1', 'v1.5', 'x1', 'v 1', 'v0x1', 'v1e0', 'callout-tip', 'callout-knee', 'v0', 'v1'];
  for (const id of [...new Set(ids)]) shapes.push(c('geomDragHandle', `${name}-${id}`, gm, id, dragTo));
}
shapes.push(c('geomDragHandle', 'rect-nw-collapse', geom('rect'), 'nw', v(178, 158)));
shapes.push(c('geomDragHandle', 'rect-rot30-se-shrink', geom('rect-rot30'), 'se', v(120, 110)));
shapes.push(c('geomDragHandle', 'poly-v-empty-string', geom('poly-closed'), 'v', v(0, 0)));
shapes.push(c('geomDragHandle', 'poly-astral', geom('poly-closed'), '\u{1F600}1', v(0, 0)));
shapes.push(c('geomDragHandle', 'poly-unicode-space', geom('poly-closed'), 'v 1', v(0, 0)));
shapes.push(c('geomDragHandle', 'poly-infinity', geom('poly-closed'), 'vInfinity', v(0, 0)));
shapes.push(c('geomDragHandle', 'poly-binary', geom('poly-closed'), 'v0b10', v(0, 0)));
shapes.push(c('geomDragHandle', 'poly-octal', geom('poly-closed'), 'v0o1', v(0, 0)));
shapes.push(c('geomDragHandle', 'poly-trailing-dot', geom('poly-closed'), 'v2.', v(0, 0)));
shapes.push(c('geomDragHandle', 'poly-leading-dot', geom('poly-closed'), 'v.0', v(0, 0)));
shapes.push(c('geomDragHandle', 'poly-plus', geom('poly-closed'), 'v+1', v(0, 0)));
shapes.push(c('geomDragHandle', 'poly-neg-zero', geom('poly-closed'), 'v-0', v(0, 0)));
shapes.push(c('geomDragHandle', 'poly-tabs', geom('poly-closed'), 'v\t2\n', v(0, 0)));
shapes.push(c('geomDragHandle', 'poly-hex-upper', geom('poly-closed'), 'v0X2', v(0, 0)));
shapes.push(c('geomDragHandle', 'poly-bad-hex', geom('poly-closed'), 'v0xg', v(0, 0)));
shapes.push(c('geomDragHandle', 'poly-exp', geom('poly-closed'), 'v2e-0', v(0, 0)));

// ── hit testing ──────────────────────────────────────────────────────────────

/** Probe points around a geom: its bounds' centre, corners, edge midpoints,
 *  and points just inside/outside each edge at the margin boundary. */
function probes(gm: Geom, margin: number): Vec[] {
  const b = g.geomBounds(gm);
  const x0 = b.x;
  const y0 = b.y;
  const x1 = b.x + b.width;
  const y1 = b.y + b.height;
  const mx = (x0 + x1) / 2;
  const my = (y0 + y1) / 2;
  const out: Vec[] = [v(mx, my), v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1), v(mx, y0), v(mx, y1), v(x0, my), v(x1, my)];
  for (const d of [margin - 0.01, margin + 0.01, margin + 1, 3, 1.5]) {
    out.push(v(x0 - d, my), v(x1 + d, my), v(mx, y0 - d), v(mx, y1 + d), v(x0 + d, my), v(mx, y0 + d));
  }
  out.push(v(x0 + b.width / 4, y0 + b.height / 4), v(x1 - 2, y1 - 2), v(-1000, -1000));
  return out;
}

const hitFns: Record<string, Fn> = {
  geomHitProbe: (gm: Geom, margin: number, filled: boolean, sw: number, points: Vec[]) =>
    points.map((p) => g.geomHit(gm, p, margin, filled, sw)),
};

const hits: FunctionCase[] = [];
for (const [name, gm] of GEOMS) {
  for (const [margin, filled, sw] of [
    [6, false, 2],
    [6, true, 2],
    [0, false, 0],
    [2, true, 8],
  ] as const) {
    hits.push(c('geomHitProbe', `${name}-m${margin}-${filled ? 'filled' : 'open'}-sw${sw}`, gm, margin, filled, sw, probes(gm, margin)));
  }
}
// Line endings as hit targets (an arrowhead is as clickable as the stroke).
for (const e of ENDINGS) {
  const gm = gLine(v(100, 100), v(300, 100), ends(e, e));
  const points = [v(100, 100), v(90, 100), v(85, 96), v(92, 104), v(80, 100), v(310, 100), v(315, 104), v(300, 118), v(88, 112), v(112, 88)];
  hits.push(c('geomHitProbe', `line-ending-${e}-sw3`, gm, 1, false, 3, points));
  hits.push(c('geomHitProbe', `line-ending-${e}-sw0`, gm, 1, false, 0, points));
  const cg = gText(rc(200, 200, 120, 40), { tip: v(100, 100), ending: e });
  hits.push(c('geomHitProbe', `callout-ending-${e}-sw3`, cg, 1, false, 3, [...points, v(150, 150), v(200, 220)]));
}
hits.push(c('geomHitProbe', 'poly-open-ending', gPoly([v(0, 0), v(100, 0), v(100, 100)], false, ends('closed-arrow', 'diamond')), 1, false, 3, [v(-10, 0), v(-20, 3), v(100, 110), v(103, 118), v(96, 116)]));
hits.push(c('geomHitProbe', 'poly-closing-edge', gPoly([v(10, 10), v(90, 20), v(40, 80)], true), 1, false, 1, [v(25, 45), v(24, 46), v(20, 50)]));
hits.push(c('geomHitProbe', 'poly-closed-two-points', gPoly([v(0, 0), v(100, 0)], true), 1, true, 1, [v(50, 0), v(50, 5)]));
hits.push(c('geomHitProbe', 'ellipse-thick-stroke', gRect(rc(0, 0, 10, 10), true), 0, false, 30, [v(5, 5), v(0, 5), v(20, 5)]));
hits.push(c('geomHitProbe', 'rect-rot-local-frame', gRect(rc(100, 100, 80, 60), false, 45), 1, true, 2, [v(140, 130), v(100, 100), v(181, 130), v(140, 80)]));
hits.push(c('geomHitProbe', 'caret-rot', gCaret(rc(100, 100, 10, 10), 45), 1, false, 1, [v(105, 105), v(100, 100), v(92, 105)]));
hits.push(c('geomHitProbe', 'callout-rot-box-local', gText(rc(100, 100, 120, 40), { tip: v(20, 20), ending: 'none' }, 90), 1, false, 2, [v(160, 120), v(145, 60), v(175, 180), v(110, 105), v(60, 70)]));
hits.push(c('geomHitProbe', 'quads-rotated-inside', gQuads([tqRot(40, 40, 80, 10, 30)]), 0, true, 0, [v(60, 55), v(40, 40), v(100, 45), v(90, 80)]));

// ── text anchors: carets, callout connection, PDF bridge ─────────────────────

const textFns: Record<string, Fn> = {
  calloutConnection: g.calloutConnection,
  caretRectFromTextEnd: g.caretRectFromTextEnd,
  caretRectFromAnchor: g.caretRectFromAnchor,
  caretGeomFromAnchor: g.caretGeomFromAnchor,
  pdfToContentPoint: g.pdfToContentPoint,
  contentToPdfPoint: g.contentToPdfPoint,
  pdfToContentRect: g.pdfToContentRect,
  contentToPdfRect: g.contentToPdfRect,
  geomPdfBounds: g.geomPdfBounds,
};
const BOX = rc(100, 100, 120, 40);
const CROP = { left: 0, bottom: 0, right: 612, top: 792 };
const OFFSET_CROP = { left: 36, bottom: -18, right: 576, top: 720 };
const text: FunctionCase[] = [
  ...[v(300, 120), v(20, 120), v(160, 300), v(160, 10), v(160, 120), v(220, 160), v(100, 100), v(0, 0), v(400, 400), v(160, 121)].flatMap((ref, i) => [
    c('calloutConnection', `ref${i}`, BOX, ref),
    c('calloutConnection', `ref${i}-rot90`, BOX, ref, 90),
    c('calloutConnection', `ref${i}-rot30`, BOX, ref, 30),
  ]),
  c('calloutConnection', 'rot-zero-explicit', BOX, v(300, 120), 0),
  c('caretRectFromTextEnd', 'line', rc(10, 20, 100, 12)),
  c('caretRectFromTextEnd', 'zero', rc(0, 0, 0, 0)),
  ...[
    ['upright-forward', tq(10, 10, 50, 12), 1],
    ['upright-backward', tq(10, 10, 50, 12), -1],
    ['tiny-glyph', tq(10, 10, 1, 1), 1],
    ['rot90', tqRot(100, 100, 50, 12, 90), 1],
    ['rot30-back', tqRot(100, 100, 50, 12, 30), -1],
    ['rot-near-upright', tqRot(100, 100, 50, 12, 0.04), 1],
    ['rot-near-360', tqRot(100, 100, 50, 12, -0.04), 1],
    ['rot-just-past', tqRot(100, 100, 50, 12, 0.06), 1],
    ['rot180', tqRot(100, 100, 50, 12, 180), 1],
    ['zero-baseline', { upperStart: v(10, 0), upperEnd: v(10, 0), lowerStart: v(10, 12), lowerEnd: v(10, 12) }, 1],
    ['zero-baseline-and-ink', { upperStart: v(10, 12), upperEnd: v(10, 12), lowerStart: v(10, 12), lowerEnd: v(10, 12) }, 1],
  ].flatMap(([label, q, adv]) => [
    c('caretRectFromAnchor', label as string, { glyphQuad: q, advance: adv }),
    c('caretGeomFromAnchor', label as string, { glyphQuad: q, advance: adv }),
  ]),
  ...[v(0, 0), v(100, 700), v(36.5, -18)].flatMap((p, i) => [
    c('pdfToContentPoint', `p${i}`, p, CROP),
    c('pdfToContentPoint', `p${i}-offset`, p, OFFSET_CROP),
    c('contentToPdfPoint', `p${i}`, p, CROP),
    c('contentToPdfPoint', `p${i}-offset`, p, OFFSET_CROP),
  ]),
  c('pdfToContentRect', 'box', { left: 100, bottom: 600, right: 200, top: 700 }, CROP),
  c('pdfToContentRect', 'box-offset', { left: 100, bottom: 600, right: 200, top: 700 }, OFFSET_CROP),
  c('pdfToContentRect', 'inverted', { left: 200, bottom: 700, right: 100, top: 600 }, CROP),
  c('contentToPdfRect', 'box', rc(100, 92, 100, 100), CROP),
  c('contentToPdfRect', 'box-offset', rc(100, 92, 100, 100), OFFSET_CROP),
  ...GEOMS.filter((_, i) => i % 3 === 0).map(([name, gm]) => c('geomPdfBounds', name, gm, 2, OFFSET_CROP)),
  c('geomPdfBounds', 'cloudy-poly', geom('poly-closed'), 2, CROP, cloudy(1)),
];

// ── endings ──────────────────────────────────────────────────────────────────

const endingFns: Record<string, Fn> = {
  endingPoints: en.endingPoints,
  endingNodes: en.endingNodes,
};
const endingCases: FunctionCase[] = [];
for (const e of [...ENDINGS, undefined]) {
  for (const [angle, sw] of [
    [0, 1],
    [Math.PI / 2, 2],
    [-2.5, 3],
    [Math.PI, 0],
  ]) {
    const label = `${e ?? 'undefined'}-a${angle.toFixed(3)}-sw${sw}`;
    endingCases.push(c('endingPoints', label, v(50, 60), angle, e, sw));
    endingCases.push(c('endingNodes', label, v(50, 60), angle, e, sw));
  }
}

// ── cloudy borders ───────────────────────────────────────────────────────────

const cloudyFns: Record<string, Fn> = {
  cloudyBorderExtent: cl.cloudyBorderExtent,
  cloudyPath: cl.cloudyPath,
  cloudyPolyPath: cl.cloudyPolyPath,
};
const INTENSITIES = [0, 0.5, 1, 1.3, 2, 3, -1];
const cloudyCases: FunctionCase[] = [
  ...INTENSITIES.flatMap((i) => [
    c('cloudyBorderExtent', `rect-i${i}`, i, 2, false),
    c('cloudyBorderExtent', `ellipse-i${i}`, i, 2, true),
  ]),
  c('cloudyBorderExtent', 'zero-stroke', 1, 0, false),
];
const CLOUD_BOXES: [string, Rect][] = [
  ['box', rc(10, 20, 200, 100)],
  ['square', rc(0, 0, 60, 60)],
  ['small', rc(0, 0, 30, 25)],
  ['tiny', rc(5, 5, 4, 4)],
  ['wide-thin', rc(0, 0, 300, 24)],
  ['tall-thin', rc(0, 0, 22, 300)],
  ['big', rc(-50, -50, 500, 420)],
  ['zero', rc(0, 0, 0, 0)],
  ['fraction', rc(0.3, 0.7, 77.77, 33.33)],
];
for (const [name, box] of CLOUD_BOXES) {
  for (const i of INTENSITIES) {
    for (const sw of name === 'box' || name === 'small' || name === 'tiny' ? [1, 3] : [1]) {
      cloudyCases.push(c('cloudyPath', `${name}-rect-i${i}-sw${sw}`, box, false, i, sw));
      cloudyCases.push(c('cloudyPath', `${name}-ellipse-i${i}-sw${sw}`, box, true, i, sw));
    }
  }
}
// Ellipse corners of the algorithm: thin boxes that stay ellipses, boxes that
// fall back to a plain ellipse because the curl radius comes out small, and
// the long-and-thin rectangle path.
for (const [label, box, i, sw] of [
  ['ellipse-thin-wide-rect-path', rc(0, 0, 400, 30), 0.3, 0.1],
  ['ellipse-thin-tall-rect-path', rc(0, 0, 30, 400), 0.3, 0.1],
  ['ellipse-radius-adjust-narrow', rc(0, 0, 36, 200), 2, 1],
  ['ellipse-radius-adjust-short', rc(0, 0, 200, 36), 2, 1],
  ['ellipse-few-curls', rc(0, 0, 40, 40), 3, 1],
  ['ellipse-small-radius', rc(0, 0, 90, 90), 0.1, 0.1],
  ['ellipse-mid', rc(0, 0, 120, 80), 0.6, 1],
  ['ellipse-huge-stroke', rc(0, 0, 120, 80), 1, 20],
  ['rect-zero-stroke', rc(0, 0, 120, 80), 1, 0],
  ['ellipse-tiny-intensity', rc(0, 0, 400, 300), 0.01, 0.2],
  ['ellipse-narrow-short', rc(0, 0, 33.5, 45.5), 3, 1],
  ['ellipse-short-narrow', rc(0, 0, 45.5, 33.5), 3, 1],
  ['ellipse-narrow-i5', rc(0, 0, 55, 70), 5, 1],
  ['ellipse-both-narrow-i5', rc(0, 0, 55, 58), 5, 1],
  ['ellipse-two-curls', rc(0, 0, 42, 42), 3, 1],
  ['ellipse-two-curls-b', rc(0, 0, 44, 42), 3, 1],
  ['rect-tiny-intensity', rc(0, 0, 400, 300), 0.01, 0],
] as const) {
  if (sw > 0) cloudyCases.push(c('cloudyPath', `${label}-ellipse`, box, true, i, sw));
  cloudyCases.push(c('cloudyPath', `${label}-rect`, box, false, i, sw));
}
const POLYS: [string, Vec[]][] = [
  ['triangle', [v(10, 10), v(90, 20), v(40, 80)]],
  ['square', [v(0, 0), v(100, 0), v(100, 100), v(0, 100)]],
  ['closed-ring', [v(0, 0), v(100, 0), v(50, 80), v(0, 0)]],
  ['clockwise', [v(0, 0), v(0, 100), v(100, 100), v(100, 0)]],
  ['zero-length-segments', [v(0, 0), v(0.2, 0.1), v(100, 0), v(100, 0), v(50, 80)]],
  ['short-edges', [v(0, 0), v(3, 0), v(3, 3), v(0, 3)]],
  ['two', [v(0, 0), v(50, 50)]],
  ['one', [v(5, 5)]],
  ['empty', []],
  ['star', [v(50, 0), v(61, 35), v(98, 35), v(68, 57), v(79, 91), v(50, 70), v(21, 91), v(32, 57), v(2, 35), v(39, 35)]],
  ['duplicate-last-x', [v(0, 0), v(100, 0), v(0, 1)]],
  ['two-same', [v(5, 5), v(5, 5)]],
];
for (const [name, pts] of POLYS) {
  for (const i of INTENSITIES) {
    cloudyCases.push(c('cloudyPolyPath', `${name}-i${i}`, pts, i, 1));
  }
  cloudyCases.push(c('cloudyPolyPath', `${name}-thick`, pts, 1, 12));
}

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'core/annotation-geometry-basics.json',
    describes: 'packages/core/annotation geometry.ts: constants, rect math, rotation helpers, upright placement, stamp fit, group scaling, rotate knob, guide chords, SAT.',
    functions: basicsFns,
    cases: basics,
  },
  {
    kind: 'function',
    file: 'core/annotation-geometry-shapes.json',
    describes: 'packages/core/annotation geometry.ts: every per-geom function crossed with one of every Geom shape (bounds, visual bounds, selection, OBB, handles, scene, translate, rotate, scale, handle drags).',
    functions: shapeFns,
    cases: shapes,
  },
  {
    kind: 'function',
    file: 'core/annotation-geometry-hit.json',
    describes: 'packages/core/annotation geometry.ts: geomHit over probe grids (wrapper geomHitProbe returns one answer per point).',
    functions: hitFns,
    cases: hits,
  },
  {
    kind: 'function',
    file: 'core/annotation-geometry-text.json',
    describes: 'packages/core/annotation geometry.ts: callout connection, caret boxes from text anchors, and the PDF/content bridge.',
    functions: textFns,
    cases: text,
  },
  {
    kind: 'function',
    file: 'core/annotation-endings.json',
    describes: 'packages/core/annotation endings.ts: /LE ending points and render nodes.',
    functions: endingFns,
    cases: endingCases,
  },
  {
    kind: 'function',
    file: 'core/annotation-cloudy.json',
    describes: 'packages/core/annotation cloudy.ts: cloudy border extent and the rect, ellipse and polygon scallop paths.',
    functions: cloudyFns,
    cases: cloudyCases,
  },
];
