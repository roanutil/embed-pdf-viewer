/**
 * `packages/plugin/search/src/reducer.ts`, called directly. Replayed by
 * `crates/plugins/search/tests/conformance.rs`.
 *
 * `searchReducer` reports `{changed, state}`: `changed` is `next !== state`,
 * the identity the store's change notification keys on.
 */
import { initialSearchState, searchReducer } from '../../../../packages/plugin/search/src/reducer';
import type { SearchAction, SearchHit, SearchState } from '../../../../packages/plugin/search/src/types';

import type { FunctionCase, Suite } from '../vectors';

const functions = {
  initialSearchState,
  searchReducer: (state: SearchState, action: SearchAction) => {
    const next = searchReducer(state, action);
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
  pon,
  pageIndex: 0,
  charStart,
  charCount: 4,
  segments: [seg(rect(0, 0, 10, 10))],
  bounds: rect(0, 0, 10, 10),
  ...over,
});

const started: SearchState = { ...initialSearchState(), query: { text: 'test' }, status: 'searching' };
const withHits: SearchState = {
  ...started,
  hits: [hit(5, 0), hit(5, 9), hit(7, 2)],
  hitsByPage: { 5: [0, 1], 7: [2] },
  activeIndex: 0,
  progress: { scanned: 2, total: 8 },
};
const done: SearchState = { ...withHits, status: 'complete', activeIndex: 2 };
const failed: SearchState = { ...withHits, status: 'error', error: 'boom' };

const c = (name: string, state: SearchState, action: unknown, covers?: string): FunctionCase => ({
  name,
  fn: 'searchReducer',
  args: [state, action],
  ...(covers === undefined ? {} : { covers }),
});

const cases: FunctionCase[] = [
  { name: 'initial-state', fn: 'initialSearchState', args: [] },
  c('start-from-idle', initialSearchState(), { type: 'START', query: { text: 'x', matchCase: true } }),
  c('start-resets-a-finished-search', done, { type: 'START', query: { text: 'again', regex: true, wholeWord: true } }, 'START drops hits, index, progress and error and keeps only the new query.'),
  c('start-resets-an-error', failed, { type: 'START', query: { text: 'y' } }),
  c('append-first-hits', started, { type: 'APPEND', hits: [hit(5, 0), hit(5, 9)], scanned: 1, total: 8 }, 'The first hits activate index 0 and index by pon.'),
  c('append-more-hits', withHits, { type: 'APPEND', hits: [hit(7, 11), hit(9, 0), hit(5, 20)], scanned: 5, total: 8 }, 'Later hits extend existing page buckets and add new ones; the active index is kept.'),
  c('append-keeps-an-explicit-index', { ...withHits, activeIndex: 2 }, { type: 'APPEND', hits: [hit(9, 1)], scanned: 6, total: 8 }),
  c('append-empty-advances-progress', withHits, { type: 'APPEND', hits: [], scanned: 7, total: 8 }, 'An empty APPEND is a new object with only progress changed.'),
  c('append-empty-before-any-hit', started, { type: 'APPEND', hits: [], scanned: 4, total: 8 }),
  c('append-to-idle', initialSearchState(), { type: 'APPEND', hits: [hit(3, 0, { segments: [], bounds: undefined, snippet: { text: 'a b', matchStart: 2, matchLength: 1 } })], scanned: 1, total: 1 }),
  c('complete', withHits, { type: 'COMPLETE' }),
  c('complete-when-idle', initialSearchState(), { type: 'COMPLETE' }),
  c('error', withHits, { type: 'ERROR', message: 'boom' }),
  c('error-replaces-an-error', failed, { type: 'ERROR', message: 'again' }),
  c('set-active', withHits, { type: 'SET_ACTIVE', index: 2 }),
  c('set-active-unchecked', withHits, { type: 'SET_ACTIVE', index: 7 }, 'The reducer stores the index it is given; wrapping is the capability\'s job.'),
  c('set-active-negative', done, { type: 'SET_ACTIVE', index: -1 }),
  c('clear', failed, { type: 'CLEAR' }),
  c('clear-when-idle', initialSearchState(), { type: 'CLEAR' }),
  c('unknown-action', withHits, { type: 'OTHER' }, 'An action outside the union returns the same state.'),
];

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'plugins/search-reducer.json',
    describes: 'packages/plugin/search: initialSearchState and searchReducer.',
    functions,
    cases,
  },
];
