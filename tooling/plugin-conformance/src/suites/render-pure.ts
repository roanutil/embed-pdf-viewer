/**
 * `packages/plugin/render`'s pure modules: tile-grid math (`tiles.ts`), the
 * option resolver (`paint-plan.ts`), policy ∧ strategy (`strategy.ts`), the
 * ledger reducer and the event→pages map (`reducer.ts`, `effects.ts`), and
 * the engine-core render-policy snaps the plugin's arithmetic rests on
 * (`snapFullPageViewport`, `snapAppearanceScale`, `snapTileScale`,
 * `CONTINUOUS_RENDER_POLICY`). Replayed by
 * `crates/plugins/render/tests/conformance.rs`.
 *
 * Wrappers, where a function takes something a vector cannot carry:
 * `regionCovered` takes the painted set as `"ix,iy"` strings and also returns
 * the coords it asked about, in order, so the early exit is pinned;
 * `resolveStrategy` and `baseAskWidth` take raw plugin options and resolve
 * them first, as the capability does; `reduceRender` reports whether it
 * returned the same state object.
 */
import {
  CONTINUOUS_RENDER_POLICY,
  snapAppearanceScale,
  snapFullPageViewport,
  snapTileScale,
  type EngineRenderPolicy,
} from '@embedpdf/engine-core/runtime';

import { annotatedPons } from '../../../../packages/plugin/render/src/invalidation';
import {
  DEFAULT_TILE_PYRAMID,
  EMPTY_TILE_PLAN,
  resolveRenderOptions,
} from '../../../../packages/plugin/render/src/paint-plan';
import { initialRenderState, reduceRender } from '../../../../packages/plugin/render/src/model';
import { baseAskWidth, resolveStrategy } from '../../../../packages/plugin/render/src/strategy';
import {
  bleedRect,
  inflateRect,
  intersectRects,
  regionCovered,
  snapToPyramid,
  tileGrid,
  tilePaintRect,
  tilesInRect,
  type PageSizePt,
  type TileCoord,
  type TileGrid,
} from '../../../../packages/plugin/render/src/tiles';
import type { RenderConfig as RenderPluginOptions } from '../../../../packages/plugin/render/src/contract';
import type { RenderAction, RenderState } from '../../../../packages/plugin/render/src/model';

import type { FunctionCase, Suite } from '../vectors';

type Fn = (...args: never[]) => unknown;

const c = (fn: string, label: string, ...args: unknown[]): FunctionCase => ({ name: `${fn}/${label}`, fn, args });

const rc = (x: number, y: number, width: number, height: number) => ({ x, y, width, height });
const LETTER: PageSizePt = { width: 612, height: 792 };
const LANDSCAPE: PageSizePt = { width: 792, height: 612 };
const A4: PageSizePt = { width: 595.28, height: 841.89 };
const TINY: PageSizePt = { width: 10, height: 10 };
const STRIP: PageSizePt = { width: 1, height: 2000 };
const CROPPED: PageSizePt = { width: 433.7, height: 288.25 };
const PAGES: [string, PageSizePt][] = [
  ['letter', LETTER],
  ['landscape', LANDSCAPE],
  ['a4', A4],
  ['tiny', TINY],
  ['strip', STRIP],
  ['cropped', CROPPED],
];
/** Levels a viewer asks for: CSS zoom × devicePixelRatio, pyramid rungs, exact levels. */
const SCALES = [0.1, 0.5, 1, 1.25 * 1.5, 2, 3.7, 8, 16, 128, 4896 / 612, 6400 / 612];
const TILE_SIZES = [256, 512, 1000];

// ── tiles.ts ────────────────────────────────────────────────────────────────

const tilesFns: Record<string, Fn> = {
  tileGrid,
  tilesInRect,
  tilePaintRect,
  bleedRect,
  regionCovered: (grid: TileGrid, page: PageSizePt, region: ReturnType<typeof rc>, painted: string[]) => {
    const set = new Set(painted);
    const asked: string[] = [];
    const covered = regionCovered(grid, page, region, (co: TileCoord) => {
      asked.push(`${co.ix},${co.iy}`);
      return set.has(`${co.ix},${co.iy}`);
    });
    return { covered, asked };
  },
  inflateRect,
  snapToPyramid,
  intersectRects,
};

const tiles: FunctionCase[] = [];
for (const [pageName, page] of PAGES) {
  for (const scale of SCALES) {
    for (const size of TILE_SIZES) {
      tiles.push(c('tileGrid', `${pageName}@${scale}/${size}`, page, scale, size));
    }
  }
}
tiles.push(
  c('tileGrid', 'zero-page-still-one-tile', { width: 0, height: 0 }, 2, 512),
  c('tileGrid', 'exact-multiple', { width: 512, height: 1024 }, 1, 512),
  c('tileGrid', 'just-over-a-multiple', { width: 512.0001, height: 1024 }, 1, 512),
  c('tileGrid', 'fractional-tile-size', LETTER, 1, 100.5),
  c('tileGrid', 'nan-scale', LETTER, NaN, 512),
  c('tileGrid', 'infinite-scale', LETTER, Infinity, 512),
  c('tileGrid', 'negative-scale', LETTER, -2, 512),
);

const RECTS: [string, ReturnType<typeof rc>][] = [
  ['inside-one', rc(70, 70, 50, 50)],
  ['corner-of-four', rc(60, 60, 10, 10)],
  ['off-page-negative', rc(-500, -500, 400, 400)],
  ['everything', rc(-10, -10, 10_000, 10_000)],
  ['straddles-left-edge', rc(-20, 100, 40, 40)],
  ['straddles-bottom-edge', rc(100, 780, 40, 40)],
  ['exact-tile', rc(64, 64, 64, 64)],
  ['zero-size-on-a-line', rc(64, 64, 0, 0)],
  ['zero-size-inside', rc(70, 70, 0, 0)],
  ['thin-row', rc(0, 100, 612, 0.5)],
  ['thin-column', rc(300, 0, 0.25, 792)],
  ['far-right', rc(5000, 0, 10, 10)],
  ['visible-quarter', rc(0, 0, 128, 128)],
  ['fractional', rc(33.3, 47.9, 101.1, 99.7)],
  ['negative-size', rc(100, 100, -50, -50)],
];
for (const [scaleName, scale] of [['s1', 1], ['s8', 8], ['s2.5', 2.5], ['s16', 16], ['exact', 6400 / 612]] as const) {
  for (const [pageName, page] of [['letter', LETTER], ['landscape', LANDSCAPE], ['cropped', CROPPED]] as const) {
    const grid = tileGrid(page, scale, 512);
    for (const [rectName, rect] of RECTS) {
      tiles.push(c('tilesInRect', `${pageName}/${scaleName}/${rectName}`, grid, page, rect));
    }
  }
}
tiles.push(c('tilesInRect', 'nan-rect', tileGrid(LETTER, 8, 512), LETTER, rc(NaN, 0, 10, 10)));

for (const [pageName, page] of PAGES) {
  for (const scale of [0.5, 1, 8, 6400 / 612]) {
    const grid = tileGrid(page, scale, 512);
    const coords: TileCoord[] = [
      { ix: 0, iy: 0 },
      { ix: 1, iy: 0 },
      { ix: 0, iy: 1 },
      { ix: grid.cols - 1, iy: 0 },
      { ix: 0, iy: grid.rows - 1 },
      { ix: grid.cols - 1, iy: grid.rows - 1 },
      { ix: grid.cols, iy: grid.rows },
    ];
    for (const co of coords) {
      const label = `${pageName}@${scale}/${co.ix},${co.iy}`;
      tiles.push(c('tilePaintRect', label, grid, page, co));
    }
  }
}


const BLEEDS = [0, 1 / 8, 0.5, 1, 64, 1000];
for (const [rectName, rect] of [
  ['origin-tile', rc(0, 0, 64, 64)],
  ['interior', rc(64, 64, 64, 64)],
  ['right-edge', rc(576, 0, 36, 64)],
  ['bottom-edge', rc(0, 768, 64, 24)],
  ['whole-page', rc(0, 0, 612, 792)],
  ['fractional', rc(33.3, 47.9, 101.1, 99.7)],
] as const) {
  for (const bleed of BLEEDS) tiles.push(c('bleedRect', `${rectName}/${bleed}`, rect, bleed, LETTER));
}
tiles.push(c('bleedRect', 'negative-bleed', rc(64, 64, 64, 64), -2, LETTER));
tiles.push(c('bleedRect', 'outside-page', rc(700, 900, 10, 10), 1, LETTER));

{
  const grid = tileGrid(LETTER, 8, 512);
  const cover: [string, ReturnType<typeof rc>, string[]][] = [
    ['inside-painted-l', rc(0, 0, 60, 60), ['0,0', '1,0', '0,1']],
    ['into-unpainted', rc(0, 0, 120, 120), ['0,0', '1,0', '0,1']],
    ['first-missing-exits-early', rc(0, 0, 200, 64), ['1,0', '2,0']],
    ['empty-region', rc(5, 5, 0, 0), []],
    ['zero-width', rc(5, 5, 0, 10), []],
    ['zero-height', rc(5, 5, 10, 0), []],
    ['negative-size', rc(5, 5, -1, 10), []],
    ['nothing-painted', rc(0, 0, 10, 10), []],
    ['all-painted-square', rc(0, 0, 128, 128), ['0,0', '1,0', '0,1', '1,1']],
    ['off-page-is-vacuous', rc(-100, -100, 50, 50), []],
    ['edge-tiles', rc(590, 770, 30, 30), ['9,12']],
  ];
  for (const [name, region, painted] of cover) tiles.push(c('regionCovered', name, grid, LETTER, region, painted));
  const exact = tileGrid(LETTER, 6400 / 612, 512);
  tiles.push(c('regionCovered', 'exact-level', exact, LETTER, rc(0, 0, 100, 100), ['0,0', '1,0']));
}

const VELOCITIES: [string, unknown][] = [
  ['none', undefined],
  ['still', { dx: 0, dy: 0 }],
  ['right', { dx: 5, dy: 0 }],
  ['left', { dx: -5, dy: 0 }],
  ['down', { dx: 0, dy: 120 }],
  ['up', { dx: 0, dy: -0.001 }],
  ['down-right', { dx: 3, dy: 4 }],
  ['up-left', { dx: -3, dy: -4 }],
  ['nan', { dx: NaN, dy: NaN }],
];
for (const margin of [0, 0.5, 1, 0.25]) {
  for (const [vName, velocity] of VELOCITIES) {
    tiles.push(c('inflateRect', `${margin}/${vName}`, rc(100, 100, 100, 100), margin, velocity));
  }
}
tiles.push(c('inflateRect', 'fractional-rect', rc(33.3, 47.9, 101.1, 99.7), 0.5, { dx: 1, dy: -1 }));
tiles.push(c('inflateRect', 'negative-margin', rc(100, 100, 100, 100), -0.25, { dx: 1, dy: 1 }));

tiles.push(
  c('snapToPyramid', 'snaps-up', [1, 2, 4, 8], 3),
  c('snapToPyramid', 'exact-rung', [1, 2, 4, 8], 8),
  c('snapToPyramid', 'caps-at-top', [1, 2, 4, 8], 50),
  c('snapToPyramid', 'unsorted', [8, 1, 4, 2], 3),
  c('snapToPyramid', 'below-bottom', [1, 2, 4, 8], 0.1),
  c('snapToPyramid', 'single', [3], 1),
  c('snapToPyramid', 'single-above', [3], 5),
  c('snapToPyramid', 'fractional', [0.5, 1.5, 4.25], 1.5000001),
  c('snapToPyramid', 'duplicates', [2, 2, 1], 1.5),
  c('snapToPyramid', 'nan-needed', [1, 2], NaN),
  c('snapToPyramid', 'default-pyramid', [...DEFAULT_TILE_PYRAMID], 9),
);

tiles.push(
  c('intersectRects', 'overlap', rc(0, 0, 10, 10), rc(5, 5, 10, 10)),
  c('intersectRects', 'disjoint', rc(0, 0, 10, 10), rc(20, 20, 5, 5)),
  c('intersectRects', 'tile-and-visible', rc(64, 64, 64, 64), rc(0, 0, 100, 100)),
);

// ── paint-plan.ts, strategy.ts, and the engine-core snaps ───────────────────

const LATTICE: EngineRenderPolicy = {
  kind: 'lattice',
  fullPage: { widths: [320, 640, 1280, 2560] },
  appearances: { scales: [1, 2, 4] },
  formats: ['webp'],
  background: 'white',
  enforced: false,
};
const LATTICE_UNSORTED: EngineRenderPolicy = { ...LATTICE, fullPage: { widths: [2560, 320, 1280, 640] } };
const LATTICE_TILES: EngineRenderPolicy = {
  ...LATTICE,
  tiles: { tileSizes: [256, 1024], scales: [16, 1, 4, 2, 8] },
  formats: ['webp', 'png'],
};
const LATTICE_TILES_512: EngineRenderPolicy = { ...LATTICE_TILES, tiles: { tileSizes: [1024, 512], scales: [1, 2] } };
const LATTICE_NO_TILE_SIZES: EngineRenderPolicy = { ...LATTICE, tiles: { tileSizes: [], scales: [3, 6] } };
const CONTINUOUS = CONTINUOUS_RENDER_POLICY;
const POLICIES: [string, EngineRenderPolicy][] = [
  ['continuous', CONTINUOUS],
  ['lattice', LATTICE],
  ['lattice-unsorted', LATTICE_UNSORTED],
  ['lattice-tiles', LATTICE_TILES],
  ['lattice-tiles-512', LATTICE_TILES_512],
  ['lattice-no-tile-sizes', LATTICE_NO_TILE_SIZES],
];

const OPTIONS: [string, RenderPluginOptions][] = [
  ['defaults', {}],
  ['tiles-false', { tiles: false }],
  ['explicit-budget', { fullPage: { maxWidth: 700 } }],
  ['tiny-budget', { fullPage: { maxWidth: 100 } }],
  ['ladder', { fullPage: { quantize: [1280, 320, 640], maxWidth: 1280 } }],
  ['ladder-over-budget', { fullPage: { quantize: [800, 1600] } }],
  ['tile-ladder', { tiles: { quantize: [4, 1, 2] } }],
  ['tile-size-256', { tiles: { size: 256 } }],
  ['tile-size-1024', { tiles: { size: 1024, quantize: 'exact' } }],
  ['engage-at', { tiles: { engageAt: 2 } }],
  ['engage-at-zero', { tiles: { engageAt: 0 } }],
  ['bmp', { format: 'bmp', quality: 0.9 }],
  ['png', { format: 'png' }],
  ['webp', { format: 'webp', quality: 0.5 }],
  [
    'everything',
    {
      fullPage: { maxWidth: 2000, quantize: 'exact' },
      tiles: {
        size: 384,
        quantize: [2, 8],
        maxScale: 64,
        bleed: 0,
        engageAt: 1.5,
        prefetch: { margin: 0.25, velocityBias: false },
        settleMs: 0,
        fadeMs: 120,
      },
      format: 'webp',
      quality: 0.8,
      debug: true,
    },
  ],
  ['prefetch-margin-only', { tiles: { prefetch: { margin: 1 } } }],
  ['prefetch-bias-only', { tiles: { prefetch: { velocityBias: false } } }],
  ['empty-tiles-object', { tiles: {} }],
  ['empty-full-page', { fullPage: {} }],
  ['debug-false', { debug: false }],
];

const strategyFns: Record<string, Fn> = {
  resolveRenderOptions,
  resolveStrategy: (policy: EngineRenderPolicy, options: RenderPluginOptions) =>
    resolveStrategy(policy, resolveRenderOptions(options)),
  baseAskWidth: (policy: EngineRenderPolicy, options: RenderPluginOptions, demandWidth: number) =>
    baseAskWidth(resolveStrategy(policy, resolveRenderOptions(options)), demandWidth),
  DEFAULT_TILE_PYRAMID: () => DEFAULT_TILE_PYRAMID,
  EMPTY_TILE_PLAN: () => EMPTY_TILE_PLAN,
};

const strategy: FunctionCase[] = [c('DEFAULT_TILE_PYRAMID', 'the-x2-pyramid'), c('EMPTY_TILE_PLAN', 'not-engaged')];
for (const [name, options] of OPTIONS) strategy.push(c('resolveRenderOptions', name, options));
for (const [pName, policy] of POLICIES) {
  for (const [oName, options] of OPTIONS) strategy.push(c('resolveStrategy', `${pName}/${oName}`, policy, options));
}
const DEMANDS = [0, 0.4, 1, 1.5, 99.5, 100.5, 306, 319.5, 320, 320.4, 612, 639.5, 640, 640.5, 700, 1224, 2560, 4896, 1e6, -5, NaN];
for (const [pName, policy] of POLICIES.slice(0, 3)) {
  for (const [oName, options] of [OPTIONS[0], OPTIONS[2], OPTIONS[3], OPTIONS[4], OPTIONS[5]]) {
    for (const demand of DEMANDS) {
      strategy.push(c('baseAskWidth', `${pName}/${oName}/${demand}`, policy, options, demand));
    }
  }
}

const policyFns: Record<string, Fn> = {
  CONTINUOUS_RENDER_POLICY: () => CONTINUOUS_RENDER_POLICY,
  snapFullPageViewport,
  snapAppearanceScale,
  snapTileScale,
};

const policy: FunctionCase[] = [c('CONTINUOUS_RENDER_POLICY', 'kind')];
const VIEWPORTS: [string, unknown, unknown][] = [
  ['width-300', { kind: 'width', width: 300 }, undefined],
  ['width-exact-rung', { kind: 'width', width: 640 }, undefined],
  ['width-720', { kind: 'width', width: 720 }, undefined],
  ['width-huge', { kind: 'width', width: 100_000 }, undefined],
  ['width-zero', { kind: 'width', width: 0 }, undefined],
  ['width-nan', { kind: 'width', width: NaN }, undefined],
  ['scale-1', { kind: 'scale', scale: 1 }, { pageWidth: 612 }],
  ['scale-2', { kind: 'scale', scale: 2 }, { pageWidth: 612 }],
  ['scale-poster', { kind: 'scale', scale: 1 }, { pageWidth: 1_000_000 }],
  ['scale-default-1', { kind: 'scale' }, { pageWidth: 612 }],
  ['scale-no-page-width', { kind: 'scale', scale: 1 }, undefined],
  ['scale-empty-opts', { kind: 'scale', scale: 1 }, {}],
  ['scale-zero-page-width', { kind: 'scale', scale: 1 }, { pageWidth: 0 }],
  ['scale-negative-page-width', { kind: 'scale', scale: 1 }, { pageWidth: -3 }],
  ['scale-half', { kind: 'scale', scale: 0.5 }, { pageWidth: 612 }],
];
for (const [pName, p] of [
  ['continuous', CONTINUOUS],
  ['lattice', LATTICE],
  ['lattice-unsorted', LATTICE_UNSORTED],
] as const) {
  for (const [vName, viewport, opts] of VIEWPORTS) {
    policy.push(c('snapFullPageViewport', `${pName}/${vName}`, p, viewport, opts));
  }
}
policy.push(c('snapFullPageViewport', 'empty-ladder', { ...LATTICE, fullPage: { widths: [] } }, { kind: 'width', width: 5 }));
policy.push(c('snapFullPageViewport', 'single-rung', { ...LATTICE, fullPage: { widths: [900] } }, { kind: 'width', width: 5 }));
const SCALE_ASKS = [0, 0.5, 1, 1.5, 2, 3.99, 4, 4.01, 100, -1, NaN];
for (const [pName, p] of [
  ['continuous', CONTINUOUS],
  ['lattice', LATTICE],
  ['lattice-tiles', LATTICE_TILES],
  ['no-appearances', { ...LATTICE, appearances: undefined }],
  ['empty-appearances', { ...LATTICE, appearances: { scales: [] } }],
  ['empty-tile-scales', { ...LATTICE, tiles: { tileSizes: [512], scales: [] } }],
] as const) {
  for (const s of SCALE_ASKS) {
    policy.push(c('snapAppearanceScale', `${pName}/${s}`, p, s));
    policy.push(c('snapTileScale', `${pName}/${s}`, p, s));
  }
}

// ── reducer.ts and effects.ts ───────────────────────────────────────────────

const ledgerFns: Record<string, Fn> = {
  initialRenderState,
  reduceRender: (state: RenderState, action: RenderAction) => {
    const next = reduceRender(state, action);
    return { changed: next !== state, state: next };
  },
  annotatedPons: (event: never, pons: number[]) => annotatedPons(event, () => pons),
};

const seeded: RenderState = {
  contentEpochs: { 11: 2 },
  annotatedEpochs: { 11: 1, 22: 4 },
  paintVersions: { 33: 7 },
};
const ledger: FunctionCase[] = [
  c('initialRenderState', 'empty-ledgers'),
  c('reduceRender', 'annotations-bump', initialRenderState(), { type: 'invalidate', scope: 'annotations', pages: [11] }),
  c('reduceRender', 'annotations-bump-seeded', seeded, { type: 'invalidate', scope: 'annotations', pages: [11, 22, 44] }),
  c('reduceRender', 'content-bump', seeded, { type: 'invalidate', scope: 'content', pages: [11, 33] }),
  c('reduceRender', 'duplicate-pons-bump-twice', seeded, { type: 'invalidate', scope: 'content', pages: [11, 11] }),
  c('reduceRender', 'empty-pons-is-a-no-op', seeded, { type: 'invalidate', scope: 'content', pages: [] }),
  c('reduceRender', 'empty-pons-annotations-no-op', seeded, { type: 'invalidate', scope: 'annotations', pages: [] }),
  c('reduceRender', 'unknown-action-no-op', seeded, { type: 'OTHER' }),
  c('reduceRender', 'paint-advanced', seeded, { type: 'paintAdvanced', page: 33 }),
  c('reduceRender', 'paint-advanced-new-pon', seeded, { type: 'paintAdvanced', page: 5 }),
];

const pr = (pon: number) => ({ kind: 'objectNumber', pageObjectNumber: pon });
const widget = (pon: number) => ({ ref: { kind: 'objectNumber', page: pr(pon), annotObjectNumber: 5 }, annotObjectNumber: 5, page: pr(pon) });
const ALL = [11, 22, 33];
for (const type of ['annotation.created', 'annotation.updated', 'annotation.deleted', 'annotation.moved']) {
  ledger.push(c('annotatedPons', type, { type, page: pr(22) }, ALL));
}
for (const type of ['form.valueChanged', 'form.effectsApplied']) {
  ledger.push(c('annotatedPons', type, { type, changedWidgets: [widget(11), widget(33), widget(11)] }, ALL));
  ledger.push(c('annotatedPons', `${type}/none`, { type, changedWidgets: [] }, ALL));
}
ledger.push(c('annotatedPons', 'form.fieldDeleted', { type: 'form.fieldDeleted', removedWidgets: [widget(22)] }, ALL));
for (const type of ['form.fieldCreated', 'form.fieldUpdated', 'form.widgetAttached', 'form.widgetDetached']) {
  ledger.push(c('annotatedPons', type, { type, field: { widgets: [widget(11), widget(22)] } }, ALL));
}
for (const type of ['form.imported', 'form.repaired']) {
  ledger.push(c('annotatedPons', type, { type }, ALL));
  ledger.push(c('annotatedPons', `${type}/no-pages`, { type }, []));
}
for (const type of ['pages.rotated', 'pages.moved', 'pages.deleted', 'metadata.updated', 'redaction.applied', 'unknown']) {
  ledger.push(c('annotatedPons', type, { type, page: pr(11) }, ALL));
}

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'plugins/render-tiles.json',
    describes: 'packages/plugin/render/src/tiles.ts: the aligned tile grid, paint and engine rects, bleed, coverage, the prefetch ring.',
    functions: tilesFns,
    cases: tiles,
  },
  {
    kind: 'function',
    file: 'plugins/render-strategy.json',
    describes: 'packages/plugin/render/src/{paint-plan,strategy}.ts: option resolution and policy ∧ strategy.',
    functions: strategyFns,
    cases: strategy,
  },
  {
    kind: 'function',
    file: 'plugins/render-policy.json',
    describes: 'engine-core render-policy snaps (DocumentRenderService.ts), ported into epdf-plugin-render.',
    functions: policyFns,
    cases: policy,
  },
  {
    kind: 'function',
    file: 'plugins/render-ledger.json',
    describes: 'packages/plugin/render/src/{reducer,effects}.ts: the epoch ledger and the event→pages map.',
    functions: ledgerFns,
    cases: ledger,
  },
];
