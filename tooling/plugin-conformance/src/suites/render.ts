/**
 * `packages/plugin/render`: the capability, the event effect and the tile
 * manager behind `tilesFor`, driven through the kernel and the fake engine.
 * Replayed by `crates/plugins/render/tests/conformance.rs`.
 *
 * Three harness shims live here, and `crates/plugins/render/src/harness.rs`
 * mirrors each:
 *
 *   - `renderPage`'s `signal`. A vector cannot carry an `AbortSignal`, so the
 *     `render` factory wraps the capability: an option `signal:
 *     {"$abortSignal": name}` becomes the signal of a controller named `name`,
 *     and the `signals` test plugin's `abort(name)` aborts it (no reason, so
 *     the reason is the platform's `AbortError` DOMException).
 *   - `console.warn` and `console.debug`. The plugin's one-shot warnings and
 *     its `debug: true` diagnostics are behaviour a developer reads, so while
 *     a render factory is current they log as callbacks named `console.warn`
 *     and `console.debug`.
 *   - Every `page.render.image` call is deferred under its own label, `r0`,
 *     `r1`, ... in call order, so a step settles exactly the call it means.
 */
import { createCapabilityToken, type AnyPlugin } from '@embedpdf/core';

import { RenderToken, renderPlugin } from '../../../../packages/plugin/render/src/index';
import type { RenderCapability, RenderPluginOptions } from '../../../../packages/plugin/render/src/types';

import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import { registerPlugin, type PluginEnv, type PluginSpec, type ScenarioCase, type Step } from '../scenario';
import type { Suite } from '../vectors';

// ── harness shims ─────────────────────────────────────────────────────────────

const controllers = new Map<string, AbortController>();
const controllerNamed = (name: string): AbortController => {
  let controller = controllers.get(name);
  if (!controller) {
    controller = new AbortController();
    controllers.set(name, controller);
  }
  return controller;
};

let currentLog: PluginEnv['log'] | null = null;
const originalConsole = { warn: console.warn, debug: console.debug };
console.warn = (...args: unknown[]) => (currentLog ? currentLog('console.warn', args) : originalConsole.warn(...args));
console.debug = (...args: unknown[]) =>
  currentLog ? currentLog('console.debug', args) : originalConsole.debug(...args);

const withSignals = (capability: RenderCapability): RenderCapability => ({
  ...capability,
  renderPage: (pon, options) => {
    const spec = (options as { signal?: { $abortSignal?: string } }).signal;
    const signal = spec?.$abortSignal !== undefined ? controllerNamed(spec.$abortSignal).signal : undefined;
    return capability.renderPage(pon, { ...options, signal });
  },
});

registerPlugin('render', {
  factory: (config, env) => {
    controllers.clear();
    currentLog = env.log;
    const plugin = renderPlugin((config ?? {}) as RenderPluginOptions) as AnyPlugin;
    return {
      ...plugin,
      capability: (ctx) => withSignals(plugin.capability!(ctx) as RenderCapability),
    } as AnyPlugin;
  },
  tokens: [RenderToken],
});

const SignalsToken = createCapabilityToken<{ abort(name: string): void }>('render-test/signals');
registerPlugin('signals', {
  factory: () =>
    ({
      id: 'render-test/signals',
      token: SignalsToken,
      capability: () => ({ abort: (name: string) => controllerNamed(name).abort() }),
    }) as AnyPlugin,
  tokens: [SignalsToken],
});

// ── fixtures ──────────────────────────────────────────────────────────────────

const layout = (pon: number, index: number, width: number, height: number) => {
  const box = { left: 0, bottom: 0, right: width, top: height };
  return { index, pageObjectNumber: pon, label: null, size: { width, height }, rotation: 0, userUnit: 1, boxes: { media: box, crop: box } };
};
const PAGES = [layout(11, 0, 612, 792), layout(22, 1, 792, 612), layout(33, 2, 300, 300)];

const LATTICE = {
  kind: 'lattice',
  fullPage: { widths: [320, 640, 1280, 2560] },
  appearances: { scales: [1, 2, 4] },
  formats: ['webp'],
  background: 'white',
  enforced: false,
};
const LATTICE_TILES = {
  ...LATTICE,
  tiles: { tileSizes: [256, 1024], scales: [16, 1, 4, 2, 8] },
  formats: ['webp', 'png'],
};

/** Every image call deferred under its own label, in call order. */
const imageCalls = (n = 160): ResponseSpec[] =>
  Array.from({ length: n }, (_, i) => ({ path: 'page.render.image', times: 1, defer: `r${i}` }));

function doc(opts: { policy?: unknown; allows?: readonly string[] | '*'; responses?: ResponseSpec[]; pages?: unknown[] } = {}): FakeDocumentSpec {
  const responses: ResponseSpec[] = [];
  if (opts.policy !== undefined) responses.push({ path: 'render.policy', value: opts.policy });
  responses.push(...(opts.responses ?? []), ...imageCalls());
  return {
    services: opts.policy !== undefined ? ['render'] : [],
    security: { allows: opts.allows ?? ['doc.render'] },
    pages: opts.pages ?? PAGES,
    responses,
  };
}

const handle = (width: number, height: number) => ({ width, height });
const HANDLE = handle(512, 512);
const BYTES_HANDLE = { source: { kind: 'bytes', bytes: { $bytes: 'AQID' } } };

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const open = (id = 'a'): Step => ({ op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id, bytes: { $bytes: '' } }] });
const close = (id = 'a'): Step => ({ op: 'call', target: 'documents', method: 'close', args: [id] });
const cap = (method: string, ...args: unknown[]): Step => ({ op: 'call', target: { token: 'render', doc: 'a' }, method, args });
const settle = (label: string, value: unknown = HANDLE): Step => ({ op: 'settle', label, value });
const fail = (label: string, message = 'boom'): Step => ({ op: 'settle', label, error: { name: 'EngineError', message, code: 'Unknown' } });
const settleRange = (from: number, to: number, value: unknown = HANDLE): Step[] =>
  Array.from({ length: to - from + 1 }, (_, i) => settle(`r${from + i}`, value));
const advance = (ms: number): Step => ({ op: 'advance', ms });
const abortSignal = (name: string): Step => ({ op: 'call', target: { token: 'render-test/signals' }, method: 'abort', args: [name] });
const emit = (event: unknown): Step => ({ op: 'emit', doc: 'a', event });
const signal = (name: string) => ({ $abortSignal: name });

const render = (config?: RenderPluginOptions): PluginSpec[] => [config === undefined ? { factory: 'render' } : { factory: 'render', config }];
const withSignalsPlugin = (config?: RenderPluginOptions): PluginSpec[] => [{ factory: 'signals' }, ...render(config)];

const SNAPSHOT = ['render::a'];

/** Tile keys as the manager mints them. */
const tk = (pon: number, width: number, ix: number, iy: number, o: { a?: 0 | 1; e?: number; g?: string } = {}) =>
  `t:${pon}|w${width}|${ix},${iy}|a${o.a ?? 1}|e${o.e ?? 0}|g${o.g ?? '512.1'}`;

/** Ref bookkeeping for `tilesFor` handles: each print registers four refs. */
function views() {
  let next = 0;
  const base: Record<string, number> = {};
  return {
    tilesFor(view: string): Step {
      base[view] = next;
      next += 4;
      return cap('tilesFor', view);
    },
    plan(view: string, pon: number, demand: unknown, opts?: unknown): Step {
      return { op: 'call', target: { ref: base[view] }, args: opts === undefined ? [pon, demand] : [pon, demand, opts] };
    },
    painted(view: string, pon: number, key: string): Step {
      return { op: 'call', target: { ref: base[view] + 1 }, args: [pon, key] };
    },
    unpainted(view: string, pon: number, key: string): Step {
      return { op: 'call', target: { ref: base[view] + 2 }, args: [pon, key] };
    },
    release(view: string, pon: number): Step {
      return { op: 'call', target: { ref: base[view] + 3 }, args: [pon] };
    },
  };
}

const visible = (x: number, y: number, width: number, height: number) => ({ x, y, width, height });
/** A 612pt page at device width 4896 is scale 8 exactly: 64pt tiles. */
const DEEP = { desiredDeviceWidth: 4896, visibleRect: visible(0, 0, 128, 128) };
const DEEPER = { desiredDeviceWidth: 9792, visibleRect: visible(0, 0, 64, 64) };
const demand = (w: number, rect = visible(0, 0, 128, 128)) => ({ desiredDeviceWidth: w, visibleRect: rect });

// ── the capability ────────────────────────────────────────────────────────────

const capabilityCases: ScenarioCase[] = [
  {
    name: 'keys-continuous-exact',
    covers:
      'Under continuous the base renders the exact demand up to the 640 budget: keys, conformed viewports, epochs, policy and paint settings, with the page width converting scale to device px.',
    plugins: render(),
    engine: { documents: { a: doc() } },
    snapshot: SNAPSHOT,
    steps: [
      start,
      open(),
      cap('canRender'),
      cap('renderPolicy'),
      cap('paintSettings'),
      cap('renderSourceKey', 11, { scale: 0.5 }),
      cap('renderSourceKey', 11, { scale: 0.5, includeAnnotations: false }),
      cap('renderSourceKey', 11, { scale: 0.5, includeAnnotations: null }),
      cap('renderSourceKey', 11, { scale: 1.53 }),
      cap('renderSourceKey', 11, { scale: 8 }),
      cap('renderSourceKey', 22, { scale: 1 }),
      cap('renderSourceKey', 33, { scale: 1 }),
      cap('renderSourceKey', 33, { scale: 0.001 }),
      cap('renderSourceKey', 33, { scale: 0.5025 }),
      cap('conformViewport', 11, 0.5),
      cap('conformViewport', 22, 2.5),
      cap('conformViewport', 33, 0),
      cap('renderEpoch', 11),
      cap('renderEpoch', 11, false),
    ],
  },
  {
    name: 'keys-ladder-quantize',
    covers: 'A continuous policy with a client width ladder snaps UP the sorted rungs and caps at the top rung.',
    plugins: render({ fullPage: { quantize: [1280, 320, 640], maxWidth: 1280 } }),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [
      start,
      open(),
      cap('renderSourceKey', 11, { scale: 0.1 }),
      cap('renderSourceKey', 11, { scale: 0.5 }),
      cap('renderSourceKey', 11, { scale: 1 }),
      cap('renderSourceKey', 11, { scale: 8 }),
      cap('conformViewport', 22, 1),
      cap('paintSettings'),
    ],
  },
  {
    name: 'ladder-over-budget-keeps-the-smallest-rung',
    covers: 'A client ladder whose every rung exceeds maxWidth keeps its smallest rung rather than none.',
    plugins: render({ fullPage: { quantize: [1600, 800], maxWidth: 500 } }),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [start, open(), cap('renderSourceKey', 11, { scale: 0.1 }), cap('renderSourceKey', 11, { scale: 8 })],
  },
  {
    name: 'lattice-default-budget',
    covers:
      'An advertised lattice (read from render.policy at open) is the deployment’s ladder: the default budget never filters it, zoom inside a rung keeps the key, crossing a rung changes it.',
    plugins: render(),
    engine: { documents: { a: doc({ policy: LATTICE }) } },
    snapshot: 'none',
    steps: [
      start,
      open(),
      cap('renderPolicy'),
      cap('renderSourceKey', 11, { scale: 1 }),
      cap('renderSourceKey', 11, { scale: 1.2 }),
      cap('renderSourceKey', 11, { scale: 1.5 }),
      cap('renderSourceKey', 11, { scale: 2.2 }),
      cap('renderSourceKey', 11, { scale: 8 }),
      cap('renderSourceKey', 11, { scale: 0.1 }),
      cap('conformViewport', 11, 2),
      cap('conformViewport', 33, 1),
    ],
  },
  {
    name: 'lattice-explicit-budget-filters',
    covers: 'An explicit maxWidth filters the advertised ladder (the mobile-memory knob); rungs above it are never asked for.',
    plugins: render({ fullPage: { maxWidth: 700 } }),
    engine: { documents: { a: doc({ policy: LATTICE }) } },
    snapshot: 'none',
    steps: [start, open(), cap('conformViewport', 11, 8), cap('conformViewport', 11, 0.5), cap('renderSourceKey', 11, { scale: 1 })],
  },
  {
    name: 'format-conforms-and-warns-once',
    covers:
      'A strategy format the lattice does not list conforms to the deployment’s first format, joins the key, reaches the engine, and warns once.',
    plugins: render({ format: 'bmp', quality: 0.7 }),
    engine: { documents: { a: doc({ policy: LATTICE }) } },
    snapshot: 'none',
    steps: [
      start,
      open(),
      cap('renderSourceKey', 11, { scale: 1 }),
      cap('renderSourceKey', 11, { scale: 2 }),
      cap('renderPage', 11, { scale: 1 }),
      settle('r0'),
    ],
  },
  {
    name: 'format-rides-under-continuous',
    covers: 'Under continuous the strategy format and quality apply as written, and the format is part of the raster identity.',
    plugins: render({ format: 'bmp', quality: 0.9 }),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [start, open(), cap('renderSourceKey', 11, { scale: 0.5 }), cap('renderPage', 11, { scale: 0.5, includeAnnotations: false })],
    allowUnmatched: false,
  },
  {
    name: 'format-listed-by-the-lattice',
    covers: 'A format the lattice lists is kept, with no warning.',
    plugins: render({ format: 'png' }),
    engine: { documents: { a: doc({ policy: LATTICE_TILES }) } },
    snapshot: 'none',
    steps: [start, open(), cap('renderSourceKey', 11, { scale: 1 }), cap('renderPage', 11, { scale: 1 })],
  },
  {
    name: 'render-page-conformed-and-collapsed',
    covers:
      'renderPage sends the conformed viewport; same-key asks collapse onto one engine call (singleflight); a re-ask after resolution is served from the store; another annotations flag is another raster.',
    plugins: render(),
    engine: { documents: { a: doc({ policy: LATTICE }) } },
    snapshot: SNAPSHOT,
    steps: [
      start,
      open(),
      cap('renderPage', 11, { scale: 1.2 }),
      cap('renderPage', 11, { scale: 1.5 }),
      settle('r0', { width: 1280, height: 1656 }),
      cap('renderPage', 11, { scale: 1.3 }),
      cap('renderPage', 11, { scale: 1.3, includeAnnotations: false }),
      settle('r1', BYTES_HANDLE),
      cap('renderPage', 22, { scale: 0.2 }),
      cap('renderPage', 22, { scale: 0.2 }),
      settle('r2', {}),
      cap('renderPage', 22, { scale: 0.2 }),
    ],
  },
  {
    name: 'render-page-failure-is-not-sticky',
    covers: 'An engine failure rejects every attached consumer with the engine’s error and drops the entry, so the next ask fetches again.',
    plugins: render(),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [
      start,
      open(),
      cap('renderPage', 11, { scale: 0.5 }),
      cap('renderPage', 11, { scale: 0.5 }),
      fail('r0', 'render failed'),
      cap('renderPage', 11, { scale: 0.5 }),
      settle('r1'),
    ],
  },
  {
    name: 'signal-one-consumer-aborts',
    covers:
      'A consumer abandoning a shared in-flight fetch rejects with its signal’s reason and detaches; the engine call stays alive for the signal-less consumer, who resolves.',
    plugins: withSignalsPlugin(),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [
      start,
      open(),
      cap('renderPage', 11, { scale: 0.5, signal: signal('s1') }),
      cap('renderPage', 11, { scale: 0.5 }),
      abortSignal('s1'),
      settle('r0'),
      abortSignal('s1'),
    ],
  },
  {
    name: 'signal-last-consumer-aborts-the-engine-call',
    covers:
      'The last live consumer leaving an unresolved fetch aborts the engine call (the store’s reason reaches the engine) and drops the entry; a late answer is ignored and the next ask fetches fresh.',
    plugins: withSignalsPlugin(),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [
      start,
      open(),
      cap('renderPage', 11, { scale: 0.5, signal: signal('s1') }),
      cap('renderPage', 11, { scale: 0.5, signal: signal('s2') }),
      abortSignal('s1'),
      abortSignal('s2'),
      settle('r0'),
      cap('renderPage', 11, { scale: 0.5 }),
      settle('r1'),
    ],
  },
  {
    name: 'signal-already-aborted',
    covers:
      'A pre-aborted signal rejects at once, but the fetch it created is already running and unowned: a later signal-less ask joins it, and it lands in the store.',
    plugins: withSignalsPlugin(),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [
      start,
      open(),
      abortSignal('s1'),
      cap('renderPage', 11, { scale: 0.5, signal: signal('s1') }),
      cap('renderPage', 11, { scale: 0.5 }),
      settle('r0'),
      cap('renderPage', 11, { scale: 0.5, signal: signal('s1') }),
    ],
  },
  {
    name: 'signal-after-resolution-is-a-no-op',
    covers: 'An abort after the fetch resolved changes nothing: the consumer already resolved, and the raster stays cached.',
    plugins: withSignalsPlugin(),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [
      start,
      open(),
      cap('renderPage', 11, { scale: 0.5, signal: signal('s1') }),
      settle('r0'),
      abortSignal('s1'),
      cap('renderPage', 11, { scale: 0.5, signal: signal('s2') }),
      abortSignal('s2'),
      cap('renderPage', 11, { scale: 0.5 }),
    ],
  },
  {
    name: 'signal-consumer-sees-the-engine-failure',
    covers: 'A signal-bearing consumer attached to a fetch that fails rejects with the engine error, and its later abort does nothing.',
    plugins: withSignalsPlugin(),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [start, open(), cap('renderPage', 11, { scale: 0.5, signal: signal('s1') }), fail('r0'), abortSignal('s1')],
  },
  {
    name: 'eviction-by-cost',
    covers:
      'The store budget (128 MiB) is cost, not count: a URL-style handle costs width × height, bytes their length, anything else 1. Over budget, the oldest consumer-free resolved entry goes first; a just-resolved entry whose consumers have not yet run is spared.',
    plugins: render(),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [
      start,
      open(),
      cap('renderPage', 11, { scale: 0.5 }),
      settle('r0', handle(10000, 10000)),
      cap('renderPage', 11, { scale: 0.6 }),
      settle('r1', handle(10000, 10000)),
      cap('renderPage', 11, { scale: 0.5 }),
      cap('renderPage', 11, { scale: 0.6 }),
      settle('r2', handle(10000, 10000)),
      cap('renderPage', 11, { scale: 0.7 }),
      settle('r3', handle(20000, 20000)),
      cap('renderPage', 11, { scale: 0.7 }),
      cap('renderPage', 11, { scale: 0.5 }),
    ],
  },
  {
    name: 'eviction-stops-once-under-budget',
    covers: 'Eviction walks candidates oldest first and stops as soon as the ledger is back under budget, keeping the younger ones.',
    plugins: render(),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [
      start,
      open(),
      cap('renderPage', 11, { scale: 0.5 }),
      settle('r0', handle(5000, 10000)),
      cap('renderPage', 11, { scale: 0.6 }),
      settle('r1', handle(5000, 10000)),
      cap('renderPage', 11, { scale: 0.7 }),
      settle('r2', handle(5000, 10000)),
      cap('renderPage', 11, { scale: 0.6 }),
      cap('renderPage', 11, { scale: 0.5 }),
    ],
  },
  {
    name: 'eviction-spares-in-flight-entries',
    covers: 'Entries still in flight are never evicted, and a zero-cost handle never pushes anything out.',
    plugins: render(),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [
      start,
      open(),
      cap('renderPage', 11, { scale: 0.5 }),
      cap('renderPage', 11, { scale: 0.6 }),
      settle('r1', handle(30000, 30000)),
      cap('renderPage', 11, { scale: 0.8 }),
      settle('r0', { width: 0, height: 5 }),
      cap('renderPage', 11, { scale: 0.5 }),
      cap('renderPage', 11, { scale: 0.6 }),
      settle('r2', handle(1, 1)),
      cap('renderPage', 11, { scale: 0.6 }),
    ],
  },
  {
    name: 'permission-denied',
    covers:
      'The twin: canRender mirrors doc.render, and a denied renderPage rejects locally with PermissionDenied (context render.renderPage) before conforming or calling the engine; keys stay computable.',
    plugins: render(),
    engine: { documents: { a: doc({ allows: [] }) } },
    snapshot: 'none',
    steps: [
      start,
      open(),
      cap('canRender'),
      cap('renderPage', 11, { scale: 1 }),
      cap('renderPage', 99, { scale: 1 }),
      cap('renderSourceKey', 11, { scale: 1 }),
    ],
  },
  {
    name: 'wildcard-allows-render',
    covers: 'A wildcard grant renders.',
    plugins: render(),
    engine: { documents: { a: doc({ allows: '*' }) } },
    snapshot: 'none',
    steps: [start, open(), cap('canRender'), cap('renderPage', 33, { scale: 1 })],
  },
  {
    name: 'unknown-page-throws',
    covers: 'A pon the registry does not know throws synchronously from renderPage, renderSourceKey and conformViewport (it is a programmer error, not a rejection).',
    plugins: render(),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [start, open(), cap('renderPage', 99, { scale: 1 }), cap('renderSourceKey', 99, { scale: 1 }), cap('conformViewport', 99, 1)],
  },
  {
    name: 'engine-refuses-a-registered-page',
    covers:
      'A page the registry lists but the handle refuses: doc.page throws inside the store’s fetch, so renderPage throws the EngineError synchronously and nothing is cached.',
    plugins: render(),
    engine: {
      documents: {
        a: doc({ responses: [{ path: 'pages.list', value: { pageCount: 4, pages: [...PAGES, layout(44, 3, 100, 100)] } }] }),
      },
    },
    snapshot: 'none',
    steps: [start, open(), cap('renderSourceKey', 44, { scale: 1 }), cap('renderPage', 44, { scale: 1 }), cap('renderPage', 44, { scale: 1 })],
  },
  {
    name: 'epochs-and-invalidate',
    covers:
      'invalidate: content reaches both products, annotations only the annotated one; no pons means every page; an empty list dispatches nothing; epochs mint new keys.',
    plugins: render(),
    engine: { documents: { a: doc() } },
    snapshot: SNAPSHOT,
    steps: [
      start,
      open(),
      cap('renderSourceKey', 22, { scale: 0.5 }),
      cap('invalidate', { pons: [22], scope: 'content' }),
      cap('renderEpoch', 22, false),
      cap('renderEpoch', 22, true),
      cap('invalidate', { pons: [22], scope: 'annotations' }),
      cap('renderEpoch', 22, false),
      cap('renderEpoch', 22),
      cap('renderSourceKey', 22, { scale: 0.5 }),
      cap('renderSourceKey', 22, { scale: 0.5, includeAnnotations: false }),
      cap('invalidate'),
      cap('invalidate', {}),
      cap('invalidate', { scope: 'annotations' }),
      cap('invalidate', { pons: [] }),
      cap('invalidate', { pons: [11, 11, 99] }),
      cap('renderEpoch', 11),
      cap('renderEpoch', 99, false),
    ],
  },
  {
    name: 'events-bump-the-ledger',
    covers:
      'The effect maps each confirmed document event to the pages whose rasters it staled: annotation and form events bump the annotated ledger, an applied redaction the content ledger, pages and metadata events nothing; coarse form results bump every page of the current registry; closing unsubscribes.',
    plugins: render(),
    engine: { documents: { a: doc() } },
    snapshot: SNAPSHOT,
    steps: [
      start,
      open(),
      emit({ type: 'annotation.created', pageObjectNumber: 22 }),
      emit({ type: 'annotation.updated', pageObjectNumber: 22, origin: { kind: 'remote', sessionId: 'x', sub: 'alice', ts: 1, serverId: 7 } }),
      emit({ type: 'annotation.deleted', pageObjectNumber: 11 }),
      emit({ type: 'annotation.moved', pageObjectNumber: 33 }),
      emit({ type: 'form.valueChanged', changedWidgets: [{ annotObjectNumber: 5, pageObjectNumber: 11 }, { annotObjectNumber: 6, pageObjectNumber: 33 }] }),
      emit({ type: 'form.effectsApplied', changedWidgets: [] }),
      emit({ type: 'form.fieldDeleted', removedWidgets: [{ annotObjectNumber: 5, pageObjectNumber: 22 }] }),
      emit({ type: 'form.fieldCreated', field: { widgets: [{ annotObjectNumber: 5, pageObjectNumber: 11 }] } }),
      emit({ type: 'form.widgetDetached', field: { widgets: [] } }),
      emit({ type: 'redaction.applied', results: [{ pageObjectNumber: 11, status: 'applied' }, { pageObjectNumber: 22, status: 'skipped' }, { pageObjectNumber: 33, status: 'applied' }] }),
      emit({ type: 'redaction.applied', results: [{ pageObjectNumber: 22, status: 'failed' }] }),
      emit({ type: 'metadata.updated', metadata: {} }),
      emit({
        type: 'pages.deleted',
        layout: { pageCount: 2, pages: [layout(11, 0, 612, 792), layout(33, 1, 300, 300)] },
      }),
      emit({ type: 'form.imported' }),
      emit({ type: 'form.repaired' }),
      cap('renderEpoch', 11),
      cap('renderEpoch', 11, false),
      close(),
    ],
  },
  {
    name: 'budget-warning-without-tiles',
    covers:
      'With the tile plane disabled, a demand more than twice the base budget warns once (the page rests blurry); paintSettings says tiles are off and the plane never engages.',
    plugins: render({ tiles: false }),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [
      start,
      open(),
      cap('renderSourceKey', 11, { scale: 2 }),
      cap('renderSourceKey', 11, { scale: 8 }),
      cap('conformViewport', 11, 16),
      cap('paintSettings'),
      cap('tilesFor', 'stage'),
      { op: 'call', target: { ref: 0 }, args: [11, DEEP] },
    ],
  },
  {
    name: 'budget-warning-needs-a-deficit',
    covers: 'The tiles-off warning measures the demand against what the base supplies: a demand at the budget never warns.',
    plugins: render({ tiles: false, fullPage: { maxWidth: 5000 } }),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [start, open(), cap('renderSourceKey', 11, { scale: 8 }), cap('renderSourceKey', 11, { scale: 20 })],
  },
  {
    name: 'paint-settings-resolve',
    covers: 'paintSettings reports the resolved fade and tile switch.',
    plugins: render({ tiles: { fadeMs: 120 } }),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [start, open(), cap('paintSettings')],
  },
  {
    name: 'close-with-a-render-in-flight',
    covers: 'Closing the document removes the ledger; a render already in flight still resolves its consumer when the engine answers.',
    plugins: render(),
    engine: { documents: { a: doc() } },
    snapshot: 'none',
    steps: [start, open(), cap('renderPage', 11, { scale: 0.5 }), close(), settle('r0')],
  },
];

// ── the tile plane ────────────────────────────────────────────────────────────

function tileCase(
  name: string,
  covers: string,
  config: RenderPluginOptions | undefined,
  engineDoc: FakeDocumentSpec,
  build: (v: ReturnType<typeof views>) => Step[],
  extra: Partial<ScenarioCase> = {},
): ScenarioCase {
  const v = views();
  return {
    name,
    covers,
    plugins: extra.plugins ?? render(config),
    engine: { documents: { a: engineDoc } },
    snapshot: SNAPSHOT,
    steps: [start, open(), v.tilesFor('stage'), ...build(v)],
    ...extra,
  };
}

const debug = (tiles: RenderPluginOptions['tiles'] = {}): RenderPluginOptions => ({ debug: true, tiles });

const tileCases: ScenarioCase[] = [
  tileCase(
    'never-engages-below-the-threshold',
    'A thumbnail demand, a demand the base already supplies exactly, an unknown page and a lattice rung that covers the demand all return the empty plan and fetch nothing.',
    undefined,
    doc(),
    (v) => [
      v.plan('stage', 11, { desiredDeviceWidth: 300 }),
      v.plan('stage', 11, { desiredDeviceWidth: 640 }),
      v.plan('stage', 99, DEEP),
      v.plan('stage', 11, { desiredDeviceWidth: 600, visibleRect: visible(0, 0, 10, 10) }, { includeAnnotations: false }),
    ],
  ),
  tileCase(
    'lattice-band-tolerance',
    'Under a lattice tiles engage only past 1.25 × the rung the base supplies.',
    undefined,
    doc({ policy: LATTICE }),
    (v) => [v.plan('stage', 11, { desiredDeviceWidth: 3200 }), v.plan('stage', 11, { desiredDeviceWidth: 3201, visibleRect: visible(0, 0, 50, 50) })],
  ),
  tileCase(
    'engages-past-the-budget',
    'Continuous past the budget: the visible tiles fetch at the exact scale (bled rects, y-up), center-out; arrivals wake the layers once per frame; P1 prefetch starts only when P0 is resolved; the plan is memoized until something changes.',
    debug(),
    doc(),
    (v) => [
      v.plan('stage', 11, DEEP),
      v.plan('stage', 11, DEEP),
      ...settleRange(0, 3),
      advance(16),
      v.plan('stage', 11, DEEP),
      v.plan('stage', 11, DEEP),
      ...settleRange(4, 8),
      advance(15),
      advance(1),
      v.plan('stage', 11, DEEP),
      v.painted('stage', 11, tk(11, 4896, 0, 0)),
      v.painted('stage', 11, tk(11, 4896, 0, 0)),
      v.painted('stage', 11, 'no-such-key'),
      v.plan('stage', 11, DEEP),
      advance(16),
    ],
  ),
  tileCase(
    'center-out-order',
    'Visible tiles fetch nearest the visible center first, ties in row-major order.',
    { tiles: { prefetch: { margin: 0 } } },
    doc(),
    (v) => [v.plan('stage', 11, demand(4896, visible(10, 20, 300, 200))), v.plan('stage', 22, { desiredDeviceWidth: 6336, visibleRect: visible(400, 100, 150, 150) })],
  ),
  tileCase(
    'identity-law-inside-a-rung',
    'Under a lattice the default ×2 pyramid owns tile levels: zoom inside a rung re-plans to the same stamp and fetches nothing.',
    undefined,
    doc({ policy: LATTICE }),
    (v) => [
      v.plan('stage', 11, DEEP),
      ...settleRange(0, 3),
      v.plan('stage', 11, DEEP),
      v.plan('stage', 11, { ...DEEP, desiredDeviceWidth: 4400 }),
      v.plan('stage', 11, { ...DEEP, desiredDeviceWidth: 2449 }),
    ],
  ),
  tileCase(
    'retention-across-a-level-crossing',
    'Zooming in keeps painted coarser tiles in the paint list, stacked under the sharper arrivals, until the covering want tiles report painted; fetch completion alone never releases.',
    debug({ settleMs: 0, prefetch: { margin: 0 } }),
    doc({ policy: LATTICE }),
    (v) => [
      v.plan('stage', 11, DEEP),
      ...settleRange(0, 3),
      v.plan('stage', 11, DEEP),
      ...[tk(11, 4896, 0, 0), tk(11, 4896, 1, 0), tk(11, 4896, 0, 1), tk(11, 4896, 1, 1)].map((k) => v.painted('stage', 11, k)),
      v.plan('stage', 11, DEEPER),
      ...settleRange(4, 7),
      v.plan('stage', 11, DEEPER),
      ...[tk(11, 9792, 0, 0), tk(11, 9792, 1, 0), tk(11, 9792, 0, 1)].map((k) => v.painted('stage', 11, k)),
      v.plan('stage', 11, DEEPER),
      v.painted('stage', 11, tk(11, 9792, 1, 1)),
      v.plan('stage', 11, DEEPER),
    ],
  ),
  tileCase(
    'zoom-out-keeps-fine-tiles',
    'Zooming out keeps the finer painted tiles above the coarse want until the coarse tiles paint.',
    { tiles: { settleMs: 0, prefetch: { margin: 0 } } },
    doc({ policy: LATTICE }),
    (v) => [
      v.plan('stage', 11, DEEPER),
      ...settleRange(0, 3),
      v.plan('stage', 11, DEEPER),
      ...[tk(11, 9792, 0, 0), tk(11, 9792, 1, 0), tk(11, 9792, 0, 1), tk(11, 9792, 1, 1)].map((k) => v.painted('stage', 11, k)),
      v.plan('stage', 11, DEEP),
      ...settleRange(4, 7),
      v.plan('stage', 11, DEEP),
      ...[tk(11, 4896, 0, 0), tk(11, 4896, 1, 0), tk(11, 4896, 0, 1)].map((k) => v.painted('stage', 11, k)),
      v.plan('stage', 11, DEEP),
    ],
  ),
  tileCase(
    'pan-fetches-only-the-exposed-tiles',
    'Panning at one level fetches the newly exposed tiles, keeps the ones still visible, drops resolved ones that left the want set (bytes stay cached) and re-promotes them from the store on a pan back.',
    { tiles: { prefetch: { margin: 0 } } },
    doc(),
    (v) => [
      v.plan('stage', 11, DEEP),
      ...settleRange(0, 3),
      v.plan('stage', 11, { ...DEEP, visibleRect: visible(64, 0, 128, 128) }),
      settle('r4'),
      settle('r5'),
      v.plan('stage', 11, { ...DEEP, visibleRect: visible(64, 0, 128, 128) }),
      v.plan('stage', 11, DEEP),
      v.plan('stage', 11, DEEP),
    ],
  ),
  tileCase(
    'abort-when-leaving-the-want-set',
    'In-flight tiles that leave the want set abort at once (the engine sees the store’s reason), free their transit slots, and a late answer changes nothing.',
    { tiles: { prefetch: { margin: 0 } } },
    doc(),
    (v) => [
      v.plan('stage', 11, DEEP),
      v.plan('stage', 11, { ...DEEP, visibleRect: visible(448, 448, 128, 128) }),
      settle('r0'),
      settleRange(4, 7)[0],
      v.plan('stage', 11, { ...DEEP, visibleRect: visible(448, 448, 128, 128) }),
    ],
  ),
  tileCase(
    'epoch-bump-drops-everything',
    'An invalidation makes retained pixels wrong: every entry of the old epoch drops, in-flight ones abort, the settle timer clears, and the want set refetches under the new key.',
    debug({ settleMs: 100 }),
    doc(),
    (v) => [
      v.plan('stage', 11, DEEP),
      ...settleRange(0, 3),
      v.plan('stage', 11, DEEP),
      ...[tk(11, 4896, 0, 0), tk(11, 4896, 1, 0)].map((k) => v.painted('stage', 11, k)),
      v.plan('stage', 11, demand(5200)),
      cap('invalidate', { pons: [11], scope: 'annotations' }),
      v.plan('stage', 11, DEEP),
      advance(200),
      v.plan('stage', 11, DEEP, { includeAnnotations: false }),
    ],
  ),
  tileCase(
    'disengage-clears-state',
    'Zooming back under the threshold drops the page’s tile bookkeeping and aborts what is in flight; resolved bytes stay in the store, so re-engaging paints from cache.',
    debug(),
    doc(),
    (v) => [
      v.plan('stage', 11, DEEP),
      ...settleRange(0, 2),
      v.plan('stage', 11, { desiredDeviceWidth: 1200 }),
      v.plan('stage', 11, { desiredDeviceWidth: 1200 }),
      v.plan('stage', 11, DEEP),
    ],
  ),
  tileCase(
    'unpaint-withdraws-coverage',
    'A want tile that leaves the DOM stops counting as coverage: the retained coarse tile stays until it paints again.',
    { tiles: { settleMs: 0, prefetch: { margin: 0 } } },
    doc({ policy: LATTICE }),
    (v) => [
      v.plan('stage', 11, DEEP),
      ...settleRange(0, 3),
      v.plan('stage', 11, DEEP),
      ...[tk(11, 4896, 0, 0), tk(11, 4896, 1, 0), tk(11, 4896, 0, 1), tk(11, 4896, 1, 1)].map((k) => v.painted('stage', 11, k)),
      v.plan('stage', 11, DEEPER),
      ...settleRange(4, 7),
      v.plan('stage', 11, DEEPER),
      ...[tk(11, 9792, 0, 0), tk(11, 9792, 1, 0), tk(11, 9792, 0, 1)].map((k) => v.painted('stage', 11, k)),
      v.unpainted('stage', 11, tk(11, 9792, 0, 0)),
      v.unpainted('stage', 11, 'no-such-key'),
      v.unpainted('stage', 99, 'no-such-page'),
      v.painted('stage', 11, tk(11, 9792, 1, 1)),
      v.plan('stage', 11, DEEPER),
      v.painted('stage', 11, tk(11, 9792, 0, 0)),
      v.plan('stage', 11, DEEPER),
    ],
  ),
  tileCase(
    'failure-is-not-retried-at-its-level',
    'A tile that fails (not an abort) wakes the layers and is not refetched at this level; a level change clears the memory and retries it.',
    debug({ prefetch: { margin: 0 }, settleMs: 0 }),
    doc(),
    (v) => [
      v.plan('stage', 11, DEEP),
      fail('r0'),
      ...settleRange(1, 3),
      advance(16),
      v.plan('stage', 11, DEEP),
      v.plan('stage', 11, DEEPER),
      v.plan('stage', 11, DEEP),
    ],
  ),
  tileCase(
    'settle-gate-under-exact',
    'Under exact mode every zoom frame is a new level: first engagement fetches at once, a burst of level changes re-arms one settle timer, and only the final level fetches when it fires.',
    debug({ prefetch: { margin: 0 } }),
    doc(),
    (v) => [
      v.plan('stage', 11, demand(4896)),
      ...settleRange(0, 3),
      v.plan('stage', 11, demand(5200)),
      advance(100),
      v.plan('stage', 11, demand(5800)),
      v.plan('stage', 11, demand(6400)),
      advance(149),
      advance(1),
      ...settleRange(4, 7),
      v.plan('stage', 11, demand(6400)),
      v.plan('stage', 11, demand(6560)),
      advance(200),
      v.plan('stage', 11, demand(6560)),
    ],
  ),
  tileCase(
    'same-pending-level-fires-at-once',
    'Planning the pending level again before the timer fires cancels the timer and fetches immediately.',
    debug({ prefetch: { margin: 0 } }),
    doc(),
    (v) => [
      v.plan('stage', 11, demand(4896)),
      v.plan('stage', 11, demand(5200)),
      v.plan('stage', 11, demand(5200)),
      advance(500),
      v.plan('stage', 11, demand(4896)),
      advance(500),
    ],
  ),
  tileCase(
    'settle-disabled',
    'settleMs 0 disables the gate: a level change fetches at once.',
    { tiles: { settleMs: 0, prefetch: { margin: 0 } } },
    doc(),
    (v) => [v.plan('stage', 11, demand(4896)), v.plan('stage', 11, demand(5200))],
  ),
  tileCase(
    'views-are-isolated',
    'Tile state is per view × page: a rail’s never-engaging plan leaves the main view untouched, a painted report against the wrong view is a no-op, and the store dedupes bytes across views.',
    { tiles: { prefetch: { margin: 0 } } },
    doc(),
    (v) => [
      v.plan('stage', 11, DEEP),
      ...settleRange(0, 3),
      v.plan('stage', 11, DEEP),
      v.tilesFor('thumbs'),
      v.plan('thumbs', 11, { desiredDeviceWidth: 200 }),
      v.painted('thumbs', 11, tk(11, 4896, 0, 0)),
      v.plan('stage', 11, DEEP),
      v.plan('thumbs', 11, DEEP),
      v.painted('stage', 11, tk(11, 4896, 0, 0)),
      v.plan('stage', 11, DEEP),
      v.tilesFor('stage'),
    ],
  ),
  tileCase(
    'release-one-view',
    'Releasing one view’s page aborts only its in-flight consumers: a fetch another view still awaits stays alive, and aborts when the last view lets go.',
    { tiles: { prefetch: { margin: 0 } } },
    doc(),
    (v) => [
      v.plan('stage', 11, DEEP),
      v.tilesFor('thumbs'),
      v.plan('thumbs', 11, DEEP),
      v.release('thumbs', 11),
      v.release('thumbs', 11),
      ...settleRange(0, 1),
      v.release('stage', 11),
      v.plan('stage', 11, DEEP),
    ],
  ),
  tileCase(
    'backpressure',
    'At most eight tile fetches are in transit; each resolution’s replan pumps the next batch, and slots freed by an abort are usable in the same plan.',
    { tiles: { prefetch: { margin: 0 } } },
    doc(),
    (v) => [
      v.plan('stage', 11, demand(4896, visible(0, 0, 512, 384))),
      ...settleRange(0, 7),
      v.plan('stage', 11, demand(4896, visible(0, 0, 512, 384))),
      v.plan('stage', 11, demand(4896, visible(0, 448, 128, 128))),
      v.plan('stage', 11, demand(4896, visible(0, 0, 512, 384))),
    ],
  ),
  tileCase(
    'stage-less-demand-is-capped-exact',
    'Without a visibleRect the whole page tiles, so the level clamps to keep the whole-page tile count near 64, with a one-time warning.',
    { tiles: { prefetch: { margin: 0 } } },
    doc(),
    (v) => [
      v.plan('stage', 11, { desiredDeviceWidth: 80_000 }),
      v.plan('stage', 22, { desiredDeviceWidth: 80_000 }),
      v.plan('stage', 33, { desiredDeviceWidth: 1000 }),
    ],
  ),
  tileCase(
    'stage-less-demand-is-capped-pyramid',
    'Under a pyramid the stage-less clamp picks the largest rung that fits, or the smallest when none does.',
    { tiles: { prefetch: { margin: 0 }, size: 256 } },
    doc({ policy: LATTICE }),
    (v) => [v.plan('stage', 11, { desiredDeviceWidth: 80_000 }), v.plan('stage', 22, { desiredDeviceWidth: 80_000 })],
  ),
  tileCase(
    'stage-less-pyramid-smallest-rung',
    'When no pyramid rung fits the stage-less clamp, the smallest rung is used.',
    { tiles: { prefetch: { margin: 0 }, quantize: [4, 8], size: 64 } },
    doc(),
    (v) => [v.plan('stage', 11, { desiredDeviceWidth: 80_000 })],
  ),
  tileCase(
    'evicted-tile-bytes-refetch',
    'The store owns the bytes: a resolved tile whose bytes were evicted leaves the paint list on the next plan and is fetched again.',
    { tiles: { prefetch: { margin: 0 } } },
    doc(),
    (v) => [
      v.plan('stage', 11, DEEP),
      settle('r0', handle(10000, 10000)),
      settle('r1', handle(10000, 10000)),
      v.plan('stage', 11, DEEP),
      settle('r2'),
      settle('r3'),
      v.plan('stage', 11, DEEP),
      v.plan('stage', 11, DEEP),
    ],
  ),
  tileCase(
    'base-tiles-ignore-annotation-epochs',
    'Tiles planned without annotations key a0 and version on content only: an annotation invalidate keeps them, a content invalidate drops them.',
    { tiles: { prefetch: { margin: 0 } } },
    doc(),
    (v) => [
      v.plan('stage', 11, DEEP, { includeAnnotations: false }),
      ...settleRange(0, 3),
      v.painted('stage', 11, tk(11, 4896, 0, 0, { a: 0 })),
      cap('invalidate', { pons: [11], scope: 'annotations' }),
      v.plan('stage', 11, DEEP, { includeAnnotations: false }),
      cap('invalidate', { pons: [11], scope: 'content' }),
      v.plan('stage', 11, DEEP, { includeAnnotations: false }),
    ],
  ),
  tileCase(
    'lattice-tiles-block',
    'An advertised tiles block wins: its scales are the pyramid, its first tile size replaces an unlisted one, and the conformed format rides into tile fetches and keys (whose geometry names the option size).',
    { format: 'png', tiles: { prefetch: { margin: 0 } } },
    doc({ policy: LATTICE_TILES }),
    (v) => [v.plan('stage', 11, demand(3000, visible(0, 0, 100, 100)))],
  ),
  tileCase(
    'lattice-tiles-block-keeps-a-listed-size',
    'A tile size the tiles block lists is kept.',
    { tiles: { size: 1024, prefetch: { margin: 0 } } },
    doc({ policy: LATTICE_TILES }),
    (v) => [v.plan('stage', 11, demand(3000, visible(0, 0, 100, 100)))],
  ),
  tileCase(
    'client-tile-ladder',
    'A client tile ladder under continuous snaps levels up its rungs, and tile fetches carry the strategy quality.',
    { tiles: { quantize: [2, 4], prefetch: { margin: 0 } }, quality: 0.5, format: 'webp' },
    doc(),
    (v) => [v.plan('stage', 11, demand(1800, visible(0, 0, 200, 200))), v.plan('stage', 11, demand(9000, visible(0, 0, 200, 200)))],
  ),
  tileCase(
    'max-scale-clamps-exact-levels',
    'Exact levels clamp at maxScale × page width.',
    { tiles: { maxScale: 4, prefetch: { margin: 0 } } },
    doc(),
    (v) => [v.plan('stage', 11, demand(9000, visible(0, 0, 100, 100)))],
  ),
  tileCase(
    'prefetch-ring-velocity-bias',
    'The prefetch ring stretches toward the scroll direction; with velocityBias off the ring is symmetric.',
    undefined,
    doc(),
    (v) => [
      v.plan('stage', 11, { ...demand(4896, visible(128, 128, 128, 128)), velocity: { dx: 10, dy: -3 } }),
      ...settleRange(0, 3),
      v.plan('stage', 11, { ...demand(4896, visible(128, 128, 128, 128)), velocity: { dx: 10, dy: -3 } }),
    ],
  ),
  tileCase(
    'prefetch-ring-without-bias',
    'velocityBias false ignores the velocity.',
    { tiles: { prefetch: { velocityBias: false, margin: 1 } } },
    doc(),
    (v) => [
      v.plan('stage', 11, { ...demand(4896, visible(128, 128, 64, 64)), velocity: { dx: 10, dy: 3 } }),
      settle('r0'),
      v.plan('stage', 11, { ...demand(4896, visible(128, 128, 64, 64)), velocity: { dx: 10, dy: 3 } }),
    ],
  ),
  tileCase(
    'retained-blocked-diagnostics',
    'With debug on, a retained tile that cannot release says which want tiles block it: missing, pending, or resolved but unpainted.',
    debug({ prefetch: { margin: 0 }, settleMs: 0 }),
    doc({ policy: LATTICE }),
    (v) => [
      v.plan('stage', 11, DEEP),
      ...settleRange(0, 3),
      v.plan('stage', 11, DEEP),
      v.painted('stage', 11, tk(11, 4896, 0, 0)),
      v.plan('stage', 11, demand(9792, visible(0, 0, 200, 200))),
      settle('r4'),
      settle('r5'),
      v.plan('stage', 11, demand(9792, visible(0, 0, 200, 200))),
    ],
  ),
  tileCase(
    'permission-denied-tiles',
    'A denied session’s tile fetches refuse locally with PermissionDenied (context render.tile): no engine calls, each key marked failed, the layers woken.',
    debug({ prefetch: { margin: 0 } }),
    doc({ allows: [] }),
    (v) => [v.plan('stage', 11, DEEP), advance(16), v.plan('stage', 11, DEEP)],
  ),
  tileCase(
    'close-with-tiles-in-flight',
    'Closing the document does not stop the tile manager: an armed settle timer still fires and fetches through the retained handle, late answers still wake, and a plan after close is empty.',
    debug({ prefetch: { margin: 0 } }),
    doc(),
    (v) => [
      v.plan('stage', 11, DEEP),
      ...settleRange(0, 3),
      v.plan('stage', 11, demand(5200)),
      close(),
      advance(200),
      settle('r4'),
      advance(16),
      v.plan('stage', 11, DEEP),
    ],
  ),
  tileCase(
    'page-size-change-reuses-width-keys',
    'A page registry update (rotation) changes the page size under a live plan: level identity is the device width, so old-scale tiles share keys with the new grid.',
    { tiles: { prefetch: { margin: 0 } } },
    doc(),
    (v) => [
      v.plan('stage', 11, DEEP),
      ...settleRange(0, 3),
      emit({ type: 'pages.rotated', layout: { pageCount: 3, pages: [layout(11, 0, 792, 612), PAGES[1], PAGES[2]] } }),
      v.plan('stage', 11, DEEP),
      v.painted('stage', 11, tk(11, 4896, 0, 0)),
      emit({ type: 'pages.deleted', layout: { pageCount: 2, pages: [PAGES[1], PAGES[2]] } }),
      v.painted('stage', 11, tk(11, 4896, 1, 0)),
      v.plan('stage', 11, DEEP),
    ],
  ),
  tileCase(
    'plan-throws-when-the-engine-refuses-the-page',
    'A page the registry lists but the handle refuses throws out of plan (doc.page inside the fetch), after the entry was recorded and its slot charged.',
    { tiles: { prefetch: { margin: 0 } } },
    doc({ responses: [{ path: 'pages.list', value: { pageCount: 4, pages: [...PAGES, layout(44, 3, 612, 792)] } }] }),
    (v) => [v.plan('stage', 44, DEEP), v.plan('stage', 44, DEEP), v.plan('stage', 11, DEEP)],
  ),
  tileCase(
    'no-bleed',
    'bleed 0 paints and fetches the logical tile rects; disengaging without debug drops state silently and re-engaging paints from the store.',
    { tiles: { bleed: 0, prefetch: { margin: 0 } } },
    doc(),
    (v) => [
      v.plan('stage', 11, DEEP),
      ...settleRange(0, 3),
      v.plan('stage', 11, DEEP),
      v.plan('stage', 11, { desiredDeviceWidth: 500 }),
      v.plan('stage', 11, DEEP),
    ],
  ),
  tileCase(
    'engage-at-option',
    'An explicit engageAt replaces the resolved default.',
    { tiles: { engageAt: 10, prefetch: { margin: 0 } } },
    doc(),
    (v) => [v.plan('stage', 11, DEEP), v.plan('stage', 11, demand(7000))],
  ),
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/render-capability.json',
    describes: 'packages/plugin/render: keys, conformance, the raster store, epochs, events, permissions.',
    cases: capabilityCases,
  },
  {
    kind: 'scenario',
    file: 'plugins/render-tile-plane.json',
    describes: 'packages/plugin/render: the tile manager behind tilesFor (engagement, levels, want sets, retention, settle, backpressure).',
    cases: tileCases,
  },
];
