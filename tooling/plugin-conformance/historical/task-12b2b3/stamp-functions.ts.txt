/**
 * `packages/plugin/stamp/src/model.ts`, called directly: the pure,
 * serializable descriptor store. `changed` is `next !== state`, the branch
 * the Rust reducer answers with `None`. Replayed by
 * `crates/plugins/stamp/tests/conformance.rs`.
 */
import * as model from '../../../../packages/plugin/stamp/src/model';
import { initialStampState } from '../../../../packages/plugin/stamp/src/model';
import type { StampState } from '../../../../packages/plugin/stamp/src/model';
import type { StampAsset, StampLibrary } from '../../../../packages/plugin/stamp/src/contract';

import type { FunctionCase, Suite } from '../vectors';

/** The old `StampAction`: its `type` and fields. */
type StampAction = { type: string; [field: string]: unknown };

/** 4fc1a32af replaced `stampReducer` with one transition per action. */
function stampReducer(state: StampState, action: StampAction): StampState {
  const a = action as unknown as Record<string, never>;
  switch (action.type) {
    case 'LIBRARY_ADDED':
      return model.addLibrary(state, a.library);
    case 'LIBRARY_UPDATED':
      return model.setLibrary(state, a.library);
    case 'LIBRARY_REMOVED':
      return model.removeLibrary(state, a.libraryId);
    case 'ASSET_ADDED':
      return model.addAsset(state, a.asset);
    case 'ASSET_UPDATED':
      return model.setAsset(state, a.asset);
    case 'ASSET_REMOVED':
      return model.removeAsset(state, a.assetId);
    default:
      return state;
  }
}

const functions = {
  initialStampState,
  stampReducer: (state: StampState, action: StampAction) => {
    const next = stampReducer(state, action);
    return { changed: next !== state, state: next };
  },
};

const r = (name: string, state: StampState, action: unknown, covers?: string): FunctionCase => ({
  name,
  fn: 'stampReducer',
  args: [state, action],
  ...(covers === undefined ? {} : { covers }),
});

const pr = (pon: number) => ({ kind: 'objectNumber' as const, pageObjectNumber: pon });
const library = (id: string, assetIds: string[] = [], over: Partial<StampLibrary> = {}): StampLibrary => ({
  id,
  name: `Library ${id}`,
  kind: 'stamps',
  assetIds,
  ...over,
});
const asset = (id: string, libraryId: string, over: Partial<StampAsset> = {}): StampAsset => ({
  id,
  libraryId,
  kind: 'stamp',
  name: `Asset ${id}`,
  label: `Asset ${id}`,
  size: { width: 200, height: 100 },
  page: pr(100),
  ...over,
});

const EMPTY = initialStampState();
const ONE: StampState = {
  libraries: { L: library('L', ['a', 'b'], { categories: ['Team'] }) },
  libraryOrder: ['L'],
  assets: { a: asset('a', 'L'), b: asset('b', 'L', { kind: 'signature', subject: 'S', categories: ['x'], page: pr(101) }) },
};
const TWO: StampState = {
  libraries: { ...ONE.libraries, M: library('M', ['c'], { kind: 'signatures' }) },
  libraryOrder: ['L', 'M'],
  assets: { ...ONE.assets, c: asset('c', 'M', { page: pr(102) }) },
};
/** An asset whose library is gone, and a library listing an asset that is. */
const ORPHANS: StampState = {
  libraries: { M: library('M', ['gone', 'c']) },
  libraryOrder: ['M'],
  assets: { c: asset('c', 'M'), stray: asset('stray', 'L') },
};

const cases: FunctionCase[] = [
  { name: 'initial-state', fn: 'initialStampState', args: [] },
  r('library-added-to-empty', EMPTY, { type: 'LIBRARY_ADDED', library: library('L') }),
  r('library-added-appends-order', ONE, { type: 'LIBRARY_ADDED', library: library('M', [], { kind: 'signatures' }) }),
  r(
    'library-added-with-a-taken-id',
    TWO,
    { type: 'LIBRARY_ADDED', library: library('L', [], { name: 'Replacement' }) },
    'A LIBRARY_ADDED for an id already present replaces the library and appends the id to the order again.',
  ),
  r('library-removed', TWO, { type: 'LIBRARY_REMOVED', libraryId: 'L' }, 'Removing a library drops it, its order entries and its listed assets.'),
  r('library-removed-unknown', TWO, { type: 'LIBRARY_REMOVED', libraryId: 'nope' }, 'An unknown library id returns the same state.'),
  r(
    'library-removed-with-missing-assets',
    ORPHANS,
    { type: 'LIBRARY_REMOVED', libraryId: 'M' },
    "Deleting a listed asset that is already gone is harmless; an asset of another library survives.",
  ),
  r(
    'library-removed-duplicate-order',
    { ...TWO, libraryOrder: ['L', 'M', 'L'] },
    { type: 'LIBRARY_REMOVED', libraryId: 'L' },
    'Every order entry of the id goes.',
  ),
  r('asset-added', TWO, { type: 'ASSET_ADDED', asset: asset('d', 'M', { label: 'JPEG' }) }),
  r('asset-added-to-unknown-library', TWO, { type: 'ASSET_ADDED', asset: asset('d', 'nope') }, 'An asset for an unknown library returns the same state.'),
  r(
    'asset-added-twice',
    ONE,
    { type: 'ASSET_ADDED', asset: asset('a', 'L', { name: 'Again' }) },
    'An asset id already present is replaced and listed a second time.',
  ),
  r('asset-removed', TWO, { type: 'ASSET_REMOVED', assetId: 'a' }),
  r('asset-removed-unknown', TWO, { type: 'ASSET_REMOVED', assetId: 'nope' }, 'An unknown asset id returns the same state.'),
  r(
    'asset-removed-without-its-library',
    ORPHANS,
    { type: 'ASSET_REMOVED', assetId: 'stray' },
    'An asset whose library is gone is deleted and the libraries are left as they are.',
  ),
  r('library-updated', TWO, { type: 'LIBRARY_UPDATED', library: library('M', ['c'], { name: 'Renamed', kind: 'signatures' }) }),
  r('library-updated-unknown', TWO, { type: 'LIBRARY_UPDATED', library: library('nope') }, 'An update for an unknown library returns the same state.'),
  r('asset-updated', TWO, { type: 'ASSET_UPDATED', asset: asset('c', 'M', { label: 'Relabelled', page: pr(102) }) }),
  r('asset-updated-unknown', TWO, { type: 'ASSET_UPDATED', asset: asset('nope', 'M') }, 'An update for an unknown asset returns the same state.'),
  r('unknown-action', TWO, { type: 'OTHER' }, 'An action outside the union returns the same state.'),
];

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'plugins/stamp-functions.json',
    describes: 'packages/plugin/stamp reducer, called directly.',
    functions: functions as unknown as Readonly<Record<string, (...args: never[]) => unknown>>,
    cases,
  },
];
