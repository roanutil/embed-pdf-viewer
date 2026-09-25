/**
 * `packages/core/stage`. Replayed by
 * `crates/plugins/stage-core/tests/conformance.rs`.
 *
 * Pure functions, so every file is a function suite. A `Scene` carries three
 * query closures: the layout entries here are wrappers that build the scene
 * and run each query on the probes the case supplies, reporting items by
 * index (`null` where the TypeScript returns `undefined`). The anchor entries
 * take the layout that builds their scene.
 */
import * as S from '../../../../packages/core/stage/src/index';
import type { PageGeom, Scene } from '../../../../packages/core/stage/src/index';

import type { FunctionCase, Suite } from '../vectors';

type Fn = (...args: never[]) => unknown;

const c = (fn: string, label: string, ...args: unknown[]): FunctionCase => ({ name: `${fn}/${label}`, fn, args });

interface Probe {
  rects?: S.Rect[];
  points?: S.Point[];
  pageIndexes?: number[];
}

/**
 * The scene's data plus its queries on the probes. A grid with a
 * fractional column count makes `query` push `undefined` for cells that
 * index between items; those holes are dropped here (the Rust port does not
 * produce them, see its README).
 */
function sceneProbe(scene: Scene, probe: Probe = {}) {
  return {
    size: scene.size,
    items: scene.items,
    itemCount: scene.itemCount,
    axis: scene.axis,
    maxItemSize: scene.maxItemSize,
    query: (probe.rects ?? []).map((r) => scene.query(r).filter((it) => it !== undefined).map((it) => it.index)),
    nearestItem: (probe.points ?? []).map((p) => scene.nearestItem(p)?.index ?? null),
    itemOfPage: (probe.pageIndexes ?? []).map((pi) => scene.itemOfPage(pi)),
  };
}

type Layout =
  | { kind: 'linear'; pages: PageGeom[]; grouping: number[][]; opts?: S.LinearOptions }
  | { kind: 'grid'; pages: PageGeom[]; grouping: number[][]; opts?: S.GridOptions };

const build = (l: Layout): Scene =>
  l.kind === 'linear' ? S.linearLayout(l.pages, l.grouping, l.opts) : S.gridLayout(l.pages, l.grouping, l.opts);

const pg = (width: number, height: number, extra: Partial<PageGeom> = {}): PageGeom => ({ size: { width, height }, ...extra });
const rect = (x: number, y: number, width: number, height: number): S.Rect => ({ x, y, width, height });
const pt = (x: number, y: number): S.Point => ({ x, y });
const cam = (x: number, y: number, zoom: number): S.Camera => ({ x, y, zoom });

const vp = { width: 1000, height: 700 };

// ── camera math ─────────────────────────────────────────────────────────────

const cameraFns: Record<string, Fn> = {
  constants: () => ({ ZOOM_MIN: S.ZOOM_MIN, ZOOM_MAX: S.ZOOM_MAX, ZoomMode: S.ZoomMode }),
  toScreen: S.toScreen,
  toWorld: S.toWorld,
  cameraWorldRect: S.cameraWorldRect,
  zoomAround: S.zoomAround,
  panByScreen: S.panByScreen,
  centerOnWorld: S.centerOnWorld,
  travelRange: S.travelRange,
  clampCamera: S.clampCamera,
  scrollMetrics: S.scrollMetrics,
  cameraFromScroll: S.cameraFromScroll,
  placeCamera: S.placeCamera,
  revealCamera: S.revealCamera,
  resolveZoom: S.resolveZoom,
};

const docBounds = rect(0, 0, 800, 5000);
const small = rect(0, 0, 300, 300);
const item = rect(0, 4000, 1000, 800);
const k = (over: Partial<S.CameraConstraint> = {}): S.CameraConstraint => ({ bounded: true, padding: 24, ...over });
const aligns: S.Align[] = ['start', 'center', 'end'];

const camera: FunctionCase[] = [
  c('constants', 'values'),

  c('toScreen', 'basic', cam(100, 50, 2), pt(300, 200)),
  c('toScreen', 'zoom-zero', cam(0, 0, 0), pt(5, 5)),
  c('toScreen', 'nan', cam(NaN, 0, 1), pt(1, 1)),
  c('toWorld', 'basic', cam(100, 50, 2), pt(300, 200)),
  c('toWorld', 'zoom-zero', cam(1, 2, 0), pt(5, -5)),
  c('toWorld', 'zoom-zero-origin', cam(1, 2, 0), pt(0, 0)),
  c('cameraWorldRect', 'basic', cam(10, 20, 2), vp),
  c('cameraWorldRect', 'zoom-zero', cam(10, 20, 0), vp),
  c('cameraWorldRect', 'negative-zoom', cam(10, 20, -1), vp),

  c('zoomAround', 'keeps-cursor', cam(100, 50, 1), pt(300, 200), 2.3),
  c('zoomAround', 'clamps-max', cam(0, 0, 10), pt(500, 350), 100),
  c('zoomAround', 'clamps-min', cam(0, 0, 1), pt(500, 350), 0.001),
  c('zoomAround', 'at-max', cam(0, 0, 64), pt(1, 1), 1),
  c('zoomAround', 'at-min', cam(0, 0, 0.05), pt(1, 1), 1),
  c('zoomAround', 'factor-nan', cam(0, 0, 1), pt(1, 1), NaN),
  c('zoomAround', 'factor-infinite', cam(0, 0, 1), pt(1, 1), Infinity),
  c('zoomAround', 'factor-negative', cam(3, 4, 2), pt(10, 10), -1),
  c('zoomAround', 'zoom-zero', cam(3, 4, 0), pt(10, 10), 2),

  c('panByScreen', 'basic', cam(100, 50, 2), 30, -40),
  c('panByScreen', 'zoom-zero', cam(100, 50, 0), 30, 0),
  c('centerOnWorld', 'basic', pt(500, 1000), vp, 2),
  c('centerOnWorld', 'zoom-zero', pt(500, 1000), vp, 0),
  c('centerOnWorld', 'odd-viewport', pt(-3, 7), { width: 333, height: 1 }, 0.3),

  c('travelRange', 'overflow', 0, 5000, 700, 1, 24),
  c('travelRange', 'fits', 0, 300, 700, 1, 24),
  c('travelRange', 'exact-fit-boundary', 0, 652, 700, 1, 24),
  c('travelRange', 'just-over-boundary', 0, 652.0001, 700, 1, 24),
  c('travelRange', 'zoom-2', 100, 400, 700, 2, 10),
  c('travelRange', 'zoom-zero', 0, 400, 700, 0, 10),
  c('travelRange', 'zero-padding', 50, 100, 100, 1, 0),
  c('travelRange', 'negative-padding', 0, 100, 700, 1, -30),
  c('travelRange', 'nan-content', 0, NaN, 700, 1, 0),

  c('clampCamera', 'unbounded-passthrough', cam(0, 99999, 1), rect(0, 0, 10, 10), vp, { bounded: false, padding: 0 }),
  c('clampCamera', 'bounded-bottom', cam(0, 99999, 1), rect(0, 0, 1000, 5000), vp, k({ padding: 0 })),
  c('clampCamera', 'bounded-top-padded', cam(0, -99999, 1), rect(0, 0, 1000, 5000), vp, k()),
  c('clampCamera', 'bounded-bottom-padded', cam(0, 99999, 1), rect(0, 0, 1000, 5000), vp, k()),
  c('clampCamera', 'within-range-untouched', cam(0, 1234, 1), docBounds, vp, k()),
  c('clampCamera', 'non-zero-origin-top', cam(0, -99999, 1), item, vp, k({ padding: 0 })),
  c('clampCamera', 'non-zero-origin-bottom', cam(0, 99999, 1), item, vp, k({ padding: 0 })),
  c('clampCamera', 'small-item-centered', cam(0, 0, 1), rect(0, 4000, 300, 300), vp, k({ padding: 0 })),
  c('clampCamera', 'fit-width-zoom', cam(-99999, 0, (1000 - 48) / 1000), rect(0, 0, 1000, 5000), vp, k()),
  ...aligns.flatMap((x) =>
    aligns.flatMap((y) =>
      (['ltr', 'rtl', undefined] as const).map((direction) =>
        c('clampCamera', `fit-${x}-${y}-${direction ?? 'default'}`, cam(0, 0, 1), small, vp, k({ fitAlign: { x, y }, direction })),
      ),
    ),
  ),
  c('clampCamera', 'rtl-default-fit-align', cam(0, 0, 1), small, vp, k({ direction: 'rtl' })),
  c('clampCamera', 'zoom-zero', cam(5, 5, 0), small, vp, k()),
  c('clampCamera', 'nan-position', cam(NaN, NaN, 1), docBounds, vp, k()),
  c('clampCamera', 'overflow-x-only', cam(-500, 0, 2), rect(0, 0, 800, 200), vp, k()),

  c('scrollMetrics', 'bounded-top', cam(88, -24, 1), docBounds, vp, 24),
  c('scrollMetrics', 'bounded-bottom', cam(88, 4324, 1), docBounds, vp, 24),
  c('scrollMetrics', 'zoom-2', cam(-24, 100, 2), docBounds, vp, 24),
  c('scrollMetrics', 'unbounded-west', cam(-3000, 0, 1), docBounds, vp, 24),
  c('scrollMetrics', 'unbounded-east', cam(4000, 0, 1), docBounds, vp, 24),
  c('scrollMetrics', 'everything-in-view', cam(-300, -300, 0.1), docBounds, vp, 24),
  c('scrollMetrics', 'half-pixel-slack-in', cam(0, 0, 1), rect(0, 0, 1000.5, 700.25), vp, 0),
  c('scrollMetrics', 'half-pixel-slack-out', cam(0, 0, 1), rect(0, 0, 1000.75, 700.5001), vp, 0),
  c('scrollMetrics', 'zoom-zero', cam(0, 0, 0), docBounds, vp, 24),
  c('scrollMetrics', 'nan-camera', cam(NaN, 0, 1), docBounds, vp, 24),

  c('cameraFromScroll', 'identity', cam(88, 1000, 1), docBounds, vp, 24, { left: 0, top: 1024 }),
  c('cameraFromScroll', 'top-only', cam(88, 1000, 1), docBounds, vp, 24, { top: 2000 }),
  c('cameraFromScroll', 'left-only', cam(88, 1000, 2), docBounds, vp, 24, { left: 150 }),
  c('cameraFromScroll', 'neither', cam(88, 1000, 1), docBounds, vp, 24, {}),
  c('cameraFromScroll', 'beyond-end', cam(88, 1000, 1), docBounds, vp, 24, { top: 1e9 }),
  c('cameraFromScroll', 'negative', cam(88, 1000, 1), docBounds, vp, 24, { left: -50, top: -50 }),
  c('cameraFromScroll', 'unbounded-east-beyond-end', cam(4000, 0, 2), docBounds, vp, 24, { left: 1e9 }),
  c('cameraFromScroll', 'zoom-2-beyond-end', cam(-24, 100, 2), docBounds, vp, 24, { top: 1e9 }),
  c('cameraFromScroll', 'no-range', cam(-300, -300, 0.1), docBounds, vp, 24, { left: 20, top: 20 }),
  c('cameraFromScroll', 'nan-target', cam(0, 0, 1), docBounds, vp, 24, { left: NaN }),
  c('cameraFromScroll', 'explicit-undefined', cam(7, 9, 1), docBounds, vp, 24, { left: undefined, top: 10 }),

  ...aligns.flatMap((x) =>
    aligns.flatMap((y) =>
      (['ltr', 'rtl'] as const).map((d) => c('placeCamera', `${x}-${y}-${d}`, rect(0, 816, 600, 800), vp, 0.5, 24, { x, y }, d)),
    ),
  ),
  c('placeCamera', 'defaults', rect(0, 816, 600, 800), vp, 1),
  c('placeCamera', 'default-align', rect(0, 816, 600, 800), vp, 2, 24),
  c('placeCamera', 'fraction-0.35', rect(0, 816, 600, 800), vp, 1, 24, { x: 'center', y: 0.35 }),
  c('placeCamera', 'fraction-half', rect(0, 816, 600, 800), vp, 1, 24, { x: 0.5, y: 0.5 }),
  c('placeCamera', 'fraction-clamped-high', rect(0, 816, 600, 800), vp, 1, 24, { x: 7, y: -3 }),
  c('placeCamera', 'fraction-rtl-physical', rect(0, 0, 1200, 1600), vp, 1, 24, { x: 0.2, y: 'start' }, 'rtl'),
  c('placeCamera', 'fraction-nan', rect(0, 0, 10, 10), vp, 1, 0, { x: NaN, y: 0 }),
  c('placeCamera', 'rtl-start-overflowing', rect(0, 0, 1200, 1600), vp, 1, 24, { x: 'start', y: 'start' }, 'rtl'),
  c('placeCamera', 'zoom-zero', rect(0, 0, 10, 10), vp, 0, 24),
  c('placeCamera', 'undefined-padding', rect(0, 0, 10, 10), vp, 2, undefined, { x: 'end', y: 'end' }),

  c('revealCamera', 'already-visible', cam(0, 1900, 1), rect(100, 2000, 300, 400), vp, 24),
  c('revealCamera', 'below', cam(0, 0, 1), rect(100, 2000, 300, 400), vp, 24),
  c('revealCamera', 'above', cam(0, 5000, 1), rect(100, 2000, 300, 400), vp, 24),
  c('revealCamera', 'oversized', cam(0, 0, 1), rect(0, 2000, 300, 5000), vp, 24),
  c('revealCamera', 'exactly-fits', cam(0, 0, 1), rect(0, 0, 1000, 700), vp, 0),
  c('revealCamera', 'oversized-by-a-hair', cam(0, 0, 1), rect(0, 0, 1000.000001, 700), vp, 0),
  c('revealCamera', 'default-padding', cam(0, 0, 2), rect(900, 900, 50, 50), vp),
  c('revealCamera', 'left-of-window', cam(5000, 0, 1), rect(100, 10, 50, 50), vp, 10),
  c('revealCamera', 'zoom-zero', cam(0, 0, 0), rect(1, 1, 1, 1), vp, 0),
  c('revealCamera', 'oversized-nan-camera', cam(NaN, NaN, 1), rect(0, 0, 3000, 3000), vp, 0),
  c('revealCamera', 'exactly-fits-nan-camera', cam(NaN, NaN, 1), rect(0, 0, 1000, 700), vp, 0),
  c('revealCamera', 'nan-camera', cam(NaN, 0, 1), rect(1, 1, 1, 1), vp, 0),

  c('resolveZoom', 'fit-width', { mode: 'fit-width' }, { width: 2000, height: 800 }, vp, 16),
  c('resolveZoom', 'fit-page', { mode: 'fit-page' }, { width: 2000, height: 3000 }, vp, 16),
  c('resolveZoom', 'fit-page-width-bound', { mode: 'fit-page' }, { width: 3000, height: 100 }, vp, 16),
  c('resolveZoom', 'fit-all', { mode: 'fit-all' }, { width: 3000, height: 2400 }, vp, 16),
  c('resolveZoom', 'automatic-capped', { mode: 'automatic' }, { width: 400, height: 800 }, vp, 16),
  c('resolveZoom', 'automatic-below', { mode: 'automatic' }, { width: 2000, height: 800 }, vp, 16),
  c('resolveZoom', 'automatic-tall', { mode: 'automatic' }, { width: 400, height: 5000 }, vp, 16),
  c('resolveZoom', 'fit-width-clamped-max', { mode: 'fit-width' }, { width: 1, height: 1 }, vp),
  c('resolveZoom', 'fit-width-clamped-min', { mode: 'fit-width' }, { width: 1e9, height: 1 }, vp),
  c('resolveZoom', 'fit-page-clamped-min', { mode: 'fit-page' }, { width: 1, height: 1e9 }, vp),
  c('resolveZoom', 'fit-page-clamped-max', { mode: 'fit-page' }, { width: 1, height: 1 }, vp),
  c('resolveZoom', 'automatic-clamped-min', { mode: 'automatic' }, { width: 1e9, height: 1 }, vp),
  c('resolveZoom', 'padding-eats-viewport', { mode: 'fit-width' }, { width: 100, height: 100 }, vp, 600),
  c('resolveZoom', 'padding-leaves-exactly-one', { mode: 'fit-page' }, { width: 1, height: 1 }, { width: 3, height: 3 }, 1),
  c('resolveZoom', 'zero-box', { mode: 'fit-width' }, { width: 0, height: 0 }, vp),
  c('resolveZoom', 'zero-box-page', { mode: 'fit-page' }, { width: 0, height: 0 }, vp),
  c('resolveZoom', 'default-padding', { mode: 'fit-page' }, { width: 500, height: 500 }, vp),
  c('resolveZoom', 'level', { level: 2.5 }, { width: 1, height: 1 }, vp),
  c('resolveZoom', 'level-max', { level: 999 }, { width: 1, height: 1 }, vp),
  c('resolveZoom', 'level-min', { level: 0 }, { width: 1, height: 1 }, vp),
  c('resolveZoom', 'level-boundary', { level: 64 }, { width: 1, height: 1 }, vp),
  c('resolveZoom', 'level-nan', { level: NaN }, { width: 1, height: 1 }, vp),
  c('resolveZoom', 'level-wins-over-mode', { level: 3, mode: 'fit-width' }, { width: 1, height: 1 }, vp),
  c('resolveZoom', 'page-width', { pageWidth: 200 }, { width: 612, height: 792 }, vp),
  c('resolveZoom', 'page-width-min', { pageWidth: 1 }, { width: 1e9, height: 1 }, vp),
  c('resolveZoom', 'page-width-max', { pageWidth: 1e6 }, { width: 1, height: 1 }, vp),
  c('resolveZoom', 'page-width-zero-box', { pageWidth: 200 }, { width: 0, height: 1 }, vp),
  c('resolveZoom', 'page-width-wins-over-height', { pageWidth: 200, pageHeight: 10 }, { width: 612, height: 792 }, vp),
  c('resolveZoom', 'page-height', { pageHeight: 150 }, { width: 612, height: 792 }, vp),
  c('resolveZoom', 'page-height-min', { pageHeight: 1 }, { width: 1, height: 1e9 }, vp),
  c('resolveZoom', 'page-height-nan', { pageHeight: NaN }, { width: 1, height: 1 }, vp),
];

// ── layout ──────────────────────────────────────────────────────────────────

const layoutFns: Record<string, Fn> = {
  groupPages: S.groupPages,
  linearLayout: (pages: PageGeom[], grouping: number[][], opts?: S.LinearOptions, probe?: Probe) =>
    sceneProbe(S.linearLayout(pages, grouping, opts), probe),
  gridLayout: (pages: PageGeom[], grouping: number[][], opts?: S.GridOptions, probe?: Probe) =>
    sceneProbe(S.gridLayout(pages, grouping, opts), probe),
};

const three = [pg(600, 800), pg(600, 800), pg(600, 800)];
const mixed = [pg(600, 800), pg(1000, 700), pg(500, 900)];
const seven = Array.from({ length: 7 }, () => pg(600, 800));
const frame = { top: 10, right: 8, bottom: 20, left: 6 };
const g = (n: number, mode?: S.SpreadMode) => S.groupPages(n, mode);

/** Probes spread over a vertical scene of letter-ish pages. */
const vprobe: Probe = {
  rects: [rect(0, 0, 1000, 700), rect(0, 2432, 1, 1), rect(0, 900, 100, 10), rect(0, -500, 10, 10), rect(0, 1e9, 10, 10), rect(0, 0, 0, 0), rect(0, NaN, 1, 1)],
  points: [pt(0, 0), pt(300, 850), pt(-50, -50), pt(0, 1e9), pt(0, NaN), pt(0, 816)],
  pageIndexes: [0, 1, 2, 3, 99],
};
const hprobe: Probe = {
  rects: [rect(0, 0, 700, 1000), rect(900, 0, 10, 100), rect(-500, 0, 10, 10), rect(1e9, 0, 10, 10), rect(NaN, 0, 1, 1)],
  points: [pt(0, 0), pt(850, 300), pt(-50, -50), pt(1e9, 0), pt(NaN, 0), pt(616, 0)],
  pageIndexes: [0, 1, 2, 3, 4],
};
const gprobe: Probe = {
  rects: [
    rect(0, 0, 1000, 700),
    rect(700, 900, 10, 10),
    rect(0, 800, 1, 1),
    rect(-500, -500, 10, 10),
    rect(0, 0, 1e9, 1e9),
    rect(1e9, 1e9, 1, 1),
    rect(NaN, 0, 1, 1),
    rect(0, NaN, 1, 1),
    rect(-Infinity, -Infinity, Infinity, Infinity),
  ],
  points: [pt(0, 0), pt(700, 900), pt(-50, -50), pt(1e9, 1e9), pt(NaN, 0), pt(0, NaN), pt(5000, 10)],
  pageIndexes: [0, 1, 2, 6, 99],
};

const layout: FunctionCase[] = [
  ...(['none', 'odd', 'even', undefined] as const).flatMap((mode) =>
    [0, 1, 2, 3, 4, 5].map((n) => c('groupPages', `${mode ?? 'default'}-${n}`, n, mode)),
  ),
  c('groupPages', 'larger', 11, 'even'),

  c('linearLayout', 'vertical-default', three, g(3), undefined, vprobe),
  c('linearLayout', 'vertical-gap-16', three, g(3), { axis: 'y', gap: 16 }, vprobe),
  c('linearLayout', 'vertical-start-align', mixed, g(3), { align: 'start' }, vprobe),
  c('linearLayout', 'vertical-mixed-center', mixed, g(3), { gap: 16 }, vprobe),
  c('linearLayout', 'vertical-uniform', mixed, g(3), { gap: 16, sizing: 'uniform' }, vprobe),
  c('linearLayout', 'horizontal', three, g(3), { axis: 'x', gap: 16 }, hprobe),
  c('linearLayout', 'horizontal-mixed-center', mixed, g(3), { axis: 'x', gap: 16 }, hprobe),
  c('linearLayout', 'uniform-spreads', [pg(600, 800), pg(300, 800), pg(1000, 700), pg(200, 500)], g(4, 'odd'), { gap: 16, sizing: 'uniform' }, vprobe),
  c('linearLayout', 'horizontal-start', mixed, g(3), { axis: 'x', gap: 16, align: 'start' }, hprobe),
  c('linearLayout', 'horizontal-uniform', mixed, g(3), { axis: 'x', gap: 16, sizing: 'uniform' }, hprobe),
  c('linearLayout', 'horizontal-rtl', [...three, pg(600, 800)], g(4), { axis: 'x', gap: 16, direction: 'rtl' }, hprobe),
  c('linearLayout', 'horizontal-rtl-spreads', [...three, pg(600, 800)], g(4, 'odd'), { axis: 'x', gap: 16, direction: 'rtl' }, hprobe),
  c('linearLayout', 'vertical-rtl-spreads', [...three, pg(600, 800)], g(4, 'odd'), { gap: 16, direction: 'rtl' }, vprobe),
  c('linearLayout', 'vertical-even-spreads', mixed, g(3, 'even'), { gap: 16 }, vprobe),
  c('linearLayout', 'frame', [pg(600, 800), pg(600, 800)], g(2), { gap: 16, pageFrame: frame }, vprobe),
  c('linearLayout', 'frame-spread', [pg(600, 800), pg(600, 800)], g(2, 'odd'), { gap: 16, pageFrame: frame }, vprobe),
  c('linearLayout', 'frame-spread-rtl', [pg(600, 800), pg(500, 600)], g(2, 'odd'), { gap: 16, pageFrame: frame, direction: 'rtl' }, vprobe),
  c('linearLayout', 'frame-uniform', [pg(600, 800), pg(1200, 800)], g(2), { gap: 16, sizing: 'uniform', pageFrame: frame }, vprobe),
  c('linearLayout', 'view-units', [pg(612, 792)], g(1), { viewUnitsPerPoint: 96 / 72 }, vprobe),
  c('linearLayout', 'view-units-uniform', [pg(600, 800), pg(1000, 700)], g(2), { sizing: 'uniform', viewUnitsPerPoint: 96 / 72 }, vprobe),
  c('linearLayout', 'rotations', [pg(600, 800, { rotation: 90 }), pg(600, 800, { rotation: 180 }), pg(600, 800, { rotation: 270 })], g(3), { gap: 16 }, vprobe),
  c('linearLayout', 'rotated-uniform', [pg(600, 800, { rotation: 90 }), pg(700, 500)], g(2), { gap: 16, sizing: 'uniform' }, vprobe),
  c('linearLayout', 'odd-rotation', [pg(600, 800, { rotation: 45 as S.PageRotation }), pg(600, 800, { rotation: -90 as S.PageRotation })], g(2), {}, vprobe),
  c('linearLayout', 'user-unit', [pg(100, 200), pg(100, 200, { userUnit: 5 })], g(2), { gap: 0 }, vprobe),
  c('linearLayout', 'user-unit-view-units', [pg(72, 72, { userUnit: 2 })], g(1), { gap: 0, viewUnitsPerPoint: 96 / 72 }, vprobe),
  c('linearLayout', 'user-unit-null-rotation-null', [pg(72, 72, { userUnit: null as never, rotation: null as never })], g(1), {}, vprobe),
  c('linearLayout', 'user-unit-nan', [pg(72, 72, { userUnit: NaN })], g(1), {}, vprobe),
  c('linearLayout', 'spread-mixed-heights', [pg(600, 800), pg(600, 400), pg(300, 900)], g(3, 'odd'), { gap: 16 }, vprobe),
  c('linearLayout', 'uniform-zero-cross', [pg(0, 100), pg(600, 800)], g(2), { sizing: 'uniform' }, vprobe),
  c('linearLayout', 'uniform-all-zero', [pg(0, 0)], g(1), { sizing: 'uniform' }, vprobe),
  c('linearLayout', 'empty', [], [], {}, vprobe),
  c('linearLayout', 'empty-horizontal-rtl', [], [], { axis: 'x', direction: 'rtl' }, hprobe),
  c('linearLayout', 'one-page', [pg(612, 792)], g(1), {}, vprobe),
  c('linearLayout', 'many-pages', Array.from({ length: 12 }, (_, i) => pg(500 + i * 10, 700 - i * 5)), g(12), { gap: 8 }, {
    rects: [rect(0, 3000, 100, 100), rect(0, 0, 10, 1e9), rect(0, 7000, 1, 1)],
    points: [pt(0, 3000), pt(0, 6000)],
    pageIndexes: [0, 5, 11, 12],
  }),
  c('linearLayout', 'many-spreads-rtl', Array.from({ length: 9 }, () => pg(400, 600)), g(9, 'even'), { axis: 'x', gap: 8, direction: 'rtl' }, {
    rects: [rect(0, 0, 100, 100), rect(2000, 0, 100, 100), rect(4000, 0, 1, 1)],
    points: [pt(0, 0), pt(2000, 0), pt(10000, 0)],
    pageIndexes: [0, 1, 2, 3, 4, 7, 8, 9],
  }),
  c('linearLayout', 'empty-group', [pg(600, 800)], [[], [0]], {}, vprobe),
  c('linearLayout', 'negative-gap', three, g(3), { gap: -20 }, vprobe),
  c('linearLayout', 'nan-gap', three, g(3), { gap: NaN }, vprobe),
  c('linearLayout', 'page-out-of-range', three, [[0], [5]], {}),

  c('gridLayout', 'default', three, g(3), undefined, gprobe),
  c('gridLayout', 'columns-2', [...three, pg(600, 800)], g(4), { gap: 48, columns: 2 }, gprobe),
  c('gridLayout', 'columns-2-rtl', [...three, pg(600, 800)], g(4), { gap: 48, columns: 2, direction: 'rtl' }, gprobe),
  c('gridLayout', 'mixed-rows', [pg(600, 800), pg(600, 400), pg(600, 1200), pg(600, 600)], g(4), { gap: 12, columns: 2 }, gprobe),
  c('gridLayout', 'mixed-widths', [pg(600, 800), pg(900, 700)], g(2), { gap: 48 }, gprobe),
  c('gridLayout', 'uniform', [pg(600, 800), pg(900, 700), pg(300, 300)], g(3), { gap: 48, sizing: 'uniform', columns: 2 }, gprobe),
  c('gridLayout', 'uniform-spreads', [pg(600, 800), pg(300, 800), pg(1000, 700), pg(200, 500), pg(400, 400)], g(5, 'odd'), { gap: 16, sizing: 'uniform' }, gprobe),
  c('gridLayout', 'line-width-1300', seven, g(7), { gap: 48, lineWidth: 1300 }, gprobe),
  c('gridLayout', 'line-width-2000', seven, g(7), { gap: 48, lineWidth: 2000 }, gprobe),
  c('gridLayout', 'line-width-exact', seven, g(7), { gap: 48, lineWidth: 1248 }, gprobe),
  c('gridLayout', 'line-width-narrow', seven, g(7), { gap: 48, lineWidth: 100 }, gprobe),
  c('gridLayout', 'line-width-wide', seven.slice(0, 3), g(3), { gap: 48, lineWidth: 99999 }, gprobe),
  c('gridLayout', 'line-width-rtl', seven, g(7), { gap: 48, lineWidth: 1300, direction: 'rtl' }, gprobe),
  c('gridLayout', 'line-width-beats-columns', seven, g(7), { gap: 48, lineWidth: 2000, columns: 1 }, gprobe),
  c('gridLayout', 'line-width-infinite', seven, g(7), { gap: 48, lineWidth: Infinity }, gprobe),
  c('gridLayout', 'line-width-negative', seven, g(7), { gap: 48, lineWidth: -5000 }, gprobe),
  c('gridLayout', 'spreads', seven, g(7, 'odd'), { gap: 20 }, gprobe),
  c('gridLayout', 'spreads-rtl-frame', seven, g(7, 'even'), { gap: 20, direction: 'rtl', pageFrame: frame }, gprobe),
  c('gridLayout', 'frame', three, g(3), { gap: 10, pageFrame: frame, columns: 3 }, gprobe),
  c('gridLayout', 'view-units', three, g(3), { viewUnitsPerPoint: 2, columns: 2 }, gprobe),
  c('gridLayout', 'rotations', [pg(600, 800, { rotation: 90 }), pg(600, 800, { rotation: 270 }), pg(600, 800, { userUnit: 2 })], g(3), { columns: 2 }, gprobe),
  c('gridLayout', 'columns-zero', three, g(3), { columns: 0 }, gprobe),
  c('gridLayout', 'columns-negative', three, g(3), { columns: -4 }, gprobe),
  c('gridLayout', 'columns-too-many', three, g(3), { columns: 10 }, gprobe),
  c('gridLayout', 'columns-infinite', three, g(3), { columns: Infinity }, gprobe),
  c('gridLayout', 'columns-fractional', seven, g(7), { columns: 2.5, gap: 10 }, gprobe),
  c('gridLayout', 'columns-fractional-rtl', seven, g(7), { columns: 2.5, gap: 10, direction: 'rtl' }, gprobe),
  c('gridLayout', 'tiny-pages-cell-floor', [pg(0.2, 0.3), pg(0.5, 0.1)], g(2), { gap: 1 }, gprobe),
  c('gridLayout', 'empty', [], [], {}, gprobe),
  c('gridLayout', 'empty-rtl-line-width', [], [], { lineWidth: 500, direction: 'rtl' }, gprobe),
  c('gridLayout', 'one', [pg(600, 800)], g(1), {}, gprobe),
  c('gridLayout', 'sixteen', Array.from({ length: 16 }, (_, i) => pg(100 + (i % 3) * 50, 150 + (i % 4) * 30)), g(16), { gap: 8 }, {
    rects: [rect(0, 0, 300, 300), rect(250, 400, 300, 200), rect(0, 700, 1e4, 1)],
    points: [pt(10, 10), pt(400, 500), pt(9999, 9999)],
    pageIndexes: [0, 7, 15, 16],
  }),
  c('gridLayout', 'empty-group', [pg(600, 800)], [[0], []], {}, gprobe),
  c('gridLayout', 'zero-step', three, g(3), { gap: -600, columns: 2 }, gprobe),
  c('gridLayout', 'line-width-nan-throws', three, g(3), { lineWidth: NaN }),
  c('gridLayout', 'gap-nan', three, g(3), { gap: NaN }),
  c('gridLayout', 'columns-nan-throws', three, g(3), { columns: NaN }),
  c('gridLayout', 'line-width-zero-over-zero-throws', three, g(3), { gap: -600, lineWidth: 600 }),
  c('gridLayout', 'page-out-of-range', three, [[0], [3]], {}),
];

// ── anchors ─────────────────────────────────────────────────────────────────

const anchorFns: Record<string, Fn> = {
  anchorAtPoint: (l: Layout, worldPt: S.Point) => S.anchorAtPoint(build(l), worldPt),
  cameraForAnchorAtScreen: (a: S.Anchor, l: Layout, screenPt: S.Point, zoom: number) =>
    S.cameraForAnchorAtScreen(a, build(l), screenPt, zoom),
  anchorFromCamera: (camera: S.Camera, l: Layout, v: S.Size, at?: S.Point) => S.anchorFromCamera(camera, build(l), v, at),
  cameraFromAnchor: (a: S.Anchor, l: Layout, v: S.Size, zoom: number, at?: S.Point) =>
    S.cameraFromAnchor(a, build(l), v, zoom, at),
};

const vertical: Layout = { kind: 'linear', pages: three, grouping: g(3), opts: { gap: 16 } };
const spreads: Layout = { kind: 'linear', pages: [...three, pg(500, 700)], grouping: g(4, 'odd'), opts: { gap: 16 } };
const spreadsRtl: Layout = { kind: 'linear', pages: [...three, pg(500, 700)], grouping: g(4, 'odd'), opts: { gap: 16, direction: 'rtl' } };
const grid: Layout = { kind: 'grid', pages: seven, grouping: g(7), opts: { columns: 3 } };
const emptyScene: Layout = { kind: 'linear', pages: [], grouping: [] };
const emptyGroup: Layout = { kind: 'linear', pages: [pg(10, 10)], grouping: [[]] };
const emptyGrid: Layout = { kind: 'grid', pages: [], grouping: [] };

const anchors: FunctionCase[] = [
  c('anchorAtPoint', 'first-page', vertical, pt(300, 400)),
  c('anchorAtPoint', 'second-page', vertical, pt(150, 1000)),
  c('anchorAtPoint', 'in-gap', vertical, pt(300, 808)),
  c('anchorAtPoint', 'above', vertical, pt(-100, -100)),
  c('anchorAtPoint', 'below', vertical, pt(700, 1e6)),
  c('anchorAtPoint', 'spread-left', spreads, pt(100, 100)),
  c('anchorAtPoint', 'spread-right', spreads, pt(1100, 100)),
  c('anchorAtPoint', 'spread-exact-middle', spreads, pt(608, 100)),
  c('anchorAtPoint', 'spread-rtl', spreadsRtl, pt(100, 900)),
  c('anchorAtPoint', 'spread-nan-x', spreads, pt(NaN, 100)),
  c('anchorAtPoint', 'grid', grid, pt(700, 900)),
  c('anchorAtPoint', 'grid-nan', grid, pt(NaN, 900)),
  c('anchorAtPoint', 'empty-scene-throws', emptyScene, pt(0, 0)),
  c('anchorAtPoint', 'empty-grid-throws', emptyGrid, pt(0, 0)),
  c('anchorAtPoint', 'empty-item-throws', emptyGroup, pt(0, 0)),

  c('cameraForAnchorAtScreen', 'page-1', { pageIndex: 1, fx: 0.25, fy: 0.5 }, vertical, pt(500, 350), 1.3),
  c('cameraForAnchorAtScreen', 'spread-second', { pageIndex: 1, fx: 0.5, fy: 0.5 }, spreads, pt(0, 0), 2),
  c('cameraForAnchorAtScreen', 'spread-rtl-first', { pageIndex: 2, fx: 0, fy: 1 }, spreadsRtl, pt(10, 10), 0.5),
  c('cameraForAnchorAtScreen', 'page-not-in-item', { pageIndex: 99, fx: 0.5, fy: 0.5 }, spreads, pt(0, 0), 1),
  c('cameraForAnchorAtScreen', 'grid-page-beyond', { pageIndex: 40, fx: 0.5, fy: 0.5 }, grid, pt(0, 0), 1),
  c('cameraForAnchorAtScreen', 'grid', { pageIndex: 5, fx: 1, fy: 0 }, grid, pt(100, 100), 0.7),
  c('cameraForAnchorAtScreen', 'zoom-zero', { pageIndex: 0, fx: 0, fy: 0 }, vertical, pt(1, 1), 0),
  c('cameraForAnchorAtScreen', 'empty-scene-throws', { pageIndex: 0, fx: 0, fy: 0 }, emptyScene, pt(0, 0), 1),
  c('cameraForAnchorAtScreen', 'empty-grid-throws', { pageIndex: 0, fx: 0, fy: 0 }, emptyGrid, pt(0, 0), 1),
  c('cameraForAnchorAtScreen', 'empty-item-throws', { pageIndex: 0, fx: 0, fy: 0 }, emptyGroup, pt(0, 0), 1),

  c('anchorFromCamera', 'center', cam(0, 936, 1.3), vertical, vp),
  c('anchorFromCamera', 'at-origin', cam(-100, 300, 0.9), vertical, vp, pt(0, 0)),
  c('anchorFromCamera', 'at-fraction', cam(-100, 300, 0.9), vertical, vp, pt(500, 245)),
  c('anchorFromCamera', 'grid', cam(0, 0, 0.5), grid, vp),
  c('cameraFromAnchor', 'center', { pageIndex: 1, fx: 0.5, fy: 0.3 }, vertical, vp, 1.3),
  c('cameraFromAnchor', 'at-origin', { pageIndex: 0, fx: 0.1, fy: 0.9 }, vertical, vp, 0.9, pt(0, 0)),
  c('cameraFromAnchor', 'spread', { pageIndex: 3, fx: 0.5, fy: 0.5 }, spreads, vp, 1),
];

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'core/stage-camera.json',
    describes: 'packages/core/stage: projection, camera operations, travel and clamp, scroll metrics, placement, reveal, zoom intent.',
    functions: cameraFns,
    cases: camera,
  },
  {
    kind: 'function',
    file: 'core/stage-layout.json',
    describes: 'packages/core/stage: groupPages, linearLayout and gridLayout (scene queries exercised on probes).',
    functions: layoutFns,
    cases: layout,
  },
  {
    kind: 'function',
    file: 'core/stage-anchor.json',
    describes: 'packages/core/stage: anchors captured from and restored to scenes built by the layouts.',
    functions: anchorFns,
    cases: anchors,
  },
];
