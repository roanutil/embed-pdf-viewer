/**
 * `packages/core/annotation`: the measurement layer (`measurement.ts`,
 * `measurement-shape.ts`, `measurement-font.ts`), the selection frame
 * (`selection.ts`), the rich-text run algebra (`richtext.ts`), and the
 * reducer, hit test, scene and view over measured and rich-text annotations.
 * Replayed by `crates/plugins/annotation-core/tests/conformance.rs`.
 *
 * The model-level file reuses the other suites' wrappers (`runs`, `hitCalls`,
 * `viewMany`, `scene`, `readProps`, `applyProps`), so the Rust function table
 * answers them unchanged. Functions that take a layout take the geometry,
 * appearance and stroke width that build it; functions that take a callback
 * take data (`transformMeasurementCaption` a translation, `rangeHasStyle` a
 * `{key, value}` probe compared as JSON).
 */
import * as ms from '../../../../packages/core/annotation/src/measurement';
import * as mf from '../../../../packages/core/annotation/src/measurement-font';
import * as msh from '../../../../packages/core/annotation/src/measurement-shape';
import * as rt from '../../../../packages/core/annotation/src/richtext';
import * as g from '../../../../packages/core/annotation/src/geometry';
import { annotationSelectionFrame } from '../../../../packages/core/annotation/src/selection';
import { pageItems } from '../../../../packages/core/annotation/src/view';
import { initialModel } from '../../../../packages/core/annotation/src/update';
import type { ViewEnv } from '../../../../packages/core/annotation/src/anchor';
import type { DistanceAppearance, MeasurementAppearance } from '../../../../packages/core/annotation/src/measurement';
import type { ShapeMeasurementAppearance } from '../../../../packages/core/annotation/src/measurement-shape';
import type { Annot, Geom, Model, Msg, PointerInput, Style, Vec } from '../../../../packages/core/annotation/src/types';

import type { FunctionCase, Suite } from '../vectors';
import {
  DASHED,
  PON,
  annot,
  c,
  ends,
  flags,
  gLine,
  gPoly,
  gQuads,
  gRect,
  gText,
  modelOf,
  pageRef,
  rc,
  sq,
  start,
  style,
  tq,
  v,
  type Fn,
} from './annotation-core-fixtures';
import { hitFns, propFns, sceneFns } from './annotation-core-model';
import { updateFns } from './annotation-core-update';
import { drive, viewFns } from './annotation-core-view';

// ── fixtures ────────────────────────────────────────────────────────────────

const CROP = { left: 0, bottom: 0, right: 612, top: 792 };
const OFFSET_CROP = { left: 36, bottom: -18, right: 576, top: 720 };
const nf = (unit: string, conversion: number | undefined, over: object = {}) =>
  conversion === undefined ? { unit, ...over } : { unit, conversion, ...over };
/** 72 pt to the metre: a 144 pt line reads 2 m. */
const METRES = {
  subtype: 'RL',
  ratio: '1 in = 1 m',
  x: [nf('m', 1 / 72)],
  distance: [nf('m', 1)],
  area: [nf('m²', 1)],
};
const measureWith = (over: object) => ({ ...METRES, ...over });
/** Every readout branch, keyed by name. */
const MEASURES: [string, unknown][] = [
  ['metres', METRES],
  ['fixed-thousandths', measureWith({ distance: [nf('m', 1, { precision: 1000, fixed: true })] })],
  ['whole-units', measureWith({ distance: [nf('m', 1, { precision: 1 })] })],
  ['grouped', measureWith({ x: [nf('mm', 1000)], distance: [nf('mm', 1, { thousands: ' ', decimal: ',', precision: 10 })] })],
  ['no-grouping', measureWith({ x: [nf('mm', 1000)], distance: [nf('mm', 1, { thousands: '' })] })],
  ['empty-decimal-separator', measureWith({ distance: [nf('m', 1, { decimal: '' })] })],
  [
    'feet-inches',
    measureWith({ x: [nf('in', 1 / 72)], distance: [nf('ft', 1 / 12), nf('in', 12, { fraction: 'fraction', precision: 16 })] }),
  ],
  [
    'feet-inches-fixed',
    measureWith({ x: [nf('in', 1 / 72)], distance: [nf('ft', 1 / 12), nf('in', 12, { fraction: 'fraction', precision: 8, fixed: true })] }),
  ],
  ['fraction-only', measureWith({ x: [nf('in', 1 / 72)], distance: [nf('in', 1, { fraction: 'fraction' })] })],
  ['round', measureWith({ x: [nf('cm', 100 / 72)], distance: [nf('cm', 1, { fraction: 'round' })] })],
  ['truncate', measureWith({ x: [nf('cm', 100 / 72)], distance: [nf('cm', 1, { fraction: 'truncate' })] })],
  ['prefix', measureWith({ distance: [nf('m', 1, { labelPosition: 'prefix' })] })],
  ['prefix-spacing', measureWith({ distance: [nf('m', 1, { labelPosition: 'prefix', prefixSpacing: '<', suffixSpacing: '>' })] })],
  ['suffix-spacing', measureWith({ distance: [nf('m', 1, { prefixSpacing: '', suffixSpacing: ' ' })] })],
  ['y-axis', measureWith({ y: [nf('m', 2 / 72)], cyx: 0.5 })],
  ['y-axis-no-cyx', measureWith({ y: [nf('m', 2 / 72)] })],
  ['y-axis-bad', measureWith({ y: [nf('m', 0)], cyx: 1 })],
  ['foreign', { subtype: 'GEO' }],
  ['no-scale', measureWith({ x: [nf('m', 0)] })],
  ['no-x-conversion', measureWith({ x: [nf('m', undefined)] })],
  ['no-distance-format', measureWith({ distance: [] })],
  ['no-area-format', measureWith({ area: [] })],
  ['invalid-precision', measureWith({ distance: [nf('m', 1, { precision: 3 })] }), ],
  ['invalid-fraction-denominator', measureWith({ distance: [nf('in', 1, { fraction: 'fraction', precision: 0.5 })] })],
  ['invalid-conversion', measureWith({ distance: [nf('m', -1)] })],
  ['overflow', measureWith({ distance: [nf('m', 1e38), nf('cm', 1e38)] })],
  ['cascade-exact', measureWith({ x: [nf('ft', 1 / 72)], distance: [nf('ft', 1), nf('in', 12)] })],
  ['huge-value', measureWith({ x: [nf('mm', 1e20)], distance: [nf('mm', 1)] })],
  ['null', null],
];
const measure = (name: string) => MEASURES.find(([n]) => n === name)![1];

const dist = (over: Partial<DistanceAppearance> = {}): DistanceAppearance =>
  ({ intent: 'LineDimension', measure: METRES, caption: { enabled: true }, crop: CROP, text: 'stored', ...over }) as DistanceAppearance;
const shape = (
  intent: 'PolyLineDimension' | 'PolygonDimension',
  over: Partial<ShapeMeasurementAppearance> = {},
): ShapeMeasurementAppearance =>
  ({ intent, measure: METRES, caption: { enabled: true }, crop: CROP, text: 'stored', ...over }) as ShapeMeasurementAppearance;
const area = (over: Partial<ShapeMeasurementAppearance> = {}) => shape('PolygonDimension', over);
const perimeter = (over: Partial<ShapeMeasurementAppearance> = {}) => shape('PolyLineDimension', over);

const LINE = gLine(v(100, 100), v(244, 100), ends('closed-arrow', 'closed-arrow'));
const LINES: [string, Geom][] = [
  ['horizontal', LINE],
  ['reversed', gLine(v(244, 100), v(100, 100), ends('open-arrow', 'circle'))],
  ['vertical-down', gLine(v(100, 100), v(100, 244), ends('none', 'square'))],
  ['vertical-up', gLine(v(100, 244), v(100, 100))],
  ['diagonal-rot', gLine(v(100, 100), v(200, 200), ends('diamond', 'slash'), 45)],
  ['short', gLine(v(100, 100), v(110, 100), ends('closed-arrow', 'closed-arrow'))],
  ['zero', gLine(v(100, 100), v(100, 100))],
];
const APPEARANCES: [string, DistanceAppearance][] = [
  ['plain', dist()],
  ['leader', dist({ leader: { length: 20 } })],
  ['leader-negative-extended', dist({ leader: { length: -20, extension: 5, offset: 3 } })],
  ['top', dist({ caption: { enabled: true, position: 'top' } })],
  ['inline', dist({ caption: { enabled: true, position: 'inline' } })],
  ['offset-zero', dist({ caption: { enabled: true, offset: { along: 0, perpendicular: 0 } } })],
  ['offset-near', dist({ caption: { enabled: true, offset: { along: 10, perpendicular: 3 } } })],
  ['offset-above', dist({ caption: { enabled: true, offset: { along: 5, perpendicular: 30 } } })],
  ['offset-below-far', dist({ caption: { enabled: true, offset: { along: 120, perpendicular: -30 } } })],
  ['offset-along-back', dist({ caption: { enabled: true, offset: { along: -80, perpendicular: 0 } } })],
  ['offset-along-past', dist({ caption: { enabled: true, offset: { along: 80, perpendicular: 0 } } })],
  ['disabled', dist({ caption: { enabled: false } })],
  ['no-measure-empty-text', dist({ measure: null, text: '' })],
  ['no-measure-text', dist({ measure: null, text: 'kept ' })],
  ['offset-crop', dist({ crop: OFFSET_CROP, leader: { length: 10 } })],
  ['outside-negative-leader', dist({ leader: { length: -12 } })],
];

const PROBES = [v(172, 100), v(172, 80), v(172, 120), v(100, 100), v(244, 100), v(90, 100), v(100, 90), v(5, 5), v(172, 104), v(110, 100)];

const AREA_POINTS = [v(100, 300), v(244, 300), v(244, 372), v(100, 372)];
const CONCAVE = [v(100, 300), v(300, 300), v(300, 320), v(120, 320), v(120, 480), v(300, 480), v(300, 500), v(100, 500)];
const ZIG = [v(350, 300), v(450, 350), v(550, 300)];
const SHAPES: [string, Geom, ShapeMeasurementAppearance][] = [
  ['area', gPoly(AREA_POINTS, true), area()],
  ['area-concave', gPoly(CONCAVE, true), area()],
  ['area-rotated', gPoly(AREA_POINTS, true, undefined, 30), area()],
  ['area-pinned', gPoly(AREA_POINTS, true), area({ caption: { enabled: true, center: { x: 150, y: 400 } } })],
  ['area-pinned-origin', gPoly(AREA_POINTS, true), area({ caption: { enabled: true, center: { x: 0, y: 0 } } })],
  ['area-collinear', gPoly([v(0, 0), v(50, 0), v(100, 0)], true), area()],
  ['area-bowtie', gPoly([v(0, 0), v(100, 100), v(100, 0), v(0, 100)], true), area()],
  ['area-disabled', gPoly(AREA_POINTS, true), area({ caption: { enabled: false } })],
  ['area-foreign', gPoly(AREA_POINTS, true), area({ measure: { subtype: 'GEO' } as never })],
  ['area-no-measure-empty', gPoly(AREA_POINTS, true), area({ measure: null, text: '' })],
  ['area-offset-crop', gPoly(AREA_POINTS, true), area({ crop: OFFSET_CROP })],
  ['perimeter', gPoly(ZIG, false, ends('none', 'open-arrow')), perimeter()],
  ['perimeter-rotated', gPoly(ZIG, false, undefined, 20), perimeter()],
  ['perimeter-pinned', gPoly(ZIG, false), perimeter({ caption: { enabled: true, center: { x: 450, y: 412 } } })],
  ['perimeter-repeat', gPoly([v(10, 10), v(10, 10), v(110, 10)], false), perimeter()],
  ['perimeter-one-point', gPoly([v(10, 10)], false), perimeter()],
  ['perimeter-all-same', gPoly([v(10, 10), v(10, 10)], false), perimeter()],
  ['perimeter-empty', gPoly([], false), perimeter()],
  ['perimeter-on-line', LINE, perimeter()],
  ['area-dashed-style', gPoly(AREA_POINTS, true), area()],
];

const STYLE = style({ strokeWidth: 1.5 });
const DASHED_STYLE = style({ strokeWidth: 2, border: DASHED, interiorColor: '#00ff00', opacity: 0.5 });

const layoutOf = (geom: Geom, appearance: DistanceAppearance, sw: number) => ms.distanceLayout(geom, appearance, sw);

// ── pure functions ──────────────────────────────────────────────────────────

const measurementFns: Record<string, Fn> = {
  distanceCaptionWidth: mf.distanceCaptionWidth,
  distanceLabel: ms.distanceLabel,
  distanceLayout: ms.distanceLayout,
  expandDistanceBounds: ms.expandDistanceBounds,
  distanceSelectionQuad: ms.distanceSelectionQuad,
  distanceHandles: (geom: Geom, a: DistanceAppearance, sw: number) => {
    const l = layoutOf(geom, a, sw);
    return l ? ms.distanceHandles(l) : null;
  },
  /** `distanceCaptionHit` and `distanceHit` over probe points. */
  distanceHitProbe: (geom: Geom, a: DistanceAppearance, sw: number, points: Vec[], margin: number) => {
    const l = layoutOf(geom, a, sw);
    return points.map((p) => [
      ms.distanceCaptionHit(l ?? { caption: null }, p, margin),
      ms.distanceCaptionHit(l ?? { caption: null }, p),
      l ? ms.distanceHit(l, p, sw, margin) : null,
    ]);
  },
  distanceCaptionAt: ms.distanceCaptionAt,
  distanceLeaderLength: ms.distanceLeaderLength,
  moveDistanceCaption: ms.moveDistanceCaption,
  distanceScene: ms.distanceScene,
  /** The caption scene of a measurement's own layout. */
  measurementCaptionScene: (geom: Geom, a: MeasurementAppearance, s: Style) =>
    ms.measurementCaptionScene(msh.measurementLayout(geom, a, s)?.caption ?? null, s),
  shapeCaptionPoint: msh.shapeCaptionPoint,
  withShapeCaptionPoint: msh.withShapeCaptionPoint,
  /** `transformMeasurementCaption` with a translation. */
  transformMeasurementCaption: (a: MeasurementAppearance | undefined, dx: number, dy: number) =>
    msh.transformMeasurementCaption(a, (p) => ({ x: p.x + dx, y: p.y + dy })),
  shapeMeasurementReadout: msh.shapeMeasurementReadout,
  shapeMeasurementLabel: msh.shapeMeasurementLabel,
  automaticShapeCaptionCenter: msh.automaticShapeCaptionCenter,
  shapeMeasurementLayout: msh.shapeMeasurementLayout,
  measurementLayout: msh.measurementLayout,
  measurementSelectionQuad: msh.measurementSelectionQuad,
  moveMeasurementCaption: msh.moveMeasurementCaption,
  annotationSelectionFrame,
  textPlateInset: g.textPlateInset,
  pointInPoly: g.pointInPoly,
  geomResetRotation: g.geomResetRotation,
};

const measurementCases: FunctionCase[] = [];
for (const text of ['', '2 m', '1,234.5 m²', "3′ 4 1/2″", 'café ', '− 1 m\t', '\u{1F4CF}', '~', ' 　﻿']) {
  measurementCases.push(c('distanceCaptionWidth', JSON.stringify(text), text));
}
for (const [name, m] of MEASURES) {
  measurementCases.push(c('distanceLabel', name, LINE, dist({ measure: m as never })));
  measurementCases.push(c('shapeMeasurementReadout', `area-${name}`, gPoly(AREA_POINTS, true), area({ measure: m as never })));
  measurementCases.push(c('shapeMeasurementReadout', `perimeter-${name}`, gPoly(ZIG, false), perimeter({ measure: m as never })));
}
measurementCases.push(
  c('distanceLabel', 'not-a-line', gRect(rc(0, 0, 10, 10)), dist()),
  c('distanceLabel', 'nan-point', gLine(v(NaN, 0), v(10, 0)), dist()),
  c('distanceLabel', 'beyond-float', gLine(v(0, 0), v(1e39, 0)), dist()),
  c('distanceLabel', 'long-line-feet', gLine(v(0, 0), v(1e6, 0)), dist({ measure: measure('feet-inches') as never })),
  c('distanceLabel', 'rounds-up-inch', gLine(v(0, 0), v(71.999, 0)), dist({ measure: measure('feet-inches') as never })),
  c('distanceLabel', 'offset-crop', LINE, dist({ crop: OFFSET_CROP })),
  c('shapeMeasurementReadout', 'area-two-points', gPoly([v(0, 0), v(10, 10)], true), area()),
  c('shapeMeasurementReadout', 'area-closing-repeat', gPoly([...AREA_POINTS, AREA_POINTS[0]], true), area()),
  c('shapeMeasurementReadout', 'area-bowtie', gPoly([v(0, 0), v(100, 100), v(100, 0), v(0, 100)], true), area()),
  c('shapeMeasurementReadout', 'area-doubles-back', gPoly([v(0, 0), v(100, 0), v(50, 0), v(50, 50)], true), area()),
  c('shapeMeasurementReadout', 'area-duplicate-vertex', gPoly([v(0, 0), v(100, 0), v(100, 0), v(50, 50)], true), area()),
  c('shapeMeasurementReadout', 'area-touching', gPoly([v(0, 0), v(100, 0), v(100, 100), v(50, 0), v(0, 100)], true), area()),
  c('shapeMeasurementReadout', 'area-collinear-forward', gPoly([v(0, 0), v(50, 0), v(100, 0), v(100, 100)], true), area()),
  c('shapeMeasurementReadout', 'area-concave', gPoly(CONCAVE, true), area()),
  c('shapeMeasurementReadout', 'area-no-distance-perimeter', gPoly(AREA_POINTS, true), area({ measure: measureWith({ distance: [] }) as never })),
  c('shapeMeasurementReadout', 'area-bad-distance-perimeter', gPoly(AREA_POINTS, true), area({ measure: measureWith({ distance: [nf('m', 0)] }) as never })),
  c('shapeMeasurementReadout', 'area-overflow-perimeter', gPoly(AREA_POINTS, true), area({ measure: measureWith({ area: [nf('m²', 1)], distance: [nf('m', 1e38), nf('cm', 1e38)] }) as never })),
  c('shapeMeasurementReadout', 'not-poly', LINE, perimeter()),
  c('shapeMeasurementReadout', 'perimeter-one-point', gPoly([v(1, 1)], false), perimeter()),
  c('shapeMeasurementReadout', 'perimeter-infinite', gPoly([v(1, 1), v(Infinity, 1)], false), perimeter()),
);
for (const [gname, geom] of LINES) {
  for (const [aname, a] of APPEARANCES) {
    const sw = gname === 'short' ? 1 : 1.5;
    measurementCases.push(c('distanceLayout', `${gname}-${aname}`, geom, a, sw));
  }
  measurementCases.push(
    c('distanceSelectionQuad', gname, geom, dist({ leader: { length: 15 } }), 1.5),
    c('distanceHandles', gname, geom, dist({ leader: { length: -10 } }), 1.5),
    c('distanceHitProbe', gname, geom, dist({ leader: { length: -20 } }), 1.5, PROBES, 3),
    c('distanceCaptionAt', gname, geom, dist(), 1.5),
    c('distanceLeaderLength', gname, geom, v(150, 60)),
    c('moveDistanceCaption', gname, geom, dist({ caption: { enabled: true, offset: { along: 3, perpendicular: -2 } } }), v(10, -5)),
    c('distanceScene', gname, geom, dist({ leader: { length: -20 } }), DASHED_STYLE),
  );
}
measurementCases.push(
  c('distanceLayout', 'not-a-line', gRect(rc(0, 0, 10, 10)), dist(), 1),
  c('distanceLayout', 'wide-stroke-outside', LINE, dist(), 8),
  c('distanceLayout', 'zero-stroke', LINE, dist(), 0),
  c('distanceSelectionQuad', 'not-a-line', gRect(rc(0, 0, 10, 10)), dist(), 2),
  c('distanceHandles', 'not-a-line', gRect(rc(0, 0, 10, 10)), dist(), 2),
  c('distanceHitProbe', 'not-a-line', gRect(rc(0, 0, 10, 10)), dist(), 2, [v(5, 5)], 1),
  c('distanceHitProbe', 'endings-hit', gLine(v(100, 100), v(244, 100), ends('circle', 'closed-arrow')), dist({ caption: { enabled: false } }), 4, [v(96, 100), v(248, 100), v(240, 100), v(100, 108), v(172, 90)], 0.5),
  c('distanceHitProbe', 'open-ending-only', gLine(v(100, 100), v(244, 100), ends('open-arrow', 'butt')), dist({ caption: { enabled: false }, leader: { length: -30 } }), 4, [v(104, 102), v(244, 104), v(172, 130)], 0.5),
  c('distanceCaptionAt', 'disabled', LINE, dist({ caption: { enabled: false } }), 1),
  c('distanceCaptionAt', 'not-a-line', gRect(rc(0, 0, 10, 10)), dist(), 1),
  c('distanceLeaderLength', 'not-a-line', gRect(rc(0, 0, 10, 10)), v(5, 5)),
  c('moveDistanceCaption', 'no-offset', LINE, dist(), v(4, 4)),
  c('moveDistanceCaption', 'not-a-line', gRect(rc(0, 0, 10, 10)), dist(), v(4, 4)),
  c('distanceScene', 'not-a-line', gRect(rc(0, 0, 10, 10)), dist(), STYLE),
  c('distanceScene', 'solid-no-fill', gLine(v(100, 100), v(244, 100), ends('circle', 'open-arrow')), dist(), STYLE),
  c('expandDistanceBounds', 'grow', rc(10, 20, 30, 40), 2.5),
  c('expandDistanceBounds', 'negative', rc(10, 20, 30, 40), -1),
);
for (const [name, geom, a] of SHAPES) {
  const s = name.includes('dashed') ? DASHED_STYLE : STYLE;
  measurementCases.push(
    c('shapeMeasurementLayout', name, geom, a, s),
    c('measurementLayout', name, geom, a, s),
    c('measurementSelectionQuad', name, geom, a, s),
    c('moveMeasurementCaption', name, geom, a, v(12, -7), s),
    c('shapeMeasurementLabel', name, geom, a),
    c('measurementCaptionScene', name, geom, a, s),
  );
}
measurementCases.push(
  c('measurementLayout', 'distance', LINE, dist(), STYLE),
  c('measurementLayout', 'distance-not-line', gRect(rc(0, 0, 10, 10)), dist(), STYLE),
  c('measurementSelectionQuad', 'distance', LINE, dist({ leader: { length: 10 } }), STYLE),
  c('moveMeasurementCaption', 'distance', LINE, dist(), v(3, 4), STYLE),
  c('measurementCaptionScene', 'distance', LINE, dist(), DASHED_STYLE),
  c('measurementCaptionScene', 'none', gRect(rc(0, 0, 10, 10)), dist(), STYLE),
  c('shapeCaptionPoint', 'automatic', area()),
  c('shapeCaptionPoint', 'pinned', area({ caption: { enabled: true, center: { x: 50, y: 700 } } })),
  c('shapeCaptionPoint', 'pinned-offset-crop', area({ crop: OFFSET_CROP, caption: { enabled: false, center: { x: 50, y: 700 } } })),
  c('withShapeCaptionPoint', 'pins', area(), v(100, 92)),
  c('withShapeCaptionPoint', 'offset-crop', area({ crop: OFFSET_CROP }), v(100, 92)),
  c('transformMeasurementCaption', 'none', undefined, 5, 5),
  c('transformMeasurementCaption', 'distance', dist(), 5, 5),
  c('transformMeasurementCaption', 'automatic', area(), 5, 5),
  c('transformMeasurementCaption', 'pinned', perimeter({ caption: { enabled: true, center: { x: 450, y: 412 } } }), 5, -5),
  c('automaticShapeCaptionCenter', 'empty-open', [], false),
  c('automaticShapeCaptionCenter', 'empty-closed', [], true),
  c('automaticShapeCaptionCenter', 'open', ZIG, false),
  c('automaticShapeCaptionCenter', 'open-zero-first', [v(0, 0), v(0, 0), v(100, 0)], false),
  c('automaticShapeCaptionCenter', 'open-all-zero', [v(3, 4), v(3, 4), v(3, 4)], false),
  c('automaticShapeCaptionCenter', 'open-single', [v(3, 4)], false),
  c('automaticShapeCaptionCenter', 'closed-square', AREA_POINTS, true),
  c('automaticShapeCaptionCenter', 'closed-concave', CONCAVE, true),
  c('automaticShapeCaptionCenter', 'closed-collinear', [v(0, 0), v(50, 0), v(100, 0)], true),
  c('automaticShapeCaptionCenter', 'closed-single', [v(7, 8)], true),
  c('automaticShapeCaptionCenter', 'closed-ring-far', [v(1000, 1000), v(1100, 1000), v(1100, 1100), v(1000, 1100)], true),
  c('automaticShapeCaptionCenter', 'closed-c-shape-wide-right', [v(0, 0), v(100, 0), v(100, 100), v(0, 100), v(0, 60), v(80, 60), v(80, 40), v(0, 40)], true),
  c('textPlateInset', 'positive', 2),
  c('textPlateInset', 'zero', 0),
  c('textPlateInset', 'negative', -3),
  c('textPlateInset', 'nan', NaN),
  c('pointInPoly', 'inside', v(150, 330), AREA_POINTS),
  c('pointInPoly', 'outside', v(50, 330), AREA_POINTS),
  c('pointInPoly', 'concave-gap', v(200, 400), CONCAVE),
  c('pointInPoly', 'empty', v(0, 0), []),
  c('geomResetRotation', 'poly-pivot', gPoly(ZIG, false, undefined, 30), v(0, 0)),
  c('geomResetRotation', 'poly-no-pivot', gPoly(ZIG, false, undefined, 30)),
  c('geomResetRotation', 'rect-pivot-ignored', gRect(rc(0, 0, 10, 10), false, 30), v(100, 100)),
  c('geomResetRotation', 'unrotated-pivot', gPoly(ZIG, false), v(100, 100)),
);

const FRAME_ANNOTS: [string, Annot, ViewEnv?][] = [
  ['square', sq('a')],
  ['square-rotated', sq('a', rc(100, 100, 80, 60), { geom: gRect(rc(100, 100, 80, 60), false, 30) })],
  ['quads', annot('a', 'highlight', gQuads([tq(10, 10, 100, 12)]))],
  ['distance', annot('a', 'line', LINE, { measure: dist({ leader: { length: -20 } }) })],
  ['distance-rotated', annot('a', 'line', gLine(v(100, 100), v(200, 200), undefined, 45), { measure: dist() })],
  ['area', annot('a', 'polygon', gPoly(AREA_POINTS, true), { measure: area() })],
  ['perimeter-rotated', annot('a', 'polyline', gPoly(ZIG, false, undefined, 20), { measure: perimeter() })],
  ['nozoom-view', sq('a', rc(100, 100, 20, 20), { flags: flags({ noZoom: true, noRotate: true }) }), { zoom: 2, rotation: 90 }],
  ['nozoom-distance-view', annot('a', 'line', LINE, { flags: flags({ noZoom: true }), measure: dist() }), { zoom: 3, rotation: 0 }],
];
for (const [name, a, view] of FRAME_ANNOTS) measurementCases.push(c('annotationSelectionFrame', name, a, view));

// ── rich text ───────────────────────────────────────────────────────────────

const run = (text: string, style?: object) => (style ? { text, style } : { text });
const para = (...runs: object[]) => ({ runs });
const DOC = {
  body: { family: 'Helvetica', size: 12 },
  paragraphs: [
    para(run('Hello '), run('bold', { weight: 700 }), run(' world')),
    { align: 'center', runs: [run('Second '), run('line', { italic: true, decoration: ['underline', 'line-through'] })] },
    para(run('')),
    para(run('café \u{1F600}!', { color: '#ff0000' })),
  ],
};
const MESSY = {
  paragraphs: [
    para(run('a', { weight: 700 }), run('', { italic: true }), run('b', { weight: 700, size: undefined }), run('c', {}), run('d')),
    para(),
    para(run('x', { decoration: ['line-through', 'underline'] }), run('y', { decoration: ['underline', 'line-through'] }), run('z', { decoration: ['underline'] })),
    para(run('p', { decoration: ['underline'] }), run('q', { decoration: ['word'] }), run('r', { family: 'Times' })),
  ],
};
const richFns: Record<string, Fn> = {
  richTextLength: rt.richTextLength,
  sameStyleDelta: rt.sameStyleDelta,
  normalizeRuns: rt.normalizeRuns,
  locateOffset: rt.locateOffset,
  splitRunsAt: rt.splitRunsAt,
  applyStyleToRange: rt.applyStyleToRange,
  styleAt: rt.styleAt,
  /** `rangeHasStyle` with a predicate: the delta's `key` equals `value` as JSON. */
  rangeHasStyle: (doc: Parameters<typeof rt.rangeHasStyle>[0], range: rt.RichTextRange, probe: { key: string; value?: unknown }) =>
    rt.rangeHasStyle(doc, range, (d) => JSON.stringify((d as Record<string, unknown>)[probe.key]) === JSON.stringify(probe.value)),
  paragraphsFromPlainText: rt.paragraphsFromPlainText,
  plainTextOf: rt.plainTextOf,
  isPlainRichText: rt.isPlainRichText,
};
const richCases: FunctionCase[] = [
  c('richTextLength', 'doc', DOC),
  c('richTextLength', 'empty', { paragraphs: [] }),
  c('richTextLength', 'messy', MESSY),
  c('sameStyleDelta', 'both-undefined', undefined, undefined),
  c('sameStyleDelta', 'empty-and-undefined', {}, undefined),
  c('sameStyleDelta', 'weights-differ', { weight: 700 }, { weight: 400 }),
  c('sameStyleDelta', 'decoration-order', { decoration: ['underline', 'line-through'] }, { decoration: ['line-through', 'underline'] }),
  c('sameStyleDelta', 'decoration-lengths', { decoration: ['underline'] }, { decoration: ['underline', 'word'] }),
  c('sameStyleDelta', 'decoration-one-side', { decoration: ['underline'] }, {}),
  c('sameStyleDelta', 'decoration-values', { decoration: ['underline'] }, { decoration: ['word'] }),
  c('sameStyleDelta', 'every-key', { family: 'A', weight: 1, italic: true, size: 2, color: '#000', script: 'sub', letterSpacing: 1, horizontalScale: 2, unknown: 'x' }, { family: 'A', weight: 1, italic: true, size: 2, color: '#000', script: 'sub', letterSpacing: 1, horizontalScale: 2, unknown: 'x' }),
  c('sameStyleDelta', 'unknown-differs', { unknown: 'x' }, { unknown: 'y' }),
  c('normalizeRuns', 'doc', DOC),
  c('normalizeRuns', 'messy', MESSY),
  c('plainTextOf', 'doc', DOC),
  c('plainTextOf', 'messy', MESSY),
  c('isPlainRichText', 'doc', DOC),
  c('isPlainRichText', 'plain', { paragraphs: [para(run('a')), para(run('b', {}))] }),
  c('isPlainRichText', 'empty-body', { body: {}, paragraphs: [para(run('a'))] }),
  c('isPlainRichText', 'body', { body: { size: 12 }, paragraphs: [para(run('a'))] }),
  ...['align', 'dir', 'lineHeight', 'margins', 'textIndent', 'unknown'].map((k) =>
    c('isPlainRichText', `paragraph-${k}`, { paragraphs: [{ [k]: k === 'margins' ? { top: 1, bottom: 1, left: 1, right: 1 } : k === 'align' ? 'left' : k === 'dir' ? 'rtl' : k === 'unknown' ? 'x: y' : 2, runs: [run('a')] }] }),
  ),
  c('isPlainRichText', 'styled-run', { paragraphs: [para(run('a', { italic: false }))] }),
  ...['a\r\nb', 'a\rb\nc', '', 'one', '\n\n', 'x\r\r\ny'].map((t) => c('paragraphsFromPlainText', JSON.stringify(t), t)),
];
for (const offset of [0, 3, 6, 10, 16, 17, 18, 22, 23, 24, 30, 100, -5, 2.5]) {
  richCases.push(c('locateOffset', `doc-${offset}`, DOC, offset), c('styleAt', `doc-${offset}`, DOC, offset));
}
richCases.push(
  c('locateOffset', 'empty', { paragraphs: [] }, 3),
  c('styleAt', 'empty', { paragraphs: [] }, 3),
  c('styleAt', 'empty-run', { paragraphs: [para(run(''))] }, 0),
  c('styleAt', 'no-runs', { paragraphs: [para()] }, 0),
  c('styleAt', 'after-styled-run', { paragraphs: [para(run('ab', { weight: 700 }))] }, 5),
);
for (const offset of [0, 3, 6, 10, 16, 2.5]) {
  richCases.push(c('splitRunsAt', `first-${offset}`, DOC.paragraphs[0], offset));
}
richCases.push(c('splitRunsAt', 'surrogate-safe', DOC.paragraphs[3], 5));
const RANGES: [string, rt.RichTextRange][] = [
  ['inside-run', { start: 1, end: 4 }],
  ['across-runs', { start: 3, end: 12 }],
  ['reversed', { start: 12, end: 3 }],
  ['empty', { start: 5, end: 5 }],
  ['across-paragraphs', { start: 8, end: 20 }],
  ['whole', { start: 0, end: 100 }],
  ['negative-start', { start: -4, end: 2 }],
  ['paragraph-boundary', { start: 16, end: 17 }],
  ['empty-paragraph', { start: 29, end: 30 }],
  ['emoji', { start: 35, end: 37 }],
];
for (const [name, range] of RANGES) {
  richCases.push(
    c('applyStyleToRange', `bold-${name}`, DOC, range, { weight: 700 }),
    c('rangeHasStyle', `bold-${name}`, DOC, range, { key: 'weight', value: 700 }),
    c('rangeHasStyle', `unstyled-${name}`, DOC, range, { key: 'weight' }),
  );
}
richCases.push(
  c('applyStyleToRange', 'clear-weight', DOC, { start: 0, end: 16 }, { italic: true }, ['weight']),
  c('applyStyleToRange', 'clear-all', DOC, { start: 0, end: 100 }, {}, ['weight', 'italic', 'decoration', 'color']),
  c('applyStyleToRange', 'merges-into-neighbour', DOC, { start: 6, end: 11 }, { weight: 400 }),
  c('applyStyleToRange', 'messy', MESSY, { start: 0, end: 4 }, { size: 9 }),
  c('applyStyleToRange', 'end-in-empty-paragraph', MESSY, { start: 2, end: 6 }, { color: '#00f' }),
  c('rangeHasStyle', 'messy-empty-paragraph', MESSY, { start: 5, end: 6 }, { key: 'weight' }),
  c('rangeHasStyle', 'empty-doc-caret', { paragraphs: [] }, { start: 2, end: 2 }, { key: 'weight' }),
  c('rangeHasStyle', 'decoration-array', MESSY, { start: 6, end: 8 }, { key: 'decoration', value: ['line-through', 'underline'] }),
);

// ── model level: reducer, hit test, scene, view ─────────────────────────────

const TEXT = { fontFamily: 'helvetica', fontSize: 12, fontColor: '#000000', textAlign: 'left' as const };
const RICH_DTO = {
  subtype: 'free-text',
  contents: 'hi',
  richText: { body: { family: 'Helvetica', size: 12, weight: 400 }, paragraphs: [para(run('hi'))] },
};
const MODEL_ANNOTS: Annot[] = [
  annot('dist', 'line', LINE, { measure: dist({ leader: { length: -20 } }) }),
  annot('dist-free', 'line', gLine(v(300, 700), v(444, 700)), { measure: dist() }),
  annot('dist-close', 'line', gLine(v(100, 600), v(244, 600), ends('closed-arrow', 'closed-arrow')), { measure: dist({ leader: { length: -3 } }) }),
  annot('area', 'polygon', gPoly(AREA_POINTS, true), { measure: area() }),
  annot('perim', 'polyline', gPoly(ZIG, false), { measure: perimeter({ caption: { enabled: true, center: { x: 450, y: 412 } } }) }),
  annot('ft', 'free-text', gText(rc(400, 500, 120, 40)), { text: { ...TEXT, bold: true }, data: RICH_DTO as never }),
  annot('ft-plain-dto', 'free-text', gText(rc(400, 560, 120, 40)), { text: TEXT, data: { subtype: 'free-text', contents: 'x' } as never }),
  annot('callout', 'free-text', gText(rc(480, 700, 100, 40), { tip: v(590, 790), ending: 'closed-arrow' }), { text: TEXT }),
  sq('sq', rc(20, 20, 40, 40), { data: { subtype: 'square' } as never }),
  sq('bare', rc(80, 20, 40, 40)),
];
const MS = (over: Partial<Model> = {}) => start(MODEL_ANNOTS, over);
const PAGE = rc(0, 0, 600, 800);
type Phase = 'down' | 'move' | 'up';
const inp = (x: number, y: number, over: Partial<PointerInput> = {}): PointerInput => ({ page: pageRef(PON), point: v(x, y), shift: false, ...over });
const ed = (phase: Phase, x: number, y: number, over: Partial<PointerInput> = {}): Msg => ({ t: 'editPointer', phase, in: inp(x, y, over) });
const cr = (subtype: string, phase: Phase, x: number, y: number, over: Partial<PointerInput> = {}, msg: object = {}): Msg =>
  ({ t: 'createPointer', phase, subtype, in: inp(x, y, over), ...msg }) as Msg;
const DIST_MSG = { measure: dist() };

/** The caption centre of the `dist` annotation, where a click grabs it. */
const DIST_CAPTION = ms.distanceCaptionAt(LINE, dist({ leader: { length: -20 } }), 2)!;
const PERIM_CAPTION = { x: 450, y: 792 - 412 };
const AREA_CAPTION = msh.shapeMeasurementLayout(gPoly(AREA_POINTS, true), area(), initialModel.style)!.caption!.center;

const RUNS: [string, Partial<Model>, Msg[]][] = [
  ['distance-create', MS(), [
    cr('line', 'down', 100, 700, {}, DIST_MSG),
    cr('line', 'down', 110, 700, {}, DIST_MSG),
    cr('line', 'move', 200, 700, {}, DIST_MSG),
    cr('line', 'up', 244, 700, {}, DIST_MSG),
    cr('line', 'up', 244, 700),
    cr('line', 'move', 170, 680, { page: pageRef(2) }),
    cr('line', 'move', 170, 680),
    cr('line', 'down', 170, 720, { pageBox: PAGE }, { flags: { print: false } }),
  ]],
  ['distance-create-flags-defaults', MS({ defaults: { dim: { lineEndings: { start: 'open-arrow', end: 'open-arrow' }, interiorColor: '#123456' } } }), [
    cr('line', 'down', 100, 700, {}, { ...DIST_MSG, preset: 'dim', flags: { locked: true } }),
    cr('line', 'up', 244, 700),
    cr('line', 'down', 170, 690),
  ]],
  ['distance-create-short', MS(), [cr('line', 'down', 100, 700, {}, DIST_MSG), cr('line', 'up', 101, 701, {}, DIST_MSG)]],
  ['distance-create-no-measure-move', MS(), [cr('line', 'move', 100, 700, {}, DIST_MSG), cr('line', 'up', 100, 700, {}, DIST_MSG)]],
  ['distance-create-foreign-final', MS(), [cr('line', 'down', 100, 700, {}, DIST_MSG), cr('line', 'up', 200, 700), cr('line', 'down', 150, 650, { page: pageRef(2) })]],
  ['distance-capture', MS(), [
    cr('line', 'down', 100, 700, {}, { ...DIST_MSG, capture: 'calibrate' }),
    cr('line', 'move', 244, 700),
    cr('line', 'up', 244, 700),
  ]],
  ['distance-capture-click', MS(), [cr('line', 'down', 100, 700, {}, { ...DIST_MSG, capture: 'calibrate' }), cr('line', 'up', 100, 700)]],
  ['distance-capture-empty-tool', MS(), [cr('line', 'down', 100, 700, {}, { ...DIST_MSG, capture: '' }), cr('line', 'up', 244, 700)]],
  ['line-shape-measure', MS(), [cr('line', 'down', 100, 700, {}, { measure: perimeter() }), cr('line', 'move', 200, 700), cr('line', 'up', 200, 700)]],
  ['poly-area-create', MS(), [
    cr('polygon', 'down', 100, 650, {}, { measure: area() }),
    cr('polygon', 'down', 200, 650),
    cr('polygon', 'down', 150, 750),
    { t: 'finishCreationDraft' },
  ]],
  ['poly-area-invalid', MS(), [
    cr('polygon', 'down', 100, 650, {}, { measure: area() }),
    cr('polygon', 'down', 150, 650),
    cr('polygon', 'down', 200, 650),
    { t: 'finishCreationDraft' },
  ]],
  ['poly-perimeter-create', MS(), [
    cr('polyline', 'down', 100, 650, {}, { measure: perimeter() }),
    cr('polyline', 'down', 200, 650),
    cr('polyline', 'down', 250, 700, { finish: true }),
  ]],
  ['poly-distance-measure-ignored', MS(), [
    cr('polyline', 'down', 100, 650, {}, { measure: dist(), capture: 'x' }),
    cr('polyline', 'down', 200, 650),
    { t: 'finishCreationDraft' },
  ]],
  ['poly-preset-restart', MS(), [
    cr('polygon', 'down', 100, 650, {}, { preset: 'a' }),
    cr('polygon', 'down', 200, 650, {}, { preset: 'a' }),
    cr('polygon', 'down', 150, 750, {}, { preset: 'b' }),
  ]],
  ['create-annot', MS({ defaults: { arrow: { lineEndings: { end: 'closed-arrow' }, color: '#0000ff' }, link: { link: { kind: 'uri', uri: 'https://e.x' } as never } } }), [
    { t: 'createAnnot', page: pageRef(PON), subtype: 'line', geom: gLine(v(0, 0), v(50, 50)), preset: 'arrow' },
    { t: 'createAnnot', page: pageRef(PON), subtype: 'line', geom: gLine(v(0, 0), v(50, 50), ends('square', 'none')) },
    { t: 'createAnnot', page: pageRef(PON), subtype: 'polyline', geom: gPoly(ZIG, false), preset: 'arrow', select: true },
    { t: 'createAnnot', page: pageRef(2), subtype: 'polygon', geom: gPoly(AREA_POINTS, true), flags: { hidden: true } },
    { t: 'createAnnot', page: pageRef(PON), subtype: 'free-text', geom: gText(rc(10, 10, 100, 30)), props: { fontSize: 20, bold: true } },
    { t: 'createAnnot', page: pageRef(PON), subtype: 'link', geom: gRect(rc(10, 10, 100, 30)) },
    { t: 'createAnnot', page: pageRef(PON), subtype: 'square', geom: gRect(rc(10, 10, 100, 30)), props: { color: '#00ff00', strokeWidth: 4 } },
    { t: 'createAnnot', page: pageRef(PON), subtype: 'square', geom: gRect(rc(10, 10, 100, 30)), props: { icon: 'x' } },
    { t: 'createAnnot', page: pageRef(PON), subtype: 'square', geom: gRect(rc(10, 10, 100, 30)), props: { color: '#00ff00' }, flags: { locked: true } },
    { t: 'createAnnot', page: pageRef(PON), subtype: 'link', geom: gRect(rc(0, 0, 5, 5)), preset: 'none-link', select: false },
  ] as Msg[]],
  ['rich-text', MS(), [
    { t: 'setRichText', id: 'ft', doc: { body: { weight: 700 }, paragraphs: [para(run('a', { italic: true }), run('b', { italic: true })), para(run(''), run('c', {}))] } },
    { t: 'setRichText', id: 'ft', doc: { paragraphs: [para(run('no body'))] } },
    { t: 'setRichText', id: 'sq', doc: { paragraphs: [] } },
    { t: 'setRichText', id: 'bare', doc: { paragraphs: [] } },
    { t: 'setRichText', id: 'ghost', doc: { paragraphs: [] } },
  ] as Msg[]],
  ['set-text-rich', MS(), [
    { t: 'setText', id: 'ft', text: 'one\ntwo\r\nthree' },
    { t: 'setText', id: 'ft-plain-dto', text: 'solo' },
    { t: 'setText', id: 'sq', text: 'note' },
  ]],
  ['caption-drag-distance', MS({ selected: ['dist'] }), [
    ed('down', DIST_CAPTION.x, DIST_CAPTION.y),
    ed('move', DIST_CAPTION.x + 10, DIST_CAPTION.y + 30, { pageBox: PAGE }),
    ed('move', DIST_CAPTION.x + 10, DIST_CAPTION.y + 30, { page: pageRef(2) }),
    ed('up', DIST_CAPTION.x + 10, DIST_CAPTION.y + 30),
  ]],
  ['caption-drag-still', MS({ selected: ['dist'] }), [ed('down', DIST_CAPTION.x, DIST_CAPTION.y), ed('up', DIST_CAPTION.x, DIST_CAPTION.y)]],
  ['caption-drag-perimeter', MS({ selected: ['perim'] }), [ed('down', PERIM_CAPTION.x, PERIM_CAPTION.y), ed('move', PERIM_CAPTION.x - 20, PERIM_CAPTION.y + 5), ed('up', 0, 0)]],
  ['caption-drag-area', MS({ selected: ['area'] }), [ed('down', AREA_CAPTION.x, AREA_CAPTION.y), ed('move', AREA_CAPTION.x, AREA_CAPTION.y - 15), ed('up', 0, 0)]],
  ['caption-unselected-hit', MS(), [ed('down', DIST_CAPTION.x, DIST_CAPTION.y), ed('move', DIST_CAPTION.x + 5, DIST_CAPTION.y), ed('up', 0, 0)]],
  ['leader-drag', MS({ selected: ['dist'] }), [ed('down', 100, 120), ed('move', 110, 150), ed('up', 110, 150)]],
  ['leader-drag-end', MS({ selected: ['dist'] }), [ed('down', 244, 120), ed('move', 244, 120), ed('up', 244, 120)]],
  ['leader-drag-close-handles', MS({ selected: ['dist-close'] }), [ed('down', 100, 602), ed('move', 100, 640), ed('up', 100, 640)]],
  ['vertex-drag-distance', MS({ selected: ['dist-close'] }), [ed('down', 101, 599), ed('move', 90, 590), ed('up', 90, 590)]],
  ['draft-leader-no-leader', MS({ draft: { g: 'leader', id: 'dist-free', start: v(0, 0), delta: 7 } }), [ed('up', 0, 0)]],
  ['draft-rotate-perimeter', MS({ selected: ['perim'], draft: { g: 'rotate', ids: ['perim', 'area'], pivot: v(450, 330), start: v(450, 250), cur: v(530, 330), free: true } }), [ed('up', 0, 0)]],
  ['draft-group-perimeter', MS({ selected: ['perim', 'area'], draft: { g: 'group', op: 'resize', ids: ['perim', 'area'], handle: 'se', anchor: v(100, 300), base: rc(100, 300, 452, 80), cur: rc(100, 300, 500, 100) } }), [ed('up', 0, 0)]],
  ['callout-page-no-slide', MS(), [
    cr('free-text-callout', 'down', 200, 200, { pageBox: PAGE }),
    cr('free-text-callout', 'down', 250, 250, { pageBox: PAGE }),
    cr('free-text-callout', 'down', 260, 260, { pageBox: PAGE }),
    cr('free-text-callout', 'up', 260, 260, { pageBox: PAGE }),
  ]],
  ['draft-leader-not-distance', MS({ draft: { g: 'leader', id: 'area', start: v(0, 0), delta: 5 } }), [ed('up', 0, 0)]],
  ['draft-leader-missing', MS({ draft: { g: 'leader', id: 'ghost', start: v(0, 0), delta: 5 } }), [ed('up', 0, 0)]],
  ['draft-caption-no-measure', MS({ draft: { g: 'caption', id: 'sq', start: v(0, 0), delta: v(3, 3) } }), [ed('up', 0, 0)]],
  ['draft-caption-missing', MS({ draft: { g: 'caption', id: 'ghost', start: v(0, 0), delta: v(3, 3) } }), [ed('up', 0, 0)]],
  ['draft-caption-y-only', MS({ draft: { g: 'caption', id: 'dist', start: v(0, 0), delta: v(0, 4) } }), [ed('up', 0, 0)]],
  ['area-vertex-degenerate', MS({ selected: ['area'] }), [ed('down', 100, 300), ed('move', 244, 300), ed('up', 244, 300)]],
  ['area-vertex-valid', MS({ selected: ['area'] }), [ed('down', 100, 300), ed('move', 90, 290), ed('up', 90, 290)]],
  ['move-perimeter', MS({ selected: ['perim'] }), [ed('down', 400, 325), ed('move', 420, 345), ed('up', 420, 345)]],
  ['rotate-perimeter', MS({ selected: ['perim'] }), [{ t: 'rotate90' }, { t: 'resetRotation' }]],
  ['group-perimeter-area', MS({ selected: ['perim', 'area'] }), [{ t: 'rotate90' }]],
  ['hydrated-under-caption-drag', MS({ selected: ['dist'] }), [
    ed('down', DIST_CAPTION.x, DIST_CAPTION.y),
    { t: 'hydrated', annots: [annot('dist', 'line', LINE)] },
    { t: 'hydrated', annots: [] },
  ]],
  ['callout-page-slide', MS(), [
    cr('free-text-callout', 'down', 560, 700, { pageBox: PAGE }),
    cr('free-text-callout', 'down', 580, 780, { pageBox: PAGE }),
    cr('free-text-callout', 'down', 590, 790, { pageBox: PAGE }),
    cr('free-text-callout', 'up', 590, 790, { pageBox: PAGE }),
  ]],
  ['callout-page-slide-upright', MS(), [
    cr('free-text-callout', 'down', 560, 700, { pageBox: PAGE, upright: true, displayRotation: 90 }),
    cr('free-text-callout', 'down', 580, 780, { pageBox: PAGE }),
    cr('free-text-callout', 'down', 590, 790, { pageBox: PAGE }),
    cr('free-text-callout', 'up', 590, 790, { pageBox: PAGE }),
  ]],
  ['callout-move-clamp', MS({ selected: ['callout'] }), [ed('down', 530, 720), ed('move', 600, 800, { pageBox: PAGE }), ed('up', 600, 800)]],
  ['callout-move-clamp-view', MS({ selected: ['callout'] }), [ed('down', 530, 720, { zoom: 2 }), ed('move', 400, 720, { pageBox: PAGE, zoom: 2 }), ed('up', 400, 720)]],
  ['bold-props', MS({ selected: ['ft', 'ft-plain-dto', 'sq'] }), [
    { t: 'setProps', patch: { bold: true } },
    { t: 'setProps', patch: { italic: true, underline: false } },
    { t: 'setProps', patch: { underline: true } },
  ]],
];

const modelFns: Record<string, Fn> = { ...updateFns, ...hitFns, ...sceneFns, ...propFns, ...viewFns };
const modelCases: FunctionCase[] = [];
{
  const groups = new Map<string, { page: Partial<Model>; runs: { name: string; over: Partial<Model>; msgs: Msg[] }[] }>();
  for (const [name, s, msgs] of RUNS) {
    const { byId, order, ...over } = s;
    const page = { byId: byId ?? {}, order: order ?? [] };
    const key = JSON.stringify(page);
    const group = groups.get(key) ?? { page, runs: [] as { name: string; over: Partial<Model>; msgs: Msg[] }[] };
    group.runs.push({ name, over, msgs });
    groups.set(key, group);
  }
  let n = 0;
  for (const { page, runs } of groups.values()) {
    for (let i = 0; i < runs.length; i += 10) {
      const chunk = runs.slice(i, i + 10);
      modelCases.push(c('runs', `${n++}-${chunk[0].name}`, page, chunk));
    }
  }
}

const CG = { handleTol: 6, knobTol: 6, knobOffset: 24 };
const hitCall = (selected: string[], p: Vec, margin = 4, over: object = {}) => ({ selected, pon: PON, p, gm: CG, margin, ...over });
modelCases.push(
  c('hitCalls', 'measured', MS(), [
    hitCall(['dist'], DIST_CAPTION),
    hitCall(['dist'], { x: DIST_CAPTION.x + 30, y: DIST_CAPTION.y + 5 }),
    hitCall(['dist'], v(100, 100)),
    hitCall(['dist'], v(100, 120)),
    hitCall(['dist'], v(244, 118)),
    hitCall(['dist'], v(172, 60)),
    hitCall(['dist-close'], v(100, 601)),
    hitCall(['dist-close'], v(100, 603)),
    hitCall(['dist-close'], v(244, 600)),
    hitCall([], DIST_CAPTION),
    hitCall([], v(150, 120)),
    hitCall([], v(150, 100)),
    hitCall([], v(172, 60)),
    hitCall([], v(104, 120)),
    hitCall(['perim'], PERIM_CAPTION),
    hitCall(['perim'], v(450, 350)),
    hitCall([], PERIM_CAPTION),
    hitCall(['area'], AREA_CAPTION),
    hitCall(['area'], v(172, 300)),
    hitCall([], AREA_CAPTION),
    hitCall([], v(172, 330)),
    hitCall(['area', 'perim'], v(300, 330)),
    hitCall(['dist'], v(20, 20), 4, { view: { zoom: 2, rotation: 0 } }),
    hitCall(['ft'], v(460, 520)),
  ]),
);

const MEASURED_ITEMS = pageItems(modelOf(MS({ selected: ['dist'] })), pageRef(PON));
for (const item of MEASURED_ITEMS) modelCases.push(c('scene', `item-${item.id}`, item));
modelCases.push(
  c('scene', 'shape-caption-disabled', { ...MEASURED_ITEMS.find((i) => i.id === 'area')!, measure: area({ caption: { enabled: false } }) }),
  c('scene', 'distance-dashed', { ...MEASURED_ITEMS.find((i) => i.id === 'dist')!, style: DASHED_STYLE }),
  c('scene', 'text-rotated-plate', { ...MEASURED_ITEMS.find((i) => i.id === 'ft')!, geom: gText(rc(400, 500, 120, 40), undefined, 30) }),
  c('scene', 'text-zero-stroke', { ...MEASURED_ITEMS.find((i) => i.id === 'ft')!, style: style({ strokeWidth: 0 }) }),
);

const BOLD_FT = annot('ft', 'free-text', gText(rc(0, 0, 100, 30)), { text: { ...TEXT, bold: true, italic: false } });
const PLAIN_FT = annot('ft', 'free-text', gText(rc(0, 0, 100, 30)), { text: TEXT });
modelCases.push(
  c('readProps', 'bold-ft', BOLD_FT, ['bold', 'italic', 'underline', 'fontSize']),
  c('readProps', 'plain-ft', PLAIN_FT, ['bold', 'italic', 'underline']),
  c('readProps', 'square', sq('s'), ['bold', 'italic', 'underline']),
  c('applyProps', 'bold-on', PLAIN_FT, { bold: true }),
  c('applyProps', 'bold-same', BOLD_FT, { bold: true, italic: false }),
  c('applyProps', 'bold-off', BOLD_FT, { bold: false, underline: true }),
  c('applyProps', 'bold-square', sq('s'), { bold: true }),
  c('applyProps', 'italic-no-text', annot('ft', 'free-text', gText(rc(0, 0, 100, 30))), { italic: true }),
  c('textStyleFromProps', 'with-bold', { ...initialModel.style, ...TEXT, bold: true, italic: false, lineEndings: ends('none', 'none') }),
  c('textStyleFromProps', 'with-underline', { ...initialModel.style, ...TEXT, underline: true, lineEndings: ends('none', 'none') }),
  c('defaultsFor', 'bold-tool', { defaults: { 'free-text': { bold: true, underline: false } } }, 'free-text'),
);

const VIEW_STATES: [string, Model][] = [
  ['idle', drive(MS(), [])],
  ['sel-dist', drive(MS({ selected: ['dist'] }), [])],
  ['sel-dist-close', drive(MS({ selected: ['dist-close'] }), [])],
  ['sel-area', drive(MS({ selected: ['area'] }), [])],
  ['sel-perim', drive(MS({ selected: ['perim'] }), [])],
  ['sel-group', drive(MS({ selected: ['perim', 'area'] }), [])],
  ['editing-ft', drive(MS({ selected: ['ft'], editing: 'ft' }), [])],
  ['caption-drag', drive(MS({ selected: ['dist'] }), [ed('down', DIST_CAPTION.x, DIST_CAPTION.y), ed('move', DIST_CAPTION.x + 10, DIST_CAPTION.y + 30)])],
  ['caption-drag-perim', drive(MS({ selected: ['perim'] }), [ed('down', PERIM_CAPTION.x, PERIM_CAPTION.y), ed('move', PERIM_CAPTION.x - 20, PERIM_CAPTION.y)])],
  ['leader-drag', drive(MS({ selected: ['dist'] }), [ed('down', 100, 120), ed('move', 110, 150)])],
  ['draft-leader-free', drive(MS({ selected: ['dist-free'], draft: { g: 'leader', id: 'dist-free', start: v(0, 0), delta: 12 } }), [])],
  ['draft-rotate-perim', drive(MS({ selected: ['perim'], draft: { g: 'rotate', ids: ['perim'], pivot: v(450, 330), start: v(450, 250), cur: v(530, 330) } }), [])],
  ['draft-group-perim', drive(MS({ selected: ['perim', 'area'], draft: { g: 'group', op: 'resize', ids: ['perim', 'area'], handle: 'se', anchor: v(100, 300), base: rc(100, 300, 452, 80), cur: rc(100, 300, 500, 100) } }), [])],
  ['draft-leader-area', drive(MS({ selected: ['area'], draft: { g: 'leader', id: 'area', start: v(0, 0), delta: 5 } }), [])],
  ['move-perim', drive(MS({ selected: ['perim'] }), [ed('down', 400, 325), ed('move', 420, 345)])],
  ['move-dist', drive(MS({ selected: ['dist'] }), [ed('down', 150, 120), ed('move', 160, 130)])],
  ['rotate-perim', drive(MS({ selected: ['perim'] }), [ed('down', 450, 250), ed('move', 520, 280)])],
  ['group-resize', drive(MS({ selected: ['perim', 'area'] }), [ed('down', 552, 416), ed('move', 580, 450)])],
  ['create-distance-endpoints', drive(MS(), [cr('line', 'down', 100, 700, {}, DIST_MSG), cr('line', 'move', 200, 700)])],
  ['create-distance-offset', drive(MS(), [cr('line', 'down', 100, 700, {}, DIST_MSG), cr('line', 'up', 200, 700), cr('line', 'move', 150, 680)])],
  ['create-distance-colored', drive(MS({ defaults: { line: { interiorColor: '#abcdef' } } }), [cr('line', 'down', 100, 700, {}, DIST_MSG), cr('line', 'move', 200, 700)])],
  ['create-line-capture', drive(MS(), [cr('line', 'down', 100, 700, {}, { ...DIST_MSG, capture: 'cal' }), cr('line', 'move', 200, 700)])],
  ['create-poly-area', drive(MS(), [cr('polygon', 'down', 100, 650, {}, { measure: area() }), cr('polygon', 'down', 200, 650), cr('polygon', 'move', 150, 750)])],
  ['create-poly-area-invalid', drive(MS(), [cr('polygon', 'down', 100, 650, {}, { measure: area() }), cr('polygon', 'down', 150, 650), cr('polygon', 'down', 200, 650)])],
  ['create-poly-area-valid', drive(MS(), [cr('polygon', 'down', 100, 650, {}, { measure: area() }), cr('polygon', 'down', 200, 650), cr('polygon', 'down', 150, 750)])],
];
{
  const calls = VIEW_STATES.map(([name, m]) => {
    const { byId: _b, order: _o, ...over } = m;
    return { name, over, pon: PON, pageBox: PAGE };
  });
  const page = { byId: VIEW_STATES[0][1].byId, order: VIEW_STATES[0][1].order };
  for (let i = 0; i < calls.length; i += 8) modelCases.push(c('viewMany', `measured-${i / 8}`, page, calls.slice(i, i + 8)));
  modelCases.push(
    c('selectionAnchors', 'measured', page, VIEW_STATES.filter(([n]) => n.startsWith('sel-')).map(([name, m]) => {
      const { byId: _b, order: _o, ...over } = m;
      return { name, over, pageBoxes: { [PON]: PAGE } };
    })),
  );
}

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'core/annotation-measurement.json',
    describes: 'packages/core/annotation measurement.ts, measurement-shape.ts, measurement-font.ts and selection.ts: distance layout, shape captions, readout labels, selection frames; textPlateInset, pointInPoly, geomResetRotation about a pivot.',
    functions: measurementFns,
    cases: measurementCases,
  },
  {
    kind: 'function',
    file: 'core/annotation-richtext.json',
    describes: 'packages/core/annotation richtext.ts: the rich text run algebra over plain-projection offsets.',
    functions: richFns,
    cases: richCases,
  },
  {
    kind: 'function',
    file: 'core/annotation-measurement-model.json',
    describes: 'packages/core/annotation over measured and rich-text annotations: update runs (distance and shape creation, caption and leader drags, createAnnot, setRichText, bold props, callout page slide), hitCalls, scene, props and view states.',
    functions: modelFns,
    cases: modelCases,
  },
];
