/**
 * `packages/plugin/search/src/model.ts`, called directly. Replayed by
 * `crates/plugins/search/tests/conformance.rs`.
 *
 * `reduceSearch` reports `{changed, state}`: `changed` is `next !== state`,
 * the identity the store's change notification keys on.
 */
import * as model from '../../../../packages/plugin/search/src/model';
import { initialSearchState } from '../../../../packages/plugin/search/src/model';
import type { SearchState } from '../../../../packages/plugin/search/src/model';
import type { SearchHit } from '../../../../packages/plugin/search/src/contract';

import type { FunctionCase, Suite } from '../vectors';

const observe =
  <S, A extends unknown[]>(transition: (state: S, ...args: A) => S) =>
  (state: S, ...args: A) => {
    const next = transition(state, ...args);
    return { changed: next !== state, state: next };
  };

const functions = {
  initialSearchState,
  startSession: observe(model.startSession),
  appendHits: observe(model.appendHits),
  completeSession: observe(model.completeSession),
  cancelSession: observe(model.cancelSession),
  failSession: observe(model.failSession),
  setActiveHit: observe(model.setActiveHit),
  clearSession: observe(model.clearSession),
};

const rect = (x: number, y: number, width: number, height: number) => ({ x, y, width, height });
const seg = (r: { x: number; y: number; width: number; height: number }) => ({
  quad: {
    upperLeft: { x: r.x, y: r.y },
    upperRight: { x: r.x + r.width, y: r.y },
    lowerLeft: { x: r.x, y: r.y + r.height },
    lowerRight: { x: r.x + r.width, y: r.y + r.height },
  },
  rect: r,
  advance: 1 as const,
});
const hit = (pon: number, charStart: number, over: Partial<SearchHit> = {}): SearchHit => ({
  page: { kind: 'objectNumber', objectNumber: pon },
  pageIndex: 0,
  start: charStart,
  count: 4,
  segments: [seg(rect(0, 0, 10, 10))],
  bounds: rect(0, 0, 10, 10),
  ...over,
});

const failure = (message: string) => ({
  code: 'operation-failed' as const,
  message,
  capability: 'search',
  permission: null,
});
const started: SearchState = {
  ...initialSearchState(),
  query: { text: 'test' },
  status: 'searching',
};
const withHits: SearchState = {
  ...started,
  hits: [hit(5, 0), hit(5, 9), hit(7, 2)],
  hitsByPage: { 5: [hit(5, 0), hit(5, 9)], 7: [hit(7, 2)] },
  activeHitIndex: 0,
  progress: { pagesSearched: 2, pageCount: 8 },
};
const done: SearchState = { ...withHits, status: 'complete', activeHitIndex: 2 };
const failed: SearchState = { ...withHits, status: 'error', error: failure('boom') };

const c = <K extends keyof typeof functions>(
  name: string,
  fn: K,
  ...args: Parameters<(typeof functions)[K]>
): FunctionCase => ({ name, fn, args });

const cases: FunctionCase[] = [
  { name: 'initial-state', fn: 'initialSearchState', args: [] },
  c(
    'start-from-idle',
    'startSession',
    initialSearchState(),
    { text: 'x', matchCase: true },
    'search#2',
  ),
  c(
    'start-resets-a-finished-search',
    'startSession',
    done,
    { text: 'again', regex: true, wholeWord: true },
    'search#2',
  ),
  c('start-resets-an-error', 'startSession', failed, { text: 'y' }, 'search#2'),
  c('append-first-hits', 'appendHits', started, [hit(5, 0), hit(5, 9)], {
    pagesSearched: 1,
    pageCount: 8,
  }),
  c('append-more-hits', 'appendHits', withHits, [hit(7, 11), hit(9, 0), hit(5, 20)], {
    pagesSearched: 5,
    pageCount: 8,
  }),
  c(
    'append-keeps-an-explicit-index',
    'appendHits',
    { ...withHits, activeHitIndex: 2 },
    [hit(9, 1)],
    { pagesSearched: 6, pageCount: 8 },
  ),
  c('append-empty-advances-progress', 'appendHits', withHits, [], {
    pagesSearched: 7,
    pageCount: 8,
  }),
  c('append-empty-before-any-hit', 'appendHits', started, [], { pagesSearched: 4, pageCount: 8 }),
  c(
    'append-to-idle',
    'appendHits',
    initialSearchState(),
    [
      hit(3, 0, {
        segments: [],
        bounds: undefined,
        snippet: { before: 'a ', match: 'b', after: '' },
      }),
    ],
    { pagesSearched: 1, pageCount: 1 },
  ),
  c('complete', 'completeSession', withHits),
  c('complete-when-idle', 'completeSession', initialSearchState()),
  c('error', 'failSession', withHits, failure('boom')),
  c('error-replaces-an-error', 'failSession', failed, failure('again')),
  c('set-active', 'setActiveHit', withHits, 2),
  c('set-active-unchecked', 'setActiveHit', withHits, 7),
  c('set-active-negative', 'setActiveHit', done, -1),
  c('set-active-same-index', 'setActiveHit', withHits, 0),
  c('cancelled', 'cancelSession', withHits),
  c('clear', 'clearSession', failed),
  c('clear-when-idle', 'clearSession', initialSearchState()),
];

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'plugins/search-reducer.json',
    describes: 'packages/plugin/search: initialSearchState and named session transitions.',
    functions,
    cases,
  },
];
