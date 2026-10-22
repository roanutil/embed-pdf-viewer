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

/** The old `SearchAction`: its `type` and fields. */
type SearchAction = { type: string; [field: string]: unknown };

/**
 * 4fc1a32af replaced `reduceSearch` with one transition per action. An
 * `APPEND`'s `scanned` and `total` are the slice's progress, which upstream
 * names `pagesSearched` and `pageCount`.
 */
function reduceSearch(state: SearchState, action: SearchAction): SearchState {
  const a = action as Record<string, never>;
  switch (action.type) {
    case 'START':
      return model.startSession(state, a.query, a.operationId);
    case 'APPEND':
      return model.appendHits(state, a.hits, { pagesSearched: a.scanned, pageCount: a.total });
    case 'COMPLETE':
      return model.completeSession(state);
    case 'CANCELLED':
      return model.cancelSession(state);
    case 'ERROR':
      return model.failSession(state, a.error);
    case 'SET_ACTIVE':
      return model.setActiveHit(state, a.index);
    case 'CLEAR':
      return model.clearSession(state);
    default:
      return state;
  }
}

const functions = {
  initialSearchState,
  reduceSearch: (state: SearchState, action: SearchAction) => {
    const next = reduceSearch(state, action);
    return { changed: next !== state, state: next };
  },
};

const rect = (x: number, y: number, width: number, height: number) => ({ x, y, width, height });
const seg = (r: { x: number; y: number; width: number; height: number }) => ({
  quad: {
    upperStart: { x: r.x, y: r.y },
    upperEnd: { x: r.x + r.width, y: r.y },
    lowerStart: { x: r.x, y: r.y + r.height },
    lowerEnd: { x: r.x + r.width, y: r.y + r.height },
  },
  rect: r,
  advance: 1 as const,
});
const hit = (pon: number, charStart: number, over: Partial<SearchHit> = {}): SearchHit => ({
  page: { kind: 'objectNumber', pageObjectNumber: pon },
  pageIndex: 0,
  charStart,
  charCount: 4,
  segments: [seg(rect(0, 0, 10, 10))],
  bounds: rect(0, 0, 10, 10),
  ...over,
});

const failure = (message: string) => ({ code: 'operation-failed' as const, message, capability: 'search' });
const started: SearchState = { ...initialSearchState(), query: { text: 'test' }, status: 'searching' };
const withHits: SearchState = {
  ...started,
  hits: [hit(5, 0), hit(5, 9), hit(7, 2)],
  hitsByPage: { 5: [hit(5, 0), hit(5, 9)], 7: [hit(7, 2)] },
  activeIndex: 0,
  progress: { scanned: 2, total: 8 },
};
const done: SearchState = { ...withHits, status: 'complete', activeIndex: 2 };
const failed: SearchState = { ...withHits, status: 'error', error: failure('boom') };

const c = (name: string, state: SearchState, action: unknown, covers?: string): FunctionCase => ({
  name,
  fn: 'reduceSearch',
  args: [state, action],
  ...(covers === undefined ? {} : { covers }),
});

const cases: FunctionCase[] = [
  { name: 'initial-state', fn: 'initialSearchState', args: [] },
  c('start-from-idle', initialSearchState(), { type: 'START', query: { text: 'x', matchCase: true }, operationId: 'search#2' }),
  c('start-resets-a-finished-search', done, { type: 'START', query: { text: 'again', regex: true, wholeWord: true }, operationId: 'search#2' }, 'START drops hits, index, progress and error and keeps only the new query.'),
  c('start-resets-an-error', failed, { type: 'START', query: { text: 'y' }, operationId: 'search#2' }),
  c('append-first-hits', started, { type: 'APPEND', hits: [hit(5, 0), hit(5, 9)], scanned: 1, total: 8 }, 'The first hits activate index 0 and bucket by page object number.'),
  c('append-more-hits', withHits, { type: 'APPEND', hits: [hit(7, 11), hit(9, 0), hit(5, 20)], scanned: 5, total: 8 }, 'Later hits extend existing page buckets and add new ones; the active index is kept.'),
  c('append-keeps-an-explicit-index', { ...withHits, activeIndex: 2 }, { type: 'APPEND', hits: [hit(9, 1)], scanned: 6, total: 8 }),
  c('append-empty-advances-progress', withHits, { type: 'APPEND', hits: [], scanned: 7, total: 8 }, 'An empty APPEND is a new object with only progress changed.'),
  c('append-empty-before-any-hit', started, { type: 'APPEND', hits: [], scanned: 4, total: 8 }),
  c('append-to-idle', initialSearchState(), { type: 'APPEND', hits: [hit(3, 0, { segments: [], bounds: undefined, snippet: { text: 'a b', matchStart: 2, matchLength: 1 } })], scanned: 1, total: 1 }),
  c('complete', withHits, { type: 'COMPLETE' }),
  c('complete-when-idle', initialSearchState(), { type: 'COMPLETE' }),
  c('error', withHits, { type: 'ERROR', error: failure('boom') }),
  c('error-replaces-an-error', failed, { type: 'ERROR', error: failure('again') }),
  c('set-active', withHits, { type: 'SET_ACTIVE', index: 2 }),
  c('set-active-unchecked', withHits, { type: 'SET_ACTIVE', index: 7 }, 'The reducer stores the index it is given; wrapping is the capability\'s job.'),
  c('set-active-negative', done, { type: 'SET_ACTIVE', index: -1 }),
  c('set-active-same-index', withHits, { type: 'SET_ACTIVE', index: 0 }, 'The index already active returns the same state, so the store does not notify.'),
  c('cancelled', withHits, { type: 'CANCELLED' }, 'CANCELLED keeps the hits, the index and the progress.'),
  c('clear', failed, { type: 'CLEAR' }),
  c('clear-when-idle', initialSearchState(), { type: 'CLEAR' }),
  c('unknown-action', withHits, { type: 'OTHER' }, 'An action outside the union returns the same state.'),
];

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'plugins/search-reducer.json',
    describes: 'packages/plugin/search: initialSearchState and reduceSearch.',
    functions,
    cases,
  },
];
