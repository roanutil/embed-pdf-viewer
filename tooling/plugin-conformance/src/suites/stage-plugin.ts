/**
 * `packages/plugin/stage`: the capability, its effects and its init, driven
 * through the kernel and the fake engine. Replayed by
 * `crates/plugins/stage/tests/conformance.rs`.
 *
 * Four harness shims live here, and `crates/plugins/stage/src/harness.rs`
 * mirrors each:
 *
 *   - The frame seam. The TypeScript binds `globalThis.requestAnimationFrame`
 *     when the host has one; Node has none, so with no `scheduler` the stage
 *     is frameless (navigation jumps, no rest detector). A config with
 *     `frames: true` injects a `Scheduler` whose frames are 16 ms timers on
 *     the fake clock, stamped with `Date.now()`, handles counting from 1.
 *     `advance` steps drive them.
 *   - `lens: name` registers the stage under plugin id `name` with a token of
 *     that name (`stagePlugin({id, token})`).
 *   - A responsive rule's `when` may be a `$fn` spec: a scripted predicate
 *     that logs the box it is asked about.
 *   - `stage-scroll` is the surface binding's half of interaction: a
 *     document plugin that registers `createScrollHandler(stage, hub,
 *     {panFallback})` with the hub, scoped to the lens as the React and
 *     Angular stages do.
 */
import { definePlugin, type AnyPlugin, type CapabilityToken } from '@embedpdf/core';

import { InteractionToken } from '../../../../packages/plugin/interaction/src/index';
import { StageToken, createScrollHandler, stagePlugin } from '../../../../packages/plugin/stage/src/index';
import type { StagePluginOptions } from '../../../../packages/plugin/stage/src/stage.plugin';
import type { ResponsiveRule, Scheduler, StageCapability } from '../../../../packages/plugin/stage/src/types';

import type { FakeDocumentSpec, FakeEngineSpec, ResponseSpec } from '../fake-engine';
import { registerPlugin, type PluginEnv, type ScenarioCase, type Step } from '../scenario';
import type { Suite } from '../vectors';
import { scriptedCallback, type AnswerSpec } from './acrojs';

// ── the factories ─────────────────────────────────────────────────────────────

/** Frames as 16 ms timers on the fake clock. */
function frames(): Scheduler {
  let next = 0;
  const timers = new Map<number, ReturnType<typeof setTimeout>>();
  return {
    raf: (cb) => {
      const id = ++next;
      timers.set(
        id,
        setTimeout(() => {
          timers.delete(id);
          cb(Date.now());
        }, 16),
      );
      return id;
    },
    caf: (id) => {
      clearTimeout(timers.get(id));
      timers.delete(id);
    },
  };
}

type Scripted = AnswerSpec & { readonly $fn: string };
interface HarnessRule {
  readonly name?: string;
  readonly when: Record<string, unknown> | Scripted;
  readonly settings?: Record<string, unknown>;
}
type HarnessConfig = Omit<StagePluginOptions, 'id' | 'token' | 'scheduler' | 'responsive'> & {
  readonly lens?: string;
  readonly frames?: boolean;
  readonly responsive?: readonly HarnessRule[];
};

const lensToken = (env: PluginEnv, lens: string | undefined): CapabilityToken<StageCapability> =>
  (lens === undefined ? StageToken : env.token(lens)) as CapabilityToken<StageCapability>;

function stageOptions(config: HarnessConfig | undefined, env: PluginEnv): StagePluginOptions {
  const { lens, frames: wantsFrames, responsive, ...settings } = config ?? {};
  return {
    ...settings,
    ...(lens === undefined ? {} : { id: lens, token: lensToken(env, lens) }),
    ...(wantsFrames ? { scheduler: frames() } : {}),
    ...(responsive === undefined
      ? {}
      : {
          responsive: responsive.map(
            (rule) =>
              ({
                ...rule,
                when: '$fn' in rule.when ? scriptedCallback((rule.when as Scripted).$fn, rule.when as Scripted, env) : rule.when,
              }) as ResponsiveRule,
          ),
        }),
  } as StagePluginOptions;
}

registerPlugin('stage', {
  factory: (config, env) => stagePlugin(stageOptions(config as HarnessConfig | undefined, env)) as AnyPlugin,
  tokens: [StageToken],
});

interface ScrollConfig {
  readonly lens?: string;
  readonly panFallback?: boolean;
}

registerPlugin('stage-scroll', {
  factory: (config, env) => {
    const { lens, panFallback } = (config ?? {}) as ScrollConfig;
    const token = lensToken(env, lens);
    return definePlugin({
      id: 'stage-scroll',
      scope: 'document',
      requires: [token, InteractionToken],
      init: (ctx) => {
        const stage = ctx.get(token);
        const hub = ctx.get(InteractionToken);
        const handler = createScrollHandler(stage, hub, panFallback === undefined ? {} : { panFallback });
        ctx.cleanup(hub.registerHandler(handler, { source: stage.lensId() }));
      },
    }) as AnyPlugin;
  },
});

// ── fixtures ──────────────────────────────────────────────────────────────────

const crop = (w: number, h: number) => ({ left: 0, bottom: 0, right: w, top: h });
const page = (pon: number, index: number, w = 600, h = 800, over: Record<string, unknown> = {}) => ({
  index,
  pageObjectNumber: pon,
  label: null,
  size: { width: w, height: h },
  rotation: 0,
  userUnit: 1,
  boxes: { media: crop(w, h), crop: crop(w, h) },
  ...over,
});
const portrait = (n: number) => Array.from({ length: n }, (_, i) => page(i + 1, i));
const FIVE = portrait(5);
const MIXED = [
  page(10, 0, 600, 800, { label: 'i' }),
  page(20, 1, 1000, 700, { label: 'ii' }),
  page(30, 2, 500, 900, { rotation: 90 }),
  page(40, 3, 300, 400, { userUnit: 2, label: 'iv' }),
];

const engine = (pages: unknown[] = FIVE, over: Partial<FakeDocumentSpec> = {}): FakeEngineSpec => ({
  documents: { a: { pages, ...over } },
});

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const open: Step = { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'a', bytes: { $bytes: '' } }] };
const close: Step = { op: 'call', target: 'documents', method: 'close', args: ['a'] };
const st = (method: string, ...args: unknown[]): Step => ({ op: 'call', target: { token: 'stage', doc: 'a' }, method, args });
const lensCall = (lens: string, method: string, ...args: unknown[]): Step => ({
  op: 'call',
  target: { token: lens, doc: 'a' },
  method,
  args,
});
const vp = (width: number, height: number): Step => st('setViewport', { width, height });
const advance = (ms: number): Step => ({ op: 'advance', ms });
const emit = (event: unknown): Step => ({ op: 'emit', doc: 'a', event });
const boot = [start, open, vp(1000, 700)];
const fnSpec = (name: string, answer: AnswerSpec = {}) => ({ $fn: name, ...answer });

type StageConfigJson = Record<string, unknown>;
const stage = (config: StageConfigJson = {}) => [{ factory: 'stage', config: { viewUnitsPerPoint: 1, ...config } }];
const SNAP = ['stage::a'];

const view = (over: Record<string, unknown> = {}) => ({
  flow: 'continuous',
  layout: 'vertical',
  spread: 'none',
  sizing: 'intrinsic',
  columns: 'square',
  bounded: true,
  padding: 24,
  gap: 16,
  pageFrame: { top: 0, right: 0, bottom: 0, left: 0 },
  direction: 'ltr',
  fitAlign: { x: 'center', y: 'center' },
  arrivalAlign: { x: 'start', y: 'start' },
  zoomAlign: { x: 'center', y: 'center' },
  anchorAlign: { x: 'start', y: 'start' },
  viewRotation: 0,
  zoom: { level: 1 },
  scrollBehavior: 'instant',
  viewUnitsPerPoint: 1,
  cursor: 2,
  anchor: { pageIndex: 2, fx: 0.25, fy: 0.5 },
  ...over,
});

const SELECTORS = [
  'camera',
  'viewport',
  'scrollMetrics',
  'pageCount',
  'currentPage',
  'currentItemPages',
  'pages',
  'flow',
  'layout',
  'spread',
  'sizing',
  'columns',
  'bounded',
  'padding',
  'gap',
  'pageFrame',
  'fitAlign',
  'arrivalAlign',
  'zoomAlign',
  'anchorAlign',
  'direction',
  'scrollBehavior',
  'viewRotation',
  'zoomLevel',
  'zoomMode',
  'viewpoint',
  'settings',
  'viewState',
  'lensId',
  'activeRules',
  'cameraInMotion',
];
const readAll = (): Step[] => SELECTORS.map((m) => st(m));

// ── placement and selectors ───────────────────────────────────────────────────

const placementCases: ScenarioCase[] = [
  {
    name: 'places-on-the-first-real-viewport',
    covers:
      'Before placement the screen selectors answer the stable empties; a zero-height viewport is stored but does not place; the first real one runs placeInitial (reset to page 0) and publishes PLACED last.',
    plugins: stage(),
    engine: engine(),
    steps: [
      start,
      open,
      st('visiblePages'),
      st('pageRect', 1),
      st('scrollMetrics'),
      st('pageAt', { x: 10, y: 10 }),
      st('pointOnPage', 1, { x: 10, y: 10 }),
      st('pageToWorld', 1, { x: 0, y: 0 }),
      st('pageRectToScreen', 1, { x: 0, y: 0, width: 10, height: 10 }),
      vp(1000, 0),
      st('camera'),
      vp(0, 700),
      vp(1000, 700),
      st('camera'),
      st('visiblePages'),
      st('scrollMetrics'),
      st('scrollMetrics'),
      st('pageRect', 1),
      st('pageRect', 5),
      st('pageRect', 99),
    ],
    snapshot: SNAP,
  },
  {
    name: 'a-document-with-no-pages',
    covers:
      'With no pages placement never starts from setViewport; an explicit placeInitial publishes PLACED over an empty scene, and every verb and selector is a harmless no-op on it.',
    plugins: stage(),
    engine: engine([]),
    steps: [
      start,
      open,
      vp(1000, 700),
      st('camera'),
      st('placeInitial'),
      st('visiblePages'),
      st('scrollMetrics'),
      st('currentItemPages'),
      st('pageAt', { x: 1, y: 1 }),
      st('pageRect', 1),
      st('viewpoint'),
      st('viewState'),
      st('goToPage', 3),
      st('reveal', 1),
      st('reveal', 1, { zoom: 'fit' }),
      st('next'),
      st('prev'),
      st('doubleTapZoom', { x: 5, y: 5 }),
      st('scrollTo', { top: 10 }),
      st('scrollBy', { top: 10 }),
      st('zoomAround', { x: 5, y: 5 }, 2),
      st('zoomIn'),
      st('panBy', 10, 10),
      st('setCamera', { x: 5, y: 6, zoom: 2 }),
      st('fitWidth'),
      st('fitAll'),
      st('setLayout', 'grid'),
      st('setFlow', 'paged'),
      st('currentItemPages'),
      st('visiblePages'),
      st('refit'),
      st('applyViewState', view({ cursor: 0, anchor: { pageIndex: 0, fx: 0, fy: 0 } })),
      st('fling', 500, 500),
      vp(800, 600),
      st('setFlow', 'paged'),
      st('scrollMetrics'),
      st('setCamera', { x: 1, y: 2, zoom: 1 }),
      st('panBy', 5, 5),
      st('zoomIn'),
      st('fling', 0, 100),
      st('setPadding', 8),
      st('beginGesture', { elastic: true }),
      st('panBy', 5, 5),
      st('endGesture'),
      st('doubleTapZoom', { x: 5, y: 5 }),
      st('scrollTo', { top: 5 }),
    ],
    snapshot: SNAP,
  },
  {
    name: 'initial-view-providers',
    covers:
      'placeInitial asks providers from the highest priority down (a stable sort: equal priorities keep registration order); the first non-null view wins through applyViewState and PLACED follows.',
    plugins: stage(),
    engine: engine(),
    steps: [
      start,
      open,
      st('provideInitialView', 1, fnSpec('low', { returns: view({ cursor: 1, anchor: { pageIndex: 1, fx: 0, fy: 0 } }) })),
      st('provideInitialView', 5, fnSpec('high-null', { returns: null })),
      st('provideInitialView', 3, fnSpec('mid-a', { returns: view({ layout: 'horizontal' }) })),
      st('provideInitialView', 3, fnSpec('mid-b', { returns: view() })),
      st('provideInitialView', 5, fnSpec('high-undefined', {})),
      vp(1000, 700),
      st('currentPage'),
      st('viewState'),
      st('placeInitial'),
    ],
    snapshot: SNAP,
  },
  {
    name: 'providers-all-null-reset',
    covers: 'When every provider answers null placeInitial resets to page 0.',
    plugins: stage(),
    engine: engine(),
    steps: [start, open, st('provideInitialView', 2, fnSpec('none', { returns: null })), vp(1000, 700), st('currentPage')],
    snapshot: SNAP,
  },
  {
    name: 'restored-view-without-a-cursor',
    covers: 'applyViewState with no cursor lands on page 0 (`view.cursor ?? 0`) and restores the anchor there.',
    plugins: stage(),
    engine: engine(),
    steps: [...boot, st('applyViewState', { ...view({ spread: 'odd' }), cursor: { $undefined: true } }), st('currentPage'), st('camera')],
    snapshot: SNAP,
  },
  {
    name: 'selectors',
    covers: 'Every read after the default placement, on the default web settings (96/72 view units per point).',
    plugins: [{ factory: 'stage' }],
    engine: engine(MIXED),
    steps: [start, open, vp(1000, 700), ...readAll(), st('matches', 'compact'), st('matches', 'nope'), st('visiblePages')],
    snapshot: SNAP,
  },
  {
    name: 'hit-testing',
    covers:
      'pageAt inverts the snapped transform of the page under the point (null over a gap); pointOnPage projects unclamped onto a visible page (null when not laid out); pageToWorld and pageRectToScreen go through the quarter-turn matrix; rotation, userUnit and a device ratio of 2 included.',
    plugins: stage({ scrollBehavior: 'instant' }),
    engine: engine(MIXED),
    steps: [
      ...boot,
      st('setDevicePixelRatio', 2),
      st('setDevicePixelRatio', 2),
      st('setDevicePixelRatio', 0),
      st('setDevicePixelRatio', -1),
      st('pageAt', { x: 500, y: 300 }),
      st('pageAt', { x: 5, y: 300 }),
      st('pageAt', { x: 500, y: 10 }),
      st('pointOnPage', 10, { x: -50, y: 900 }),
      st('pointOnPage', 40, { x: 0, y: 0 }),
      st('pointOnPage', 77, { x: 0, y: 0 }),
      st('pageToWorld', 30, { x: 10, y: 20 }),
      st('pageToWorld', 40, { x: 10, y: 20 }),
      st('pageToWorld', 77, { x: 10, y: 20 }),
      st('pageRectToScreen', 30, { x: 10, y: 20, width: 100, height: 50 }),
      st('pageRectToScreen', 77, { x: 10, y: 20, width: 100, height: 50 }),
      st('goToPage', 2),
      st('pageAt', { x: 500, y: 300 }),
      st('pointOnPage', 30, { x: 500, y: 300 }),
      st('pageRect', 30),
      st('pageRect', 40),
      st('toScreen', { x: 100, y: 100 }),
      st('toWorld', { x: 100, y: 100 }),
      st('visiblePages'),
    ],
    snapshot: SNAP,
  },
];

// ── settings, zoom intents, layout ────────────────────────────────────────────

const settingsCases: ScenarioCase[] = [
  {
    name: 'setters-by-effect-class',
    covers:
      'Each setter lands one PATCH and reacts by its registry class: scene settings rebuild and re-anchor at anchorAlign, reclamp settings re-clamp in place, none settings only store, and a flow change re-places the cursor page.',
    plugins: stage({ scrollBehavior: 'instant' }),
    engine: engine(),
    steps: [
      ...boot,
      st('goToPage', 2),
      st('setLayout', 'horizontal'),
      st('camera'),
      st('setLayout', 'vertical'),
      st('setPadding', 40),
      st('setBounded', false),
      st('setCamera', { x: -5000, y: -5000, zoom: 1 }),
      st('setBounded', true),
      st('setFitAlign', { x: 'start', y: 'end' }),
      st('setArrivalAlign', { x: 'center', y: 0.35 }),
      st('setZoomAlign', { x: 'start', y: 'start' }),
      st('setAnchorAlign', { x: 'center', y: 'center' }),
      st('setScrollBehavior', 'smooth'),
      st('setSizing', 'uniform'),
      st('setGap', 40),
      st('setPageFrame', { top: 10, right: 5, bottom: 20, left: 5 }),
      st('setDirection', 'rtl'),
      st('setSpread', 'even'),
      st('setColumns', 3),
      st('setFlow', 'paged'),
      st('setFlow', 'continuous'),
      st('update', { layout: 'grid', bounded: false, zoom: { mode: 'fit-page' }, padding: { $undefined: true } }),
      st('update', {}),
      st('update', { layout: 'grid' }),
      st('settings'),
      ...readAll(),
    ],
    snapshot: SNAP,
  },
  {
    name: 'zoom-intents',
    covers:
      'zoomTo and the fit shorthands are zoom-intent updates that hold the zoomAlign point; zoomIn/zoomOut magnify around it by 1.2 and record the level; zoomMode reads custom for levels and page-width specs.',
    plugins: stage({ scrollBehavior: 'instant' }),
    engine: engine(MIXED),
    steps: [
      ...boot,
      st('goToPage', 1),
      st('zoomTo', { level: 2 }),
      st('zoomMode'),
      st('fitWidth'),
      st('zoomMode'),
      st('fitPage'),
      st('fitAll'),
      st('camera'),
      st('automatic'),
      st('zoomTo', { pageWidth: 500 }),
      st('zoomMode'),
      st('zoomTo', { pageHeight: 300 }),
      st('zoomIn'),
      st('zoomIn'),
      st('zoomOut'),
      st('zoomLevel'),
      st('setZoomAlign', { x: 'end', y: 0.25 }),
      st('zoomIn'),
      st('setDirection', 'rtl'),
      st('setZoomAlign', { x: 'start', y: 'end' }),
      st('zoomOut'),
      st('setZoomAlign', { x: 'end', y: 1.5 }),
      st('zoomOut'),
    ],
    snapshot: SNAP,
  },
  {
    name: 'fit-all',
    covers:
      'Under fit-all the subject is the whole scene: resize, refit and scene changes re-place it (centered even when unbounded), and next/prev always step by item.',
    plugins: stage({ scrollBehavior: 'instant', zoom: { mode: 'fit-all' } }),
    engine: engine(),
    steps: [...boot, st('camera'), st('next'), st('next'), st('prev'), vp(1200, 900), st('setBounded', false), st('setGap', 50), st('refit'), st('goToPage', 4), st('currentPage')],
    snapshot: SNAP,
  },
  {
    name: 'view-rotation',
    covers:
      'rotateView steps a quarter-turn relative (90 or anything else as 270), setViewRotation sets it absolutely; each is an anchor-preserving scene change that rotates every page footprint.',
    plugins: stage({ scrollBehavior: 'instant' }),
    engine: engine(MIXED),
    steps: [
      ...boot,
      st('goToPage', 1),
      st('rotateView', 90),
      st('viewRotation'),
      st('pageRect', 20),
      st('rotateView', -90),
      st('rotateView', -90),
      st('viewRotation'),
      st('setViewRotation', 180),
      st('pageAt', { x: 500, y: 300 }),
      st('setViewRotation', 0),
    ],
    snapshot: SNAP,
  },
  {
    name: 'wrapped-grid',
    covers:
      'A grid with auto columns wraps to the padded viewport line at the effective zoom: a fixed level keys it exactly, resizes re-wrap and keep the page, zoomAround re-pins the focal point when the scene re-wraps, and fit-modes converge through stabilized.',
    plugins: stage({ layout: 'grid', columns: 'auto', zoom: { level: 0.25 }, gap: { px: 8 }, pageFrame: { top: 0, right: 0, bottom: 18, left: 0 }, scrollBehavior: 'instant' }),
    engine: engine(portrait(12)),
    steps: [
      ...boot,
      st('visiblePages'),
      st('goToPage', 9),
      vp(400, 700),
      st('currentPage'),
      st('zoomAround', { x: 100, y: 100 }, 2),
      st('zoomAround', { x: 100, y: 100 }, 1.01),
      st('fitWidth'),
      st('fitAll'),
      st('automatic'),
      st('setPageFrame', { top: 0, right: 0, bottom: 0, left: 0 }),
      st('setGap', { px: 0 }),
      st('zoomTo', { level: 0.5 }),
      st('setColumns', 'square'),
      st('setBounded', false),
      st('setColumns', 'auto'),
      st('zoomAround', { x: 200, y: 300 }, 0.5),
    ],
    snapshot: SNAP,
  },
  {
    name: 'screen-px-settings-under-a-fit-mode',
    covers: 'A {px} gap and a pageFrame under a fit mode convert at the camera zoom and quantize the scene key; the reveal box includes the chrome bands.',
    plugins: stage({ gap: { px: 12 }, pageFrame: { top: 20, right: 0, bottom: 30, left: 0 }, scrollBehavior: 'instant' }),
    engine: engine(),
    steps: [...boot, st('visiblePages'), st('reveal', 3), st('fitWidth'), st('reveal', 0), st('setGap', { px: 0 }), st('reveal', 4), st('setGap', { px: 12 }), st('zoomIn'), st('visiblePages')],
    snapshot: SNAP,
  },
  {
    name: 'paged-flow',
    covers:
      'Paged flow renders a one-item slice at the cursor: next/prev step items, pans never move the cursor, bare reveal navigates, fit-width fits the current page, and the scroller reads the slice.',
    plugins: stage({ flow: 'paged', scrollBehavior: 'instant', zoom: { mode: 'fit-width' } }),
    engine: engine(MIXED),
    steps: [
      ...boot,
      st('visiblePages'),
      st('currentItemPages'),
      st('next'),
      st('zoomLevel'),
      st('panBy', 0, -5000),
      st('currentPage'),
      st('scrollMetrics'),
      st('reveal', 3),
      st('reveal', 0, { behavior: 'instant' }),
      st('reveal', 2, { rect: { x: 10, y: 10, width: 50, height: 50 }, zoom: 'fit' }),
      st('prev'),
      st('setSpread', 'odd'),
      st('currentItemPages'),
      st('next'),
      st('setBounded', false),
      st('panBy', 99999, 99999),
      st('currentPage'),
      st('viewState'),
      st('setFlow', 'continuous'),
      st('currentPage'),
    ],
    snapshot: SNAP,
  },
  {
    name: 'spreads-step-by-item-or-page',
    covers:
      'next/prev step by spread while the spread fits the padded viewport and by page when zoomed in past it, clamping at both ends; currentItemPages reports the spread.',
    plugins: stage({ spread: 'even', scrollBehavior: 'instant', zoom: { mode: 'fit-page' } }),
    engine: engine(FIVE),
    steps: [
      ...boot,
      st('currentItemPages'),
      st('next'),
      st('currentItemPages'),
      st('next'),
      st('next'),
      st('next'),
      st('currentPage'),
      st('zoomTo', { level: 3 }),
      st('prev'),
      st('prev'),
      st('currentPage'),
      st('goToPage', 0),
      st('prev'),
      st('currentPage'),
    ],
    snapshot: SNAP,
  },
  {
    name: 'rtl-horizontal',
    covers: 'Horizontal RTL: items advance leftward, start/start lands top-right, spreads bind on the right, and the focal and anchor points are logical.',
    plugins: stage({ layout: 'horizontal', direction: 'rtl', scrollBehavior: 'instant', spread: 'odd' }),
    engine: engine(FIVE),
    steps: [...boot, st('camera'), st('next'), st('camera'), st('currentPage'), st('zoomIn'), vp(800, 700), st('setAnchorAlign', { x: 'end', y: 'center' }), vp(900, 700), st('viewpoint'), st('visiblePages')],
    snapshot: SNAP,
  },
];

// ── navigation and reveal ─────────────────────────────────────────────────────

const navigationCases: ScenarioCase[] = [
  {
    name: 'go-to-page',
    covers:
      'goToPage is a canonical arrival: it clamps the index, lands the subject at arrivalAlign (a per-call override wins, keep holds an axis), clamps within the true bounds even unbounded, and tags the cause programmatic.',
    plugins: stage({ scrollBehavior: 'instant' }),
    engine: engine(),
    steps: [
      ...boot,
      st('goToPage', 3),
      st('camera'),
      st('goToPage', -4),
      st('goToPage', 99),
      st('goToPage', 1, { arrivalAlign: { y: 0.35 } }),
      st('camera'),
      st('zoomTo', { level: 3 }),
      st('goToPage', 2, { arrivalAlign: { x: 'keep' } }),
      st('goToPage', 3, { arrivalAlign: { x: 'center', y: 'keep' } }),
      st('goToPage', 0, { arrivalAlign: { x: 'end', y: 'end' } }),
      st('setBounded', false),
      st('goToPage', 4),
      st('setCamera', { x: 10, y: 10, zoom: 3 }),
      st('resetView'),
      st('camera'),
    ],
    snapshot: SNAP,
  },
  {
    name: 'viewpoint-round-trip',
    covers: 'viewpoint() captures the anchor at anchorAlign and the zoom intent; goToPage with a viewpoint restores it instead of placing fresh.',
    plugins: stage({ scrollBehavior: 'instant' }),
    engine: engine(),
    steps: [
      ...boot,
      st('goToPage', 2),
      st('zoomAround', { x: 300, y: 300 }, 2.5),
      st('panBy', -40, -120),
      st('viewpoint'),
      st('goToPage', 0),
      st('goToPage', 2, { viewpoint: { anchor: { pageIndex: 2, fx: 0.4, fy: 0.3 }, zoom: { level: 2.5 } } }),
      st('camera'),
      st('goToPage', 1, { viewpoint: { anchor: { pageIndex: 1, fx: 0.1, fy: 0.9 }, zoom: { mode: 'fit-width' } }, behavior: 'smooth' }),
    ],
    snapshot: SNAP,
  },
  {
    name: 'bare-reveal',
    covers:
      'A bare reveal is minimal movement: a visible page leaves the camera alone, an off-screen one scrolls just enough (outer box, smooth or instant without frames), and the cursor never moves.',
    plugins: stage({ scrollBehavior: 'smooth' }),
    engine: engine(),
    steps: [...boot, st('reveal', 0), st('reveal', 3), st('currentPage'), st('reveal', 3), st('reveal', 0, { behavior: 'instant' }), st('reveal', 7), st('reveal', -2), st('camera')],
    snapshot: SNAP,
  },
  {
    name: 'positioned-reveal-zooms',
    covers:
      'A positioned reveal moves the cursor up front and resolves its zoom directive (level, fit, fit-width, fit-height, keep) against the target rect or the whole page; a degenerate point with a fit directive pans only; a resolved zoom becomes the intent.',
    plugins: stage({ scrollBehavior: 'instant' }),
    engine: engine(MIXED),
    steps: [
      ...boot,
      st('reveal', 1, { zoom: 'fit' }),
      st('camera'),
      st('zoomMode'),
      st('reveal', 2, { rect: { x: 50, y: 60, width: 200, height: 100 }, zoom: 'fit' }),
      st('reveal', 3, { rect: { x: 10, y: 10, width: 100, height: 0 }, zoom: 'fit-width', anchor: { y: 'start' } }),
      st('reveal', 0, { rect: { x: 0, y: 300, width: 0, height: 200 }, zoom: 'fit-height', anchor: { x: 'start' } }),
      st('reveal', 1, { rect: { x: 100, y: 100, width: 0, height: 0 }, zoom: { level: 1.5 }, anchor: { x: 'start', y: 'start' } }),
      st('reveal', 1, { rect: { x: 100, y: 100, width: 0, height: 0 }, zoom: 'fit' }),
      st('reveal', 1, { rect: null, zoom: 'keep' }),
      st('reveal', 1, { rect: null, zoom: 'keep', anchor: {} }),
      st('reveal', 2, { zoom: 'fit-width' }),
      st('reveal', 2, { zoom: 'fit-height' }),
      st('reveal', 0, { zoom: { level: 0 } }),
      st('reveal', 0, { zoom: { level: 2 }, behavior: 'smooth' }),
      st('reveal', 9, { zoom: 'fit' }),
    ],
    snapshot: SNAP,
  },
  {
    name: 'positioned-reveal-anchors',
    covers:
      'Anchor axes: keep holds the camera, start/end put the rect edge a padding in, center and fractions put its center at that line (fractions clamp to 0..1); an absent axis is nearest without a zoom change (already visible, above, below, larger than the view) and center with one.',
    plugins: stage({ scrollBehavior: 'instant', zoom: { level: 1 } }),
    engine: engine(),
    steps: [
      ...boot,
      st('goToPage', 1),
      st('reveal', 1, { rect: { x: 100, y: 100, width: 50, height: 50 }, anchor: { x: 'keep', y: 'keep' } }),
      st('reveal', 1, { rect: { x: 100, y: 100, width: 50, height: 50 }, anchor: {} }),
      st('reveal', 1, { rect: { x: 100, y: 700, width: 50, height: 50 }, anchor: {} }),
      st('reveal', 0, { rect: { x: 100, y: 100, width: 50, height: 50 }, anchor: {} }),
      st('reveal', 2, { rect: { x: 0, y: 0, width: 600, height: 800 }, anchor: {} }),
      st('reveal', 2, { rect: { x: 100, y: 200, width: 50, height: 50 }, anchor: { x: 'end', y: 'end' } }),
      st('reveal', 2, { rect: { x: 100, y: 200, width: 50, height: 50 }, anchor: { x: 'center', y: 0.35 } }),
      st('reveal', 2, { rect: { x: 100, y: 200, width: 50, height: 50 }, anchor: { x: -3, y: 7 } }),
      st('reveal', 3, { rect: { x: 100, y: 200, width: 50, height: 50 }, zoom: { level: 2 } }),
      st('reveal', 3, { rect: { x: 100, y: 200, width: 50, height: 50 }, zoom: { level: 2 }, anchor: { x: 'start' } }),
    ],
    snapshot: SNAP,
  },
  {
    name: 'reveal-in-a-wrapped-grid',
    covers: 'A positioned reveal whose zoom re-wraps a wrapped grid re-places against the new scene (instant: stabilized; smooth without frames: one recompute).',
    plugins: stage({ layout: 'grid', columns: 'auto', scrollBehavior: 'instant', zoom: { level: 0.3 } }),
    engine: engine(portrait(10)),
    steps: [...boot, st('reveal', 7, { zoom: { level: 0.6 } }), st('reveal', 2, { zoom: { level: 0.2 }, behavior: 'smooth' }), st('reveal', 9, { zoom: 'fit' })],
    snapshot: SNAP,
  },
];

// ── manipulation ──────────────────────────────────────────────────────────────

const manipulationCases: ScenarioCase[] = [
  {
    name: 'camera-writes',
    covers:
      'setCamera and panBy clamp, tag the cause user and sync the cursor from the camera; unbounded they roam freely; the cause flips back to programmatic on navigation only when it changed.',
    plugins: stage({ scrollBehavior: 'instant' }),
    engine: engine(),
    steps: [
      ...boot,
      st('setCamera', { x: 0, y: 99999, zoom: 1 }),
      st('currentPage'),
      st('panBy', 0, 2000),
      st('panBy', 0, 2000),
      st('currentPage'),
      st('goToPage', 1),
      st('goToPage', 2),
      st('setBounded', false),
      st('setCamera', { x: 99999, y: -99999, zoom: 2 }),
      st('camera'),
      st('zoomAround', { x: 0, y: 0 }, 1000),
      st('zoomAround', { x: 0, y: 0 }, 0),
    ],
    snapshot: SNAP,
  },
  {
    name: 'scroller',
    covers:
      'scrollMetrics reads the camera as a DOM scroller over the clamp range; scrollTo is absolute and clamped with omitted axes held; scrollBy adds to the current offsets; both sync the cursor; smooth without frames is instant.',
    plugins: stage({ scrollBehavior: 'instant' }),
    engine: engine(),
    steps: [
      ...boot,
      st('scrollMetrics'),
      st('scrollTo', { top: 1500 }),
      st('currentPage'),
      st('scrollMetrics'),
      st('scrollTo', { left: 5, top: -50 }),
      st('scrollBy', { top: 900 }),
      st('scrollBy', { left: 10 }),
      st('scrollBy', {}),
      st('zoomTo', { level: 3 }),
      st('scrollMetrics'),
      st('scrollTo', { left: 99999, top: 99999, behavior: 'smooth' }),
      st('scrollMetrics'),
      st('setBounded', false),
      st('panBy', 500, 500),
      st('scrollMetrics'),
      st('setBounded', true),
      st('scrollTo', { left: 100, top: 100 }),
      st('scrollBy', { left: 30, top: 40 }),
      st('scrollMetrics'),
    ],
    snapshot: SNAP,
  },
  {
    name: 'gestures-without-frames',
    covers:
      'A gesture bracket defers the zoom-intent PATCH to the outermost endGesture; a pan-only gesture commits nothing; an unmatched endGesture is ignored; an elastic pan stretches past the clamp on the rubber curve (a fitting axis stays rigid) and the release snaps home when there are no frames; fling without frames snaps into bounds.',
    plugins: stage({ scrollBehavior: 'instant' }),
    engine: engine(),
    steps: [
      ...boot,
      st('endGesture'),
      st('beginGesture'),
      st('beginGesture'),
      st('zoomAround', { x: 500, y: 350 }, 1.5),
      st('zoomAround', { x: 500, y: 350 }, 1.1),
      st('endGesture'),
      st('zoomMode'),
      st('endGesture'),
      st('zoomMode'),
      st('beginGesture', {}),
      st('panBy', 0, -100),
      st('endGesture'),
      st('beginGesture', { elastic: true }),
      st('panBy', 300, 400),
      st('camera'),
      st('panBy', 0, 200),
      st('zoomAround', { x: 500, y: 350 }, 1.2),
      st('panBy', 0, 100),
      st('endGesture'),
      st('camera'),
      st('fling', 0, 3000),
      st('beginGesture', { elastic: false }),
      st('panBy', 0, 400),
      st('endGesture'),
    ],
    snapshot: SNAP,
  },
  {
    name: 'double-tap-ladder-without-frames',
    covers:
      'doubleTapZoom climbs automatic, fit-width, 2.5x automatic and wraps, committing each target as a level up front; off the ladder it resets to the base rung; without frames each tap lands at once, focal point held.',
    plugins: stage({ scrollBehavior: 'instant' }),
    engine: engine([page(1, 0, 1200, 800), page(2, 1, 1200, 800)]),
    steps: [
      ...boot,
      st('zoomLevel'),
      st('doubleTapZoom', { x: 500, y: 300 }),
      st('zoomLevel'),
      st('doubleTapZoom', { x: 500, y: 300 }),
      st('zoomLevel'),
      st('doubleTapZoom', { x: 500, y: 300 }),
      st('zoomLevel'),
      st('zoomTo', { level: 1.4 }),
      st('doubleTapZoom', { x: 100, y: 100 }),
      st('zoomLevel'),
    ],
    snapshot: SNAP,
  },
];

const mutationEdgeCases: ScenarioCase[] = [
  {
    name: 'scroller-memo-per-field',
    covers: 'The scroller memo answers the new metrics when any one of its six numbers moves alone: top, height, width, client height and client width.',
    plugins: stage({ scrollBehavior: 'instant' }),
    engine: engine(),
    steps: [
      ...boot,
      st('scrollMetrics'),
      st('panBy', 0, -100),
      st('scrollMetrics'),
      st('setBounded', false),
      st('panBy', 0, 5000),
      st('scrollMetrics'),
      st('panBy', 0, 100),
      st('scrollMetrics'),
      st('panBy', 5000, 0),
      st('scrollMetrics'),
      st('panBy', 100, 0),
      st('scrollMetrics'),
      st('setBounded', true),
      st('zoomTo', { level: 3 }),
      st('scrollTo', { left: 100, top: 100 }),
      st('scrollMetrics'),
      vp(1000, 690),
      st('scrollMetrics'),
      vp(990, 690),
      st('scrollMetrics'),
    ],
    snapshot: SNAP,
  },
  {
    name: 'paged-slice-rekeys',
    covers: 'In paged flow the next slice can land the camera exactly where it was; the scene key still moves, so the visible pages follow.',
    plugins: stage({ flow: 'paged', scrollBehavior: 'instant' }),
    engine: engine(),
    steps: [...boot, st('visiblePages'), st('camera'), st('next'), st('camera'), st('visiblePages')],
    snapshot: SNAP,
  },
  {
    name: 'the-fits-predicate-width',
    covers:
      'next steps by spread only while the spread fits the padded viewport width with half a pixel of slack: zooms just inside, just outside and around the padding bound.',
    plugins: stage({ scrollBehavior: 'instant', spread: 'odd' }),
    engine: engine(Array.from({ length: 7 }, (_, i) => page(i + 1, i, 400, 300))),
    steps: [
      ...boot,
      ...[1.1666, 1.1674, 1.18, 0.5, 1.2].flatMap((level) => [st('zoomTo', { level }), st('goToPage', 1), st('next'), st('currentPage')]),
    ],
    snapshot: 'none',
  },
  {
    name: 'the-fits-predicate-height',
    covers: 'The same predicate on a spread whose height binds.',
    plugins: stage({ scrollBehavior: 'instant', spread: 'odd' }),
    engine: engine(Array.from({ length: 7 }, (_, i) => page(i + 1, i, 300, 800))),
    steps: [
      ...boot,
      ...[0.815, 0.816, 0.83, 0.3, 0.9].flatMap((level) => [st('zoomTo', { level }), st('goToPage', 1), st('next'), st('currentPage')]),
    ],
    snapshot: 'none',
  },
  {
    name: 'positioned-reveal-anchors-zoomed',
    covers:
      'Reveal anchors at zoom 2 with a padding of 30, where dividing and multiplying by the zoom differ; a nearest reveal whose extent is exactly the padded view end-aligns.',
    plugins: stage({ scrollBehavior: 'instant', zoom: { level: 2 }, padding: 30 }),
    engine: engine(),
    steps: [
      ...boot,
      st('goToPage', 1),
      st('camera'),
      st('reveal', 1, { rect: { x: 50, y: 10, width: 40, height: 320 }, anchor: { x: 'keep' } }),
      st('goToPage', 1),
      st('reveal', 1, { rect: { x: 50, y: 700, width: 40, height: 40 }, anchor: { x: 'keep' } }),
      st('reveal', 1, { rect: { x: 50, y: 10, width: 40, height: 40 }, anchor: { x: 'keep' } }),
      st('reveal', 1, { rect: { x: 50, y: 10, width: 40, height: 1000 }, anchor: { x: 'keep' } }),
      st('reveal', 2, { rect: { x: 500, y: 300, width: 40, height: 40 }, anchor: {} }),
      st('reveal', 2, { rect: { x: 10, y: 300, width: 40, height: 40 }, anchor: {} }),
      st('reveal', 2, { rect: { x: 100, y: 200, width: 50, height: 50 }, anchor: { x: 'start', y: 'end' } }),
      st('reveal', 2, { rect: { x: 100, y: 200, width: 50, height: 50 }, anchor: { x: 'end', y: 'start' } }),
      st('reveal', 2, { rect: { x: 100, y: 200, width: 50, height: 50 }, anchor: { x: 0.25, y: 0.75 } }),
      st('reveal', 3, { rect: { x: 100, y: 200, width: 50, height: 500 }, zoom: 'fit' }),
      st('reveal', 3, { rect: { x: 100, y: 200, width: 400, height: 50 }, zoom: 'fit' }),
    ],
    snapshot: SNAP,
  },
  {
    name: 'bare-reveal-outer-box',
    covers: 'A bare reveal of pages smaller than the viewport scrolls the outer box, pageFrame bands on all four sides, horizontally and vertically.',
    plugins: stage({ scrollBehavior: 'instant', layout: 'horizontal', zoom: { level: 1 }, pageFrame: { top: 10, right: 20, bottom: 30, left: 40 } }),
    engine: engine(Array.from({ length: 8 }, (_, i) => page(i + 1, i, 300, 200))),
    steps: [...boot, st('reveal', 6), st('camera'), st('reveal', 0), st('camera'), st('setLayout', 'vertical'), st('reveal', 7), st('camera'), st('reveal', 1), st('camera')],
    snapshot: 'none',
  },
  {
    name: 'focal-points-in-ltr',
    covers: 'Named zoomAlign stops are physical in LTR: end holds the right padded edge, start the left.',
    plugins: stage({ scrollBehavior: 'instant' }),
    engine: engine(),
    steps: [...boot, st('zoomTo', { level: 3 }), st('setZoomAlign', { x: 'end', y: 'start' }), st('zoomIn'), st('camera'), st('setZoomAlign', { x: 'start', y: 'end' }), st('zoomOut'), st('camera')],
    snapshot: 'none',
  },
  {
    name: 'double-tap-on-a-phone',
    covers: 'On a narrow box automatic is fit-width: the ladder dedupes to two rungs, and the top rung is within 1.1 of nothing else.',
    plugins: stage({ scrollBehavior: 'instant', responsive: [] }),
    engine: engine(),
    steps: [start, open, vp(348, 700), st('zoomLevel'), st('doubleTapZoom', { x: 100, y: 100 }), st('zoomLevel'), st('doubleTapZoom', { x: 100, y: 100 }), st('zoomLevel'), st('doubleTapZoom', { x: 100, y: 100 }), st('zoomLevel')],
    snapshot: 'none',
  },
  {
    name: 'a-caught-extreme-stretch',
    covers: 'Catching a stretch near the rubber asymptote caps the overshoot just under the viewport dimension so the inverse stays finite.',
    plugins: stage({ scrollBehavior: 'instant', zoom: { level: 2 } }),
    engine: engine(),
    steps: [...boot, st('beginGesture', { elastic: true }), st('panBy', 100000, 100000), st('camera'), st('zoomAround', { x: 500, y: 350 }, 1.1), st('panBy', 0, 0), st('camera'), st('endGesture')],
    snapshot: 'none',
  },
];

// ── responsive ────────────────────────────────────────────────────────────────

const responsiveCases: ScenarioCase[] = [
  {
    name: 'default-compact-rule',
    covers:
      'The default rule gives a compact box (width at most 600) a padding of 4 and names it; crossing back releases it to the base; activeRules only republishes when the names change.',
    plugins: stage({ scrollBehavior: 'instant' }),
    engine: engine(),
    steps: [start, open, vp(500, 700), st('padding'), st('matches', 'compact'), vp(550, 700), vp(1000, 700), st('padding'), st('activeRules'), vp(400, 400)],
    snapshot: SNAP,
  },
  {
    name: 'rules-and-the-base',
    covers:
      'Setters write the base and a matching rule wins until it stops matching; only keys whose effective value changed re-assert; a predicate rule sees the box; setResponsive swaps rules and re-resolves; applyViewState writes the base under the rules.',
    plugins: stage({
      scrollBehavior: 'instant',
      responsive: [
        { name: 'narrow', when: { maxWidth: 700 }, settings: { padding: 2, spread: 'odd' } },
        { name: 'portrait', when: { orientation: 'portrait' } },
        { when: fnSpec('tall?', { seq: [{ returns: false }, { returns: true }, { returns: 1 }, { returns: 0 }] }), settings: { gap: 4 } },
      ],
    }),
    engine: engine(),
    steps: [
      start,
      open,
      vp(1000, 700),
      st('setPadding', 30),
      st('padding'),
      vp(600, 900),
      st('padding'),
      st('setPadding', 50),
      st('padding'),
      st('activeRules'),
      st('zoomAround', { x: 100, y: 100 }, 2),
      vp(600, 950),
      st('zoomLevel'),
      vp(1000, 700),
      st('padding'),
      st('setResponsive', [{ name: 'always', when: {}, settings: { bounded: false } }]),
      st('bounded'),
      st('setResponsive', []),
      st('bounded'),
      st('activeRules'),
      st('setResponsive', [{ name: 'wide', when: { minWidth: 900 }, settings: { padding: 7 } }]),
      st('applyViewState', view({ padding: 60 })),
      st('padding'),
    ],
    snapshot: SNAP,
  },
  {
    name: 'a-rule-flips-the-flow',
    covers: 'A rule that changes flow at a viewport crossing escalates the resize to a re-placement on the cursor page; one that changes a scene setting rebuilds.',
    plugins: stage({
      scrollBehavior: 'instant',
      responsive: [
        { name: 'phone', when: { maxWidth: 500 }, settings: { flow: 'paged' } },
        { name: 'landscape', when: { orientation: 'landscape', minWidth: 1100 }, settings: { spread: 'odd' } },
      ],
    }),
    engine: engine(),
    steps: [...boot, st('goToPage', 3), vp(400, 700), st('flow'), st('currentPage'), vp(1000, 700), st('flow'), vp(1200, 700), st('spread')],
    snapshot: SNAP,
  },
  {
    name: 'responsive-opt-out',
    covers: 'responsive: [] opts out: a compact box keeps the base padding and no rule is active.',
    plugins: stage({ responsive: [] }),
    engine: engine(),
    steps: [start, open, vp(400, 700), st('padding'), st('activeRules')],
    snapshot: SNAP,
  },
];

// ── edges ─────────────────────────────────────────────────────────────────────

const edgeCases: ScenarioCase[] = [
  {
    name: 'rigid-axes-under-an-elastic-gesture',
    covers:
      'An axis whose content fits the viewport has no rubber: an elastic drag moves only the travelling axis, and a fling discards the fitting axis velocity (without frames it snaps into bounds).',
    plugins: stage({ scrollBehavior: 'instant' }),
    engine: engine(),
    steps: [...boot, st('beginGesture', { elastic: true }), st('panBy', 300, 400), st('camera'), st('endGesture'), st('camera')],
    snapshot: SNAP,
  },
  {
    name: 'a-page-without-a-user-unit',
    covers: 'A registry entry without userUnit reads as 1 in the page transform baseline.',
    plugins: stage(),
    engine: engine([page(1, 0, 600, 800, { userUnit: { $undefined: true } })]),
    steps: [...boot, st('pageRect', 1), st('visiblePages')],
    snapshot: SNAP,
  },
  {
    name: 'double-tap-in-paged-flow',
    covers: 'In paged flow the double-tap ladder resolves against the current slice item.',
    plugins: stage({ flow: 'paged', scrollBehavior: 'instant' }),
    engine: engine(MIXED),
    steps: [...boot, st('next'), st('doubleTapZoom', { x: 300, y: 300 }), st('zoomLevel')],
    snapshot: SNAP,
  },
];

// ── lenses ────────────────────────────────────────────────────────────────────

const lensCases: ScenarioCase[] = [
  {
    name: 'two-lenses',
    covers: 'stagePlugin({id, token}) registers an independent lens: its own slice, camera and settings, and lensId reports its id.',
    plugins: [
      { factory: 'stage', config: { viewUnitsPerPoint: 1, scrollBehavior: 'instant' } },
      { factory: 'stage', config: { lens: 'stage-thumbs', layout: 'grid', columns: 'auto', zoom: { level: 0.2 }, gap: { px: 8 } } },
    ],
    engine: engine(),
    steps: [
      ...boot,
      lensCall('stage-thumbs', 'setViewport', { width: 240, height: 700 }),
      lensCall('stage-thumbs', 'lensId'),
      st('lensId'),
      st('goToPage', 3),
      lensCall('stage-thumbs', 'currentPage'),
      lensCall('stage-thumbs', 'reveal', 3),
      lensCall('stage-thumbs', 'camera'),
    ],
    snapshot: ['stage::a', 'stage-thumbs::a'],
  },
];

// ── motion over frames ────────────────────────────────────────────────────────

const framed = (config: StageConfigJson = {}) => stage({ frames: true, ...config });

const motionCases: ScenarioCase[] = [
  {
    name: 'rest-detector',
    covers:
      'Placement snaps at once; a zoom write afterwards un-rests the camera and a frame chain declares rest after 150 ms of frames, re-armed by each further zoom; pans never un-rest.',
    plugins: framed(),
    engine: engine(),
    steps: [
      ...boot,
      st('panBy', 0, -50),
      st('zoomIn'),
      advance(16),
      st('visiblePages'),
      advance(64),
      st('zoomIn'),
      advance(100),
      advance(100),
      st('cameraInMotion'),
      st('visiblePages'),
    ],
    snapshot: SNAP,
  },
  {
    name: 'smooth-navigation-tween',
    covers:
      'A smooth goToPage tweens x, y and zoom on easeOutCubic over 240 ms, clamping each frame to the target bounds; cameraInMotion reads true until the last frame; the cursor is set up front.',
    plugins: framed(),
    engine: engine(),
    steps: [...boot, st('goToPage', 3), st('cameraInMotion'), st('currentPage'), advance(16), advance(48), advance(96), advance(96), st('cameraInMotion'), advance(32)],
    snapshot: SNAP,
  },
  {
    name: 'caught-tween',
    covers: 'Every verb cancels a running tween first: a setCamera mid-flight stops it where it is and no stale frame lands later.',
    plugins: framed(),
    engine: engine(),
    steps: [...boot, st('next'), advance(48), st('setCamera', { x: 0, y: 500, zoom: 1 }), st('cameraInMotion'), advance(300), st('next'), advance(32), vp(900, 700), advance(300)],
    snapshot: SNAP,
  },
  {
    name: 'smooth-scroll-and-reveal',
    covers:
      'A smooth scrollTo tweens and syncs the cursor only on arrival; a smooth bare reveal tweens against the scene; a smooth positioned reveal commits its zoom intent then tweens.',
    plugins: framed(),
    engine: engine(),
    steps: [
      ...boot,
      st('scrollTo', { top: 2500, behavior: 'smooth' }),
      advance(128),
      st('currentPage'),
      advance(160),
      st('currentPage'),
      st('reveal', 0),
      advance(300),
      st('reveal', 4, { rect: { x: 100, y: 100, width: 100, height: 100 }, zoom: 'fit' }),
      st('zoomMode'),
      advance(300),
    ],
    snapshot: SNAP,
  },
  {
    name: 'animated-double-tap',
    covers:
      'doubleTapZoom tweens the zoom geometrically around the tapped point, holding it fixed each frame, syncs the cursor on arrival; a tap mid-tween climbs from where the intent says it was going.',
    plugins: framed(),
    engine: engine([page(1, 0, 1200, 800), page(2, 1, 1200, 800)]),
    steps: [...boot, st('doubleTapZoom', { x: 500, y: 300 }), advance(64), advance(64), advance(160), st('zoomLevel'), st('doubleTapZoom', { x: 200, y: 600 }), advance(32), st('doubleTapZoom', { x: 200, y: 600 }), advance(300)],
    snapshot: SNAP,
  },
  {
    name: 'fling-glide-and-bounce',
    covers:
      'fling glides on the UIScrollView decay, syncing the cursor each frame; reaching an edge converts to a critically-damped spring that lands exactly on the clamp; a fitting axis discards its velocity; a slow fling starts nothing; a gesture catches a glide.',
    plugins: framed(),
    engine: engine(),
    steps: [
      ...boot,
      st('fling', 0, -3000),
      advance(160),
      st('cameraInMotion'),
      st('currentPage'),
      st('beginGesture'),
      st('cameraInMotion'),
      st('endGesture'),
      st('fling', 400, 5),
      st('cameraInMotion'),
      st('fling', 5, 5),
      st('cameraInMotion'),
      st('fling', 0, 8000),
      advance(400),
      advance(400),
      advance(1000),
      st('camera'),
      st('cameraInMotion'),
      advance(1000),
    ],
    snapshot: SNAP,
  },
  {
    name: 'elastic-release-springs-home',
    covers:
      'An elastic gesture held past the edge springs home on release; catching it mid-bounce holds the stretch and hands it back to the finger through the inverse curve; a release inside the bounds starts nothing.',
    plugins: framed(),
    engine: engine(),
    steps: [
      ...boot,
      st('zoomTo', { level: 2 }),
      advance(200),
      st('beginGesture', { elastic: true }),
      st('panBy', 200, 300),
      st('endGesture'),
      advance(48),
      st('beginGesture', { elastic: true }),
      st('panBy', 0, 10),
      st('camera'),
      st('endGesture'),
      advance(800),
      st('camera'),
      st('beginGesture', { elastic: true }),
      st('panBy', 0, -20),
      st('endGesture'),
      st('cameraInMotion'),
    ],
    snapshot: SNAP,
  },
  {
    name: 'gesture-holds-the-rest-countdown',
    covers: 'A zoom inside a gesture un-rests at once and cancels the countdown; endGesture commits the level and arms it.',
    plugins: framed(),
    engine: engine(),
    steps: [...boot, st('zoomIn'), advance(32), st('beginGesture'), st('zoomAround', { x: 10, y: 10 }, 1.3), advance(300), st('zoomAround', { x: 10, y: 10 }, 1.1), st('endGesture'), advance(100), advance(100)],
    snapshot: SNAP,
  },
  {
    name: 'fling-on-a-short-wide-page',
    covers: 'A fling over a page that fits vertically but travels horizontally glides only on x.',
    plugins: framed({ zoom: { level: 1 } }),
    engine: engine([page(1, 0, 2400, 300)]),
    steps: [...boot, st('fling', -2000, 3000), advance(160), st('camera'), advance(2000), st('cameraInMotion')],
    snapshot: SNAP,
  },
  {
    name: 'zoomed-fling',
    covers: 'At zoom 2 the integrator works in screen px: positions scale by the zoom once, the edge conversion compares in px; a release of exactly 20 px/s still glides.',
    plugins: framed({ zoom: { level: 2 } }),
    engine: engine(),
    steps: [...boot, st('fling', 1500, -2500), advance(96), st('camera'), advance(2000), st('camera'), st('fling', 0, 20), st('cameraInMotion'), advance(100)],
    snapshot: SNAP,
  },
  {
    name: 'paged-reveal-tweens',
    covers: 'A bare reveal in paged flow navigates with the reveal behavior: smooth tweens even when the setting is instant.',
    plugins: framed({ flow: 'paged', scrollBehavior: 'instant' }),
    engine: engine(),
    steps: [...boot, st('reveal', 2, { behavior: 'smooth' }), st('cameraInMotion'), advance(300)],
    snapshot: SNAP,
  },
  {
    name: 'fit-all-with-frames',
    covers: 'Under fit-all with frames, next and prev tween the whole scene and resizes re-place at once.',
    plugins: framed({ zoom: { mode: 'fit-all' } }),
    engine: engine(),
    steps: [...boot, st('next'), advance(300), vp(900, 800), advance(300)],
    snapshot: SNAP,
  },
];

// ── effects, actions and interaction ──────────────────────────────────────────

const layoutEvent = (type: string, pages: unknown[]) => ({ type, layout: { pageCount: pages.length, pages } });

const FORMS_EMPTY: ResponseSpec = { path: 'forms.list', value: { formKind: 'none', needsAppearances: false, fields: [], calculationOrder: [] } };
const ANNOTS_EMPTY: ResponseSpec = { path: 'page.annotations.list', value: { annotations: [] } };
const withActions = (pages: unknown[]): FakeEngineSpec => ({
  documents: { a: { pages, security: { allows: '*' }, responses: [FORMS_EMPTY, ANNOTS_EMPTY] } },
});
const ACTIONS_OFF = { factory: 'actions', config: { openSequence: 'off' } };
const act = (method: string, ...args: unknown[]): Step => ({ op: 'call', target: { token: 'actions', doc: 'a' }, method, args });
const tree = (root: unknown) => ({ root, incomplete: false, warningFlags: 0, warnings: [] });
const goto = (destination: Record<string, unknown>) => ({ type: 'goto', subtype: 'GoTo', destination, next: [] });
const named = (name: string) => ({ type: 'named', subtype: 'Named', name, next: [] });
const USER = { origin: 'user', source: { kind: 'api' }, event: { scope: 'activate' } };

const interaction = { factory: 'interaction' };
const ix = (method: string, ...args: unknown[]): Step => ({ op: 'call', target: { token: 'interaction', doc: 'a' }, method, args });
const mods = { shift: false, alt: false, ctrl: false, meta: false };
const sample = (phase: string, x: number, y: number, onPage: boolean) => ({
  phase,
  viewport: { x, y },
  modifiers: mods,
  ...(onPage ? { page: { pon: 1, point: { x: 1, y: 1 } } } : {}),
  source: 'stage',
});

const feedCases: ScenarioCase[] = [
  {
    name: 'page-mutations-refit',
    covers:
      'A page-registry event bumps the document revision and the effect refits: the rotated footprint re-resolves the zoom intent and the anchor holds; before placement the refit is a no-op.',
    plugins: stage({ scrollBehavior: 'instant', zoom: { mode: 'fit-page' } }),
    engine: engine(portrait(3)),
    steps: [
      start,
      open,
      emit(layoutEvent('pages.rotated', [page(1, 0, 600, 800, { rotation: 90 }), page(2, 1), page(3, 2)])),
      vp(1000, 700),
      st('goToPage', 1),
      emit(layoutEvent('pages.rotated', [page(1, 0), page(2, 1, 600, 800, { rotation: 90 }), page(3, 2)])),
      st('pageRect', 2),
      emit(layoutEvent('pages.deleted', [page(1, 0), page(3, 1)])),
      st('pageCount'),
      st('pages'),
      emit({ type: 'annotations.created', pageObjectNumber: 1 }),
      close,
    ],
    snapshot: SNAP,
  },
  {
    name: 'navigation-executors',
    covers:
      'The default lens registers goto and named executors with actions: goto reveals the destination smoothly (failed when the page is gone), NextPage/PrevPage/FirstPage/LastPage navigate, and any other name is inert.',
    plugins: [ACTIONS_OFF, ...stage({ scrollBehavior: 'instant' })],
    engine: withActions(FIVE),
    steps: [
      ...boot,
      act('execute', tree(goto({ kind: 'xyz', pageObjectNumber: 3, left: 100, top: 700, zoom: 2 })), USER),
      st('currentPage'),
      act('execute', tree(goto({ kind: 'fit', pageObjectNumber: 5 })), USER),
      act('execute', tree(goto({ kind: 'fitH', pageObjectNumber: 99, top: 10 })), USER),
      act('execute', tree(named('FirstPage')), USER),
      act('execute', tree(named('NextPage')), USER),
      act('execute', tree(named('NextPage')), USER),
      act('execute', tree(named('PrevPage')), USER),
      act('execute', tree(named('LastPage')), USER),
      st('currentPage'),
      act('execute', tree(named('Bogus')), USER),
      close,
    ],
    snapshot: SNAP,
  },
  {
    name: 'executors-navigate-smoothly',
    covers: 'The executors navigate smoothly whatever the scrollBehavior setting: with frames each lands over a tween.',
    plugins: [ACTIONS_OFF, ...stage({ scrollBehavior: 'instant', frames: true })],
    engine: withActions(FIVE),
    steps: [
      ...boot,
      act('execute', tree(goto({ kind: 'xyz', pageObjectNumber: 3, left: 100, top: 700, zoom: 2 })), USER),
      st('cameraInMotion'),
      advance(300),
      act('execute', tree(named('NextPage')), USER),
      st('cameraInMotion'),
      advance(300),
      act('execute', tree(named('LastPage')), USER),
      st('cameraInMotion'),
      advance(300),
    ],
    snapshot: SNAP,
  },
  {
    name: 'page-state-feed',
    covers:
      'The default lens feeds actions a page-state report on placement and whenever the current pon or the visible pon set changes (not on a cause flip alone); a page open trigger it causes runs a navigation executor, whose report is programmatic.',
    plugins: [ACTIONS_OFF, ...stage({ scrollBehavior: 'instant' })],
    engine: withActions([page(1, 0), page(2, 1, 600, 800, { actions: { open: tree(named('NextPage')) } }), page(3, 2), page(4, 3)]),
    steps: [
      start,
      open,
      act('onDiagnostic', fnSpec('diagnostic')),
      vp(1000, 700),
      st('panBy', 0, -5),
      st('goToPage', 1),
      st('currentPage'),
      st('setCamera', { x: 0, y: 0, zoom: 1 }),
      st('setDevicePixelRatio', 2),
      st('goToPage', 3),
      emit(layoutEvent('pages.deleted', [page(1, 0), page(2, 1)])),
      st('currentPage'),
      st('pages'),
    ],
    snapshot: ['stage::a', 'actions::a'],
  },
  {
    name: 'a-thumbnail-lens-stays-out',
    covers: 'A lens with its own id registers no executors and feeds no page state, so actions only ever drive the main lens.',
    plugins: [ACTIONS_OFF, { factory: 'stage', config: { lens: 'stage-thumbs', viewUnitsPerPoint: 1 } }],
    engine: withActions(FIVE),
    steps: [start, open, lensCall('stage-thumbs', 'setViewport', { width: 300, height: 700 }), act('execute', tree(named('NextPage')), USER), act('execute', tree(goto({ kind: 'fit', pageObjectNumber: 2 })), USER)],
    snapshot: ['stage-thumbs::a'],
  },
  {
    name: 'pan-handler',
    covers:
      'The scroll handler pans under the pan tool anywhere (closed-hand cursor while dragging, released on up); under another tool it captures only gap downs, declines page downs, and shows the open hand over gaps on hover.',
    plugins: [interaction, ...stage({ scrollBehavior: 'instant' }), { factory: 'stage-scroll' }],
    engine: engine(),
    steps: [
      ...boot,
      ix('dispatch', sample('move', 5, 5, false)),
      ix('cursor'),
      ix('dispatch', sample('move', 500, 300, true)),
      ix('cursor'),
      ix('dispatch', sample('down', 500, 300, true)),
      ix('dispatch', sample('move', 500, 200, true)),
      ix('dispatch', sample('up', 500, 200, true)),
      st('camera'),
      ix('dispatch', sample('down', 5, 300, false)),
      ix('cursor'),
      ix('dispatch', sample('move', 5, 100, false)),
      ix('dispatch', sample('move', 25, 50, false)),
      ix('dispatch', sample('up', 25, 50, false)),
      ix('cursor'),
      st('camera'),
      st('currentPage'),
      ix('activateTool', 'pan'),
      ix('dispatch', sample('move', 5, 5, false)),
      ix('dispatch', sample('down', 500, 300, true)),
      ix('dispatch', sample('move', 500, 100, true)),
      ix('cursor'),
      ix('dispatch', sample('cancel', 500, 100, true)),
      ix('cursor'),
      st('camera'),
      ix('dispatch', sample('move', 5, 5, false)),
      ix('activateTool', 'pointer'),
      ix('cursor'),
      st('zoomTo', { level: 3 }),
      ix('registerHandler', {
        id: 'low',
        priority: 5,
        enabledFor: { $fn: 'low.enabledFor', returns: true },
        onDown: { $fn: 'low.onDown', returns: true },
        onMove: { $fn: 'low.onMove' },
        onUp: { $fn: 'low.onUp' },
      }),
      ix('dispatch', sample('down', 5, 300, false)),
      ix('dispatch', sample('move', 45, 290, false)),
      ix('dispatch', sample('up', 45, 290, false)),
      st('camera'),
    ],
    snapshot: SNAP,
  },
  {
    name: 'pan-handler-without-fallback',
    covers: 'With panFallback off the handler is enabled only for scroll tools: gap drags under the pointer tool do nothing and hovering claims no cursor.',
    plugins: [interaction, ...stage({ scrollBehavior: 'instant' }), { factory: 'stage-scroll', config: { panFallback: false } }],
    engine: engine(),
    steps: [
      ...boot,
      ix('dispatch', sample('move', 5, 5, false)),
      ix('cursor'),
      ix('dispatch', sample('down', 5, 300, false)),
      ix('dispatch', sample('move', 5, 100, false)),
      ix('dispatch', sample('up', 5, 100, false)),
      st('camera'),
      ix('activateTool', 'pan'),
      ix('dispatch', sample('move', 5, 5, false)),
      ix('dispatch', sample('down', 5, 300, true)),
      ix('dispatch', sample('move', 5, 100, true)),
      ix('dispatch', sample('up', 5, 100, true)),
      st('camera'),
    ],
    snapshot: SNAP,
  },
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/stage-capability.json',
    describes: 'packages/plugin/stage: placement, selectors, settings, navigation, reveal, manipulation and responsive rules, frameless.',
    cases: [...placementCases, ...settingsCases, ...navigationCases, ...manipulationCases, ...mutationEdgeCases, ...responsiveCases, ...edgeCases, ...lensCases],
  },
  {
    kind: 'scenario',
    file: 'plugins/stage-motion.json',
    describes: 'packages/plugin/stage: the camera tween, rest detector, fling and rubber-band physics over the frame seam.',
    cases: motionCases,
  },
  {
    kind: 'scenario',
    file: 'plugins/stage-feeds.json',
    describes: 'packages/plugin/stage: the page-registry refit, the actions executors and page-state feed, and the pan handler on the interaction hub.',
    cases: feedCases,
  },
];
