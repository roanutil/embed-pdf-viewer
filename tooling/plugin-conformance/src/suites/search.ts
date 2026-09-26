/**
 * `packages/plugin/search`: the session, the session-free `findAll`
 * service, navigation through the Stage, the permission twin and the
 * mutation-rerun effect, driven through the kernel and the fake engine.
 * Replayed by `crates/plugins/search/tests/conformance.rs`.
 *
 * One harness shim lives here, and `crates/plugins/search/src/harness.rs`
 * mirrors it: `findAll`'s `signal`. A vector cannot carry an `AbortSignal`,
 * so the `search` factory wraps the capability: an option `signal:
 * {"$abortSignal": name}` becomes the signal of a controller named `name`,
 * and the `search-signals` test plugin's `abort(name, reason?)` aborts it.
 * Without a reason the signal's reason is the platform's `AbortError`
 * DOMException; with one (an `$error` spec) it is that error.
 *
 * The Stage cases use the `stage` factory `stage-plugin.ts` registers.
 */
import { createCapabilityToken, type AnyPlugin } from '@embedpdf/core';

import { SearchToken, searchPlugin } from '../../../../packages/plugin/search/src/index';
import type { SearchCapability, SearchConfig as SearchPluginConfig } from '../../../../packages/plugin/search/src/contract';

import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import { registerPlugin, type PluginSpec, type ScenarioCase, type Step } from '../scenario';
import type { Suite } from '../vectors';
import { stageStep } from './stage-plugin';

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

const withSignals = (capability: SearchCapability): SearchCapability => ({
  ...capability,
  findAll: (query, opts) => {
    const spec = (opts as { signal?: { $abortSignal?: string } } | undefined)?.signal;
    if (spec?.$abortSignal === undefined) return capability.findAll(query, opts);
    return capability.findAll(query, { ...opts, signal: controllerNamed(spec.$abortSignal).signal });
  },
});

registerPlugin('search', {
  factory: (config) => {
    controllers.clear();
    const plugin = searchPlugin(config as SearchPluginConfig | undefined) as AnyPlugin;
    return {
      ...plugin,
      create: (ctx) => {
        const { api, connect } = plugin.create!(ctx);
        return { api: withSignals(api as SearchCapability), connect };
      },
    } as AnyPlugin;
  },
  tokens: [SearchToken],
});

const SignalsToken = createCapabilityToken<{ abort(name: string, reason?: unknown): void }>('search-test/signals');
registerPlugin('search-signals', {
  factory: () =>
    ({
      id: 'search-test/signals',
      token: SignalsToken,
      capability: () => ({
        abort: (name: string, reason?: unknown) =>
          reason === undefined ? controllerNamed(name).abort() : controllerNamed(name).abort(reason),
      }),
    }) as AnyPlugin,
  tokens: [SignalsToken],
});

// ── fixtures ──────────────────────────────────────────────────────────────────

const pr = (pon: number) => ({ kind: 'objectNumber', pageObjectNumber: pon });

const layout = (
  pon: number,
  index: number,
  crop: { left: number; bottom: number; right: number; top: number },
  rotation = 0,
  userUnit = 1,
) => ({
  index,
  ref: pr(pon),
  label: null,
  size: { width: crop.right - crop.left, height: crop.top - crop.bottom },
  rotation,
  userUnit,
  boxes: { media: crop, crop },
});

/** Three pages: plain, an offset crop turned 90° at userUnit 2, and a square. */
const PAGES = [
  layout(11, 0, { left: 0, bottom: 0, right: 600, top: 800 }),
  layout(22, 1, { left: 10, bottom: 20, right: 310, top: 420 }, 90, 2),
  layout(33, 2, { left: 0, bottom: 0, right: 300, top: 300 }),
];

type PdfRect = { left: number; bottom: number; right: number; top: number };
const pt = (x: number, y: number) => ({ x, y });

/** An axis-aligned engine segment over a PDF rect. */
const seg = (r: PdfRect, advance: 1 | -1 = 1) => ({
  quad: { p1: pt(r.left, r.top), p2: pt(r.right, r.top), p3: pt(r.left, r.bottom), p4: pt(r.right, r.bottom) },
  rect: r,
  advance,
});

const m = (pon: number, charStart: number, segments: unknown[] = [seg({ left: 10, bottom: 20, right: 50, top: 40 })], extra: Record<string, unknown> = {}) => ({
  page: pr(pon),
  charStart,
  charCount: 4,
  segments,
  ...extra,
});

const slice = (matches: unknown[], nextCursor: string | null, scannedPages: number, totalPages = 3) => ({
  matches,
  nextCursor,
  scannedPages,
  totalPages,
});

const Q = { text: 'the' };
const Q2 = { text: 'other', matchCase: true };

/** A `search.query` request as the capability builds it. */
const req = (query: unknown, mode: 'full' | 'rects', at: { cursor?: string; startPage?: number } = {}) => [
  { query, mode, ...at, ...(at.startPage === undefined ? {} : { startPage: pr(at.startPage) }) },
];

const answer = (query: unknown, mode: 'full' | 'rects', at: { cursor?: string; startPage?: number }, value: unknown, times?: number): ResponseSpec => ({
  path: 'search.query',
  args: req(query, mode, at),
  value,
  ...(times === undefined ? {} : { times }),
});
const deferAt = (query: unknown, mode: 'full' | 'rects', at: { cursor?: string; startPage?: number }, label: string, times?: number): ResponseSpec => ({
  path: 'search.query',
  args: req(query, mode, at),
  defer: label,
  ...(times === undefined ? {} : { times }),
});
const failAt = (
  query: unknown,
  mode: 'full' | 'rects',
  at: { cursor?: string; startPage?: number },
  error: { name: string; message: string; [k: string]: unknown },
  times?: number,
): ResponseSpec => ({
  path: 'search.query',
  args: req(query, mode, at),
  error,
  ...(times === undefined ? {} : { times }),
});

const DENIED = { name: 'PermissionDenied', message: 'permission denied (local): doc.text.copy', required: 'doc.text.copy', context: 'local' };
const FORBIDDEN = { name: 'EngineError', message: 'forbidden', code: 'Forbidden' };
const STALE = { name: 'EngineError', message: 'stale cursor', code: 'InvalidArg' };
const BOOM = { name: 'EngineError', message: 'boom', code: 'Unknown' };

const doc = (responses: ResponseSpec[], over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec => ({
  security: { allows: ['doc.text.search', 'doc.text.copy'] },
  pages: PAGES,
  responses,
  ...over,
});

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const open = (id = 'a'): Step => ({ op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id, bytes: { $bytes: '' } }] });
const close = (id = 'a'): Step => ({ op: 'call', target: 'documents', method: 'close', args: [id] });
/** v2 search method → v3 name. `hitsForPage(pon)` is `listHits({page})`, and
 *  a search's `startPage` is a PageRef. */
const RENAMED: Readonly<Record<string, string>> = {
  rerun: 'refresh',
  next: 'nextHit',
  prev: 'previousHit',
  goTo: 'goToHit',
  query: 'getQuery',
  status: 'getStatus',
  hits: 'listHits',
  hitCount: 'getHitCount',
  activeIndex: 'getActiveHitIndex',
  activeHit: 'getActiveHit',
  progress: 'getProgress',
  errorMessage: 'getError',
};
const s = (method: string, ...args: unknown[]): Step => {
  const target = { token: 'search', doc: 'a' };
  if (method === 'hitsForPage') return { op: 'call', target, method: 'listHits', args: [{ page: pr(args[0] as number) }] };
  if (method === 'search' && args[1] && typeof (args[1] as { startPage?: unknown }).startPage === 'number') {
    const options = args[1] as { startPage: number };
    return { op: 'call', target, method, args: [args[0], { ...options, startPage: pr(options.startPage) }] };
  }
  return { op: 'call', target, method: RENAMED[method] ?? method, args };
};
const st = (method: string, ...args: unknown[]): Step => stageStep('stage', 'a', method, args);
const settle = (label: string, value: unknown): Step => ({ op: 'settle', label, value });
const reject = (label: string, error: { name: string; message: string; [k: string]: unknown }): Step => ({ op: 'settle', label, error });
const abortSignal = (name: string, reason?: unknown): Step => ({
  op: 'call',
  target: { token: 'search-test/signals' },
  method: 'abort',
  args: reason === undefined ? [name] : [name, reason],
});
const emit = (event: unknown): Step => ({ op: 'emit', doc: 'a', event });
const advance = (ms: number): Step => ({ op: 'advance', ms });
const signal = (name: string) => ({ $abortSignal: name });

/** Every read the session exposes, in one burst. */
const reads: Step[] = [
  s('query'),
  s('status'),
  s('hits'),
  s('hitCount'),
  s('activeIndex'),
  s('activeHit'),
  s('hitsForPage', 11),
  s('hitsForPage', 99),
  s('progress'),
  s('errorMessage'),
];

const search = (config?: SearchPluginConfig): PluginSpec[] => [config === undefined ? { factory: 'search' } : { factory: 'search', config }];
const withSignals_ = (): PluginSpec[] => [{ factory: 'search-signals' }, ...search()];
const withStage = (config?: SearchPluginConfig): PluginSpec[] => [{ factory: 'stage', config: { viewUnitsPerPoint: 1 } }, ...search(config)];

const SNAPSHOT = ['search::a'];
const STAGE_SNAPSHOT = ['search::a', 'stage::a'];

// Two slices over the three pages, answered at once.
const CHAIN: ResponseSpec[] = [
  answer(Q, 'full', {}, slice([m(11, 0), m(11, 9)], 'c1', 1)),
  answer(Q, 'full', { cursor: 'c1' }, slice([m(22, 2), m(33, 5)], null, 3)),
];

// ── the session ───────────────────────────────────────────────────────────────

const sessionCases: ScenarioCase[] = [
  {
    name: 'initial-reads',
    covers: 'Before any search every read answers the idle state: null query, no hits, -1 active, zero progress, no error; next/prev/goTo with no hits return null and dispatch nothing.',
    plugins: search(),
    engine: { documents: { a: doc([]) } },
    snapshot: SNAPSHOT,
    steps: [start, open(), ...reads, s('next'), s('prev'), s('goTo', 3)],
  },
  {
    name: 'streams-a-cursor-chain',
    covers:
      'search() dispatches START, requests full mode with no start page (no Stage), APPENDs each slice with cumulative progress, threads the cursor, and COMPLETEs on a null cursor; the first hit becomes active and hitsByPage indexes by pon.',
    plugins: search(),
    engine: { documents: { a: doc(CHAIN) } },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), ...reads, s('hitsForPage', 22)],
  },
  {
    name: 'deferred-slices-stream-in',
    covers:
      'With each slice deferred, state is usable between them: status stays searching, an empty slice only advances progress, and the next request goes out only after the previous slice lands.',
    plugins: search(),
    engine: {
      documents: {
        a: doc([
          deferAt(Q, 'full', {}, 's1'),
          deferAt(Q, 'full', { cursor: 'c1' }, 's2'),
          deferAt(Q, 'full', { cursor: 'c2' }, 's3'),
        ]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [
      start,
      open(),
      s('search', Q),
      s('status'),
      settle('s1', slice([], 'c1', 1)),
      s('activeIndex'),
      settle('s2', slice([m(22, 1)], 'c2', 2)),
      s('activeHit'),
      s('status'),
      settle('s3', slice([m(33, 0), m(22, 7)], null, 3)),
      s('status'),
      s('hitsForPage', 22),
    ],
  },
  {
    name: 'a-new-search-supersedes',
    covers:
      'A second search() aborts the running slice with reason "superseded" and restarts from START; the old loop stops silently, and a late answer to the aborted slice changes nothing.',
    plugins: search(),
    engine: {
      documents: {
        a: doc([deferAt(Q, 'full', {}, 'old'), deferAt(Q2, 'full', {}, 'new')]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [
      start,
      open(),
      s('search', Q),
      s('search', Q2),
      settle('old', slice([m(11, 0)], null, 3)),
      settle('new', slice([m(33, 4)], null, 3)),
      s('query'),
      s('hitCount'),
    ],
  },
  {
    name: 'superseding-between-slices',
    covers:
      'Superseding after a slice has landed aborts the in-flight follow-up; aborting a settled slice is a no-op, and the stale loop never dispatches again.',
    plugins: search(),
    engine: {
      documents: {
        a: doc([
          answer(Q, 'full', {}, slice([m(11, 0)], 'c1', 1)),
          deferAt(Q, 'full', { cursor: 'c1' }, 'follow'),
          answer(Q2, 'full', {}, slice([m(22, 3)], null, 3)),
        ]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), s('search', Q2), settle('follow', slice([m(33, 1)], null, 3)), s('hits')],
  },
  {
    name: 'a-completed-session-is-not-aborted',
    covers: 'Searching again after completion aborts nothing: the finished loop cleared its in-flight handle.',
    plugins: search(),
    engine: { documents: { a: doc([...CHAIN, answer(Q2, 'full', {}, slice([], null, 3))]) } },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), s('search', Q2), s('status'), s('activeHit')],
  },
  {
    name: 'clear-aborts-and-resets',
    covers:
      'clear() aborts the in-flight slice with reason "cleared" and dispatches CLEAR; a late answer is ignored; clear() when idle still dispatches CLEAR and aborts nothing.',
    plugins: search(),
    engine: { documents: { a: doc([deferAt(Q, 'full', {}, 's1')]) } },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('clear'), s('search', Q), s('clear'), settle('s1', slice([m(11, 0)], null, 3)), ...reads],
  },
  {
    name: 'empty-text-is-clear',
    covers: 'search() with an empty text makes no engine call: it is clear(), aborting a running session with "cleared".',
    plugins: search(),
    engine: { documents: { a: doc([deferAt(Q, 'full', {}, 's1')]) } },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', { text: '' }), s('search', Q), s('search', { text: '', matchCase: true }), s('status')],
  },
  {
    name: 'query-flags-reach-the-engine',
    covers: 'The stored query is the caller\'s SearchQuery verbatim, every flag included, and the request carries it unchanged.',
    plugins: search(),
    engine: {
      documents: {
        a: doc([
          answer({ text: 'caf', regex: true, matchCase: false, wholeWord: true, matchDiacritics: false }, 'full', {}, slice([], null, 3)),
        ]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', { text: 'caf', regex: true, matchCase: false, wholeWord: true, matchDiacritics: false }), s('query')],
  },
  {
    name: 'exec-start-page',
    covers: 'exec.startPage seeds the first request; the cursor then owns position and startPage is not sent again.',
    plugins: search(),
    engine: {
      documents: {
        a: doc([
          answer(Q, 'full', { startPage: 22 }, slice([m(22, 0)], 'c1', 1)),
          answer(Q, 'full', { cursor: 'c1' }, slice([m(33, 0), m(11, 0)], null, 3)),
          answer(Q2, 'full', {}, slice([], null, 3)),
        ]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q, { startPage: 22 }), s('search', Q2, {}), s('status')],
  },
];

// ── permission degradation, stale cursors, errors ─────────────────────────────

const failureCases: ScenarioCase[] = [
  {
    name: 'snippets-denied-degrade-to-rects',
    covers:
      'A PermissionDenied on the first full request degrades to rects and retries from the same origin with no second START; the rest of the chain stays in rects.',
    plugins: search(),
    engine: {
      documents: {
        a: doc([
          failAt(Q, 'full', {}, DENIED),
          answer(Q, 'rects', {}, slice([m(11, 0)], 'c1', 1)),
          answer(Q, 'rects', { cursor: 'c1' }, slice([m(22, 0)], null, 3)),
        ]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), s('status'), s('hitCount')],
  },
  {
    name: 'forbidden-degrades-too',
    covers: 'The cloud refusal, EngineError Forbidden, degrades the same way.',
    plugins: search(),
    engine: {
      documents: { a: doc([failAt(Q, 'full', {}, FORBIDDEN), answer(Q, 'rects', {}, slice([m(33, 2)], null, 3))]) },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), s('status')],
  },
  {
    name: 'denied-in-rects-is-an-error',
    covers: 'Refused again in rects mode, the session enters error with the refusal\'s message.',
    plugins: search(),
    engine: { documents: { a: doc([failAt(Q, 'full', {}, DENIED), failAt(Q, 'rects', {}, { ...DENIED, message: 'permission denied (local): doc.text.search', required: 'doc.text.search' })]) } },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), s('status'), s('errorMessage')],
  },
  {
    name: 'denied-mid-chain-is-an-error',
    covers: 'A refusal on a cursor request does not degrade: position would be lost, so the session errors.',
    plugins: search(),
    engine: {
      documents: { a: doc([answer(Q, 'full', {}, slice([m(11, 0)], 'c1', 1)), failAt(Q, 'full', { cursor: 'c1' }, DENIED)]) },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), s('hitCount'), s('errorMessage')],
  },
  {
    name: 'stale-cursor-restarts-once',
    covers:
      'An InvalidArg on a cursor request restarts from scratch once, with a fresh START (hits reset) and the original origin; the restarted chain completes.',
    plugins: search(),
    engine: {
      documents: {
        a: doc([
          answer(Q, 'full', { startPage: 22 }, slice([m(22, 0)], 'c1', 1), 1),
          failAt(Q, 'full', { cursor: 'c1' }, STALE, 1),
          answer(Q, 'full', { startPage: 22 }, slice([m(22, 0), m(22, 8)], 'c2', 1), 1),
          answer(Q, 'full', { cursor: 'c2' }, slice([m(33, 0)], null, 3), 1),
        ]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q, { startPage: 22 }), s('hits'), s('status')],
  },
  {
    name: 'a-second-stale-cursor-is-an-error',
    covers: 'The restart happens once: a second InvalidArg on a cursor errors the session.',
    plugins: search(),
    engine: {
      documents: {
        a: doc([answer(Q, 'full', {}, slice([m(11, 0)], 'c1', 1)), failAt(Q, 'full', { cursor: 'c1' }, STALE)]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), s('status'), s('errorMessage')],
  },
  {
    name: 'invalid-query-is-an-error',
    covers: 'An InvalidArg with no cursor (a bad regex, a regex with matchDiacritics) is not a stale cursor: the session errors at once.',
    plugins: search(),
    engine: {
      documents: {
        a: doc([failAt({ text: '(', regex: true }, 'full', {}, { name: 'EngineError', message: 'invalid regex', code: 'InvalidArg' })]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', { text: '(', regex: true }), ...reads],
  },
  {
    name: 'engine-error-after-hits',
    covers: 'A later slice failing keeps the hits already streamed and records the error message; a new search clears it.',
    plugins: search(),
    engine: {
      documents: {
        a: doc([
          answer(Q, 'full', {}, slice([m(11, 0)], 'c1', 1)),
          deferAt(Q, 'full', { cursor: 'c1' }, 's2'),
          answer(Q2, 'full', {}, slice([], null, 3)),
        ]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), reject('s2', BOOM), s('status'), s('hitCount'), s('errorMessage'), s('search', Q2), s('errorMessage')],
  },
  {
    name: 'non-engine-errors-report-their-message',
    covers: 'Any Error the engine rejects with reaches ERROR as its message.',
    plugins: search(),
    engine: { documents: { a: doc([failAt(Q, 'full', {}, { name: 'TypeError', message: 'worker gone' })]) } },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), s('errorMessage')],
  },
  {
    name: 'a-geometry-failure-is-an-error',
    covers: 'A page whose geometry cannot invert (userUnit 0) throws while converting its hits, and the session errors with the geometry message.',
    plugins: search(),
    engine: {
      documents: {
        a: doc([answer(Q, 'full', {}, slice([m(44, 0)], null, 1, 1))], {
          pages: [layout(44, 0, { left: 0, bottom: 0, right: 100, top: 100 }, 0, 0)],
        }),
      },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), s('status'), s('errorMessage')],
  },
  {
    name: 'rerun',
    covers:
      'rerun() while idle does nothing; after completion, or after an error, it replays the stored query from scratch.',
    plugins: search(),
    engine: {
      documents: {
        a: doc([
          answer(Q, 'full', {}, slice([m(11, 0)], null, 3), 1),
          failAt(Q, 'full', {}, BOOM, 1),
          answer(Q, 'full', {}, slice([m(33, 0)], null, 3), 1),
        ]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('rerun'), s('search', Q), s('rerun'), s('status'), s('rerun'), s('status'), s('hits')],
  },
  {
    name: 'close-mid-search',
    covers:
      'Closing the document does not abort the session: a slice answered after close is still converted and dispatched into the removed slice.',
    plugins: search(),
    engine: { documents: { a: doc([deferAt(Q, 'full', {}, 's1')]) } },
    steps: [start, open(), s('search', Q), close(), settle('s1', slice([m(11, 0)], null, 3))],
  },
];

// ── hits: geometry and snippets ───────────────────────────────────────────────

const geometryCases: ScenarioCase[] = [
  {
    name: 'hit-geometry',
    covers:
      'Engine quads map from PDF space to content space through the page\'s crop (rotation and userUnit do not enter at zoom 1), rect is recomputed from the mapped quad, bounds is the union of segments, advance and snippet ride along; a match with no segments has no bounds, a null snippet is dropped, and a match on an unknown page is dropped (cached per slice).',
    plugins: search(),
    engine: {
      documents: {
        a: doc([
          answer(
            Q,
            'full',
            {},
            slice(
              [
                m(11, 0, [seg({ left: 10, bottom: 700, right: 90, top: 720 }), seg({ left: 10, bottom: 680, right: 40, top: 700 })], {
                  snippet: { text: 'in the end', matchStart: 3, matchLength: 3 },
                }),
                m(22, 3, [
                  {
                    quad: { p1: pt(50, 400), p2: pt(50, 300), p3: pt(30, 400), p4: pt(30, 300) },
                    rect: { left: 30, bottom: 300, right: 50, top: 400 },
                    advance: -1,
                  },
                ]),
                m(99, 0),
                m(33, 1, [], { snippet: null }),
                m(99, 5),
                m(22, 9, [
                  {
                    quad: { p1: pt(100.5, 200.25), p2: pt(140.75, 210), p3: pt(98, 190), p4: pt(138.125, 199.5) },
                    rect: { left: 98, bottom: 190, right: 140.75, top: 210 },
                    advance: 1,
                  },
                ]),
              ],
              null,
              3,
            ),
          ),
        ]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), s('hitsForPage', 22), s('hitsForPage', 33)],
  },
];

// ── the permission twin ───────────────────────────────────────────────────────

const twinCases: ScenarioCase[] = [
  {
    name: 'can-search',
    covers:
      "canSearch() and canSearch('rects') ask doc.text.search; canSearch('full') also needs doc.text.copy; copy alone finds nothing; the wildcard grants both.",
    plugins: search(),
    engine: {
      documents: {
        a: doc([], { security: { allows: ['doc.text.search'] } }),
        b: doc([], { security: { allows: ['doc.text.search', 'doc.text.copy'] } }),
        c: doc([], { security: { allows: ['doc.text.copy'] } }),
        d: doc([], { security: { allows: '*' } }),
      },
    },
    snapshot: 'none',
    steps: [
      start,
      open('a'),
      open('b'),
      open('c'),
      open('d'),
      ...['a', 'b', 'c', 'd'].flatMap((id): Step[] => [
        { op: 'call', target: { token: 'search', doc: id }, method: 'canSearch', args: [] },
        { op: 'call', target: { token: 'search', doc: id }, method: 'canSearch', args: ['rects'] },
        { op: 'call', target: { token: 'search', doc: id }, method: 'canSearch', args: ['full'] },
      ]),
    ],
  },
  {
    name: 'verbs-carry-no-gate',
    covers: 'With search denied the verbs still ask the engine (the engine enforces; the error state reports).',
    plugins: search(),
    engine: {
      documents: {
        a: doc(
          [failAt(Q, 'full', {}, DENIED), failAt(Q, 'rects', {}, { ...DENIED, message: 'permission denied (local): doc.text.search', required: 'doc.text.search' })],
          { security: { allows: [] } },
        ),
      },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('canSearch'), s('search', Q), s('status')],
  },
];

// ── navigation ────────────────────────────────────────────────────────────────

const FOUR: ResponseSpec[] = [answer(Q, 'full', {}, slice([m(11, 0), m(11, 9), m(22, 2), m(33, 5, [])], null, 3))];

const navigationCases: ScenarioCase[] = [
  {
    name: 'navigation-wraps',
    covers:
      'next/prev/goTo dispatch SET_ACTIVE with the index wrapped modulo the hit count (negative, more than twice the count below zero, and past the end alike) and return the hit; with no Stage nothing moves.',
    plugins: search(),
    engine: { documents: { a: doc(FOUR) } },
    snapshot: SNAPSHOT,
    steps: [
      start,
      open(),
      s('search', Q),
      s('next'),
      s('next'),
      s('next'),
      s('next'),
      s('prev'),
      s('prev'),
      s('goTo', -1),
      s('goTo', 9),
      s('goTo', -6),
      s('goTo', -9),
      s('goTo', 2, { behavior: 'instant' }),
      s('activeIndex'),
      s('activeHit'),
    ],
  },
  {
    name: 'navigation-before-the-first-slice',
    covers: 'While the first slice is in flight there are no hits: navigation returns null and dispatches nothing.',
    plugins: search(),
    engine: { documents: { a: doc([deferAt(Q, 'full', {}, 's1')]) } },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), s('next'), s('goTo', 0), settle('s1', slice([m(11, 0)], null, 3)), s('prev')],
  },
];

// ── the Stage ─────────────────────────────────────────────────────────────────

const stageCases: ScenarioCase[] = [
  {
    name: 'viewport-first-start-page',
    covers:
      "With a Stage the scan starts at the current page's pon; exec.startPage overrides it; rerun() starts viewport-first again from wherever the Stage now is.",
    plugins: withStage(),
    engine: {
      documents: {
        a: doc([
          answer(Q, 'full', { startPage: 11 }, slice([m(11, 0)], null, 3), 1),
          answer(Q, 'full', { startPage: 33 }, slice([m(33, 0)], null, 3), 1),
          answer(Q, 'full', { startPage: 22 }, slice([m(22, 0)], null, 3), 1),
          answer(Q, 'full', { startPage: 33 }, slice([m(33, 0), m(11, 0)], null, 3), 1),
        ]),
      },
    },
    snapshot: STAGE_SNAPSHOT,
    steps: [
      start,
      open(),
      st('setViewport', { width: 800, height: 600 }),
      s('search', Q),
      st('goToPage', 2),
      s('search', Q),
      s('search', Q, { startPage: 22 }),
      s('rerun'),
    ],
  },
  {
    name: 'no-pages-no-start-page',
    covers: 'A Stage over a document with no pages has no current page to name, so the scan starts with no startPage.',
    plugins: withStage(),
    engine: { documents: { a: doc([answer(Q, 'full', {}, slice([], null, 0, 0))], { pages: [] }) } },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), s('status')],
  },
  {
    name: 'navigation-reveals-the-hit',
    covers:
      "next/prev/goTo reveal the hit's page at its bounds with the find-bar default arrival (anchor y 0.35, smooth); a hit with no segments reveals with no rect.",
    plugins: withStage(),
    engine: { documents: { a: doc([answer(Q, 'full', { startPage: 11 }, slice([m(11, 0), m(22, 2), m(33, 5, [])], null, 3))]) } },
    snapshot: STAGE_SNAPSHOT,
    steps: [start, open(), st('setViewport', { width: 800, height: 600 }), s('search', Q), s('next'), s('next'), s('prev'), s('goTo', 0), st('currentPage')],
  },
  {
    name: 'reveal-options-merge',
    covers:
      'Arrival options merge per key: per-call over plugin config over the default. An explicit undefined anchor overrides the default away (a bare reveal when the hit has no rect); a null config anchor is a positioned reveal with no anchor axes.',
    plugins: withStage({ reveal: { anchor: { x: 'center', y: 'start' }, behavior: 'instant' } }),
    engine: { documents: { a: doc([answer(Q, 'full', { startPage: 11 }, slice([m(11, 0), m(22, 2), m(33, 5, [])], null, 3))]) } },
    snapshot: STAGE_SNAPSHOT,
    steps: [
      start,
      open(),
      st('setViewport', { width: 800, height: 600 }),
      s('search', Q),
      s('next'),
      s('goTo', 0, { behavior: 'smooth' }),
      s('goTo', 1, { anchor: { y: 'end' } }),
      s('goTo', 2, { anchor: { $undefined: true } }),
      s('goTo', 2, { anchor: { y: 0.5 }, behavior: { $undefined: true } }),
      s('goTo', 1, {}),
    ],
  },
  {
    name: 'reveal-config-anchor-null',
    covers: 'A config anchor of null replaces the default: the reveal is positioned (anchor present) with neither axis set.',
    plugins: withStage({ reveal: { anchor: null as never } }),
    engine: { documents: { a: doc([answer(Q, 'full', { startPage: 11 }, slice([m(11, 0), m(33, 5, [])], null, 3))]) } },
    snapshot: STAGE_SNAPSHOT,
    steps: [start, open(), st('setViewport', { width: 800, height: 600 }), s('search', Q), s('next'), s('next')],
  },
];

// ── the rerun effect ──────────────────────────────────────────────────────────

const effectCases: ScenarioCase[] = [
  {
    name: 'mutation-events-rerun-debounced',
    covers:
      'A document event while idle arms nothing; with a search stored, every event restarts a 250 ms timer, and one rerun fires after the burst goes quiet.',
    plugins: search(),
    engine: {
      documents: {
        a: doc([
          answer(Q, 'full', {}, slice([m(11, 0)], null, 3), 1),
          answer(Q, 'full', {}, slice([m(22, 0)], null, 3), 1),
        ]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [
      start,
      open(),
      emit({ type: 'annotations.created', page: pr(11) }),
      advance(300),
      s('search', Q),
      emit({ type: 'annotations.created', page: pr(11) }),
      advance(200),
      emit({ type: 'pages.changed' }),
      advance(249),
      advance(1),
      advance(500),
    ],
  },
  {
    name: 'rerun-after-clear-is-disarmed',
    covers: 'A timer armed while searching still fires after clear(); rerun() then finds the session idle and does nothing.',
    plugins: search(),
    engine: { documents: { a: doc([deferAt(Q, 'full', {}, 's1')]) } },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), emit({ type: 'metadata.updated' }), s('clear'), advance(250), emit({ type: 'metadata.updated' }), advance(250)],
  },
  {
    name: 'rerun-supersedes-a-running-scan',
    covers: 'A rerun during a scan aborts the in-flight slice as superseded and restarts.',
    plugins: search(),
    engine: {
      documents: { a: doc([deferAt(Q, 'full', {}, 's1', 1), answer(Q, 'full', {}, slice([m(33, 0)], null, 3))]) },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), emit({ type: 'annotations.updated' }), advance(250), s('status')],
  },
  {
    name: 'close-disarms-the-timer',
    covers: 'Closing the document clears an armed timer and unsubscribes from the event stream.',
    plugins: search(),
    engine: { documents: { a: doc([answer(Q, 'full', {}, slice([], null, 3))]) } },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('search', Q), emit({ type: 'annotations.created' }), close(), advance(500)],
  },
  {
    name: 'close-with-no-timer',
    covers: 'Closing with nothing armed only unsubscribes.',
    plugins: search(),
    engine: { documents: { a: doc([]) } },
    snapshot: 'none',
    steps: [start, open(), close()],
  },
];

// ── findAll ───────────────────────────────────────────────────────────────────

const findAllCases: ScenarioCase[] = [
  {
    name: 'find-all-walks-the-chain',
    covers:
      'findAll() walks the cursor chain in natural page order (no startPage, even with a Stage) and resolves every hit, dispatching nothing and leaving the session untouched.',
    plugins: withStage(),
    engine: { documents: { a: doc(CHAIN) } },
    snapshot: SNAPSHOT,
    steps: [start, open(), st('goToPage', 2), s('findAll', Q), s('status')],
  },
  {
    name: 'find-all-pinned-mode',
    covers: "An explicit mode is sent as is and never degrades: a pinned 'full' refused rejects with the refusal.",
    plugins: search(),
    engine: {
      documents: {
        a: doc([answer(Q, 'rects', {}, slice([m(11, 0)], null, 3)), failAt(Q2, 'full', {}, DENIED)]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('findAll', Q, { mode: 'rects' }), s('findAll', Q2, { mode: 'full' })],
  },
  {
    name: 'find-all-degrades',
    covers: "With no mode, findAll degrades 'full' to 'rects' on a refusal, as the session does.",
    plugins: search(),
    engine: { documents: { a: doc([failAt(Q, 'full', {}, FORBIDDEN), answer(Q, 'rects', {}, slice([m(22, 0)], null, 3))]) } },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('findAll', Q, {})],
  },
  {
    name: 'find-all-restart-resets',
    covers: 'A stale cursor restarts findAll once and drops what the first pass had collected.',
    plugins: search(),
    engine: {
      documents: {
        a: doc([
          answer(Q, 'full', {}, slice([m(11, 0)], 'c1', 1), 1),
          failAt(Q, 'full', { cursor: 'c1' }, STALE, 1),
          answer(Q, 'full', {}, slice([m(11, 4)], 'c2', 1), 1),
          answer(Q, 'full', { cursor: 'c2' }, slice([m(22, 0)], null, 3), 1),
        ]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('findAll', Q)],
  },
  {
    name: 'find-all-errors-reject',
    covers: 'An engine error rejects findAll with that error; so does a geometry failure while converting hits.',
    plugins: search(),
    engine: {
      documents: {
        a: doc([failAt(Q, 'full', {}, BOOM), answer(Q2, 'full', {}, slice([m(44, 0)], null, 1, 1))], {
          pages: [...PAGES, layout(44, 3, { left: 0, bottom: 0, right: 100, top: 100 }, 0, 0)],
        }),
      },
    },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('findAll', Q), s('findAll', Q2)],
  },
  {
    name: 'find-all-is-independent-of-the-session',
    covers: 'A findAll running beside the session neither supersedes it nor is superseded by it; both complete.',
    plugins: search(),
    engine: {
      documents: {
        a: doc([deferAt(Q, 'full', {}, 'session', 1), deferAt(Q, 'full', {}, 'find', 1), answer(Q2, 'full', {}, slice([], null, 3))]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [
      start,
      open(),
      s('search', Q),
      s('findAll', Q),
      s('search', Q2),
      settle('find', slice([m(33, 0)], null, 3)),
      settle('session', slice([m(11, 0)], null, 3)),
      s('findAll', Q2),
    ],
  },
  {
    name: 'find-all-signal-aborts-the-slice',
    covers:
      "Aborting findAll's signal aborts the in-flight slice with the signal's reason and rejects with it: the platform AbortError with no reason, the given error with one.",
    plugins: withSignals_(),
    engine: {
      documents: {
        a: doc([
          deferAt(Q, 'full', {}, 'f1', 1),
          answer(Q2, 'full', {}, slice([m(11, 0)], 'c1', 1)),
          deferAt(Q2, 'full', { cursor: 'c1' }, 'f2'),
        ]),
      },
    },
    snapshot: SNAPSHOT,
    steps: [
      start,
      open(),
      s('findAll', Q, { signal: signal('s1') }),
      abortSignal('s1'),
      s('findAll', Q2, { signal: signal('s3') }),
      abortSignal('s3', { $error: { name: 'Error', message: 'user left' } }),
      settle('f1', slice([m(22, 0)], null, 3)),
      settle('f2', slice([m(22, 0)], null, 3)),
    ],
  },
  {
    name: 'find-all-signal-already-aborted',
    covers: 'A signal aborted before the call still sends the first request; when it lands, findAll rejects with the reason.',
    plugins: withSignals_(),
    engine: { documents: { a: doc([answer(Q, 'full', {}, slice([m(11, 0)], 'c1', 1))]) } },
    snapshot: SNAPSHOT,
    steps: [start, open(), abortSignal('s1'), s('findAll', Q, { signal: signal('s1') })],
  },
  {
    name: 'find-all-signal-after-resolution',
    covers: 'Aborting after findAll resolved does nothing: the listener was removed and no slice is in flight.',
    plugins: withSignals_(),
    engine: { documents: { a: doc([answer(Q, 'full', {}, slice([m(11, 0)], null, 3))]) } },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('findAll', Q, { signal: signal('s1') }), abortSignal('s1')],
  },
  {
    name: 'find-all-signal-on-an-error',
    covers: 'A signal that never aborts leaves an engine error to reject findAll; aborting afterwards does nothing.',
    plugins: withSignals_(),
    engine: { documents: { a: doc([failAt(Q, 'full', {}, BOOM)]) } },
    snapshot: SNAPSHOT,
    steps: [start, open(), s('findAll', Q, { signal: signal('s1') }), abortSignal('s1')],
  },
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/search.json',
    describes: 'packages/plugin/search: the session, degradation and errors, hit geometry, the permission twin, navigation and the rerun effect.',
    cases: [...sessionCases, ...failureCases, ...geometryCases, ...twinCases, ...navigationCases, ...effectCases],
  },
  {
    kind: 'scenario',
    file: 'plugins/search-stage.json',
    describes: 'packages/plugin/search with a Stage: viewport-first scans and reveal-on-navigate.',
    cases: stageCases,
  },
  {
    kind: 'scenario',
    file: 'plugins/search-find-all.json',
    describes: 'packages/plugin/search: the session-free findAll service and its abort signal.',
    cases: findAllCases,
  },
];
