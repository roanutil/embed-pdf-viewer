/**
 * `packages/plugin/stamp/src/model.ts`, called directly: the pure,
 * serializable descriptor store, including the per-document armed map. Replayed by
 * `crates/plugins/stamp/tests/conformance.rs`.
 */
import * as model from '../../../../packages/plugin/stamp/src/model';
import { initialStampState } from '../../../../packages/plugin/stamp/src/model';
import type { StampState } from '../../../../packages/plugin/stamp/src/model';
import type { StampAsset, StampLibrary } from '../../../../packages/plugin/stamp/src/contract';

import type { FunctionCase, Suite } from '../vectors';

const functions = {
  initialStampState,
  addLibrary: model.addLibrary,
  setLibrary: model.setLibrary,
  removeLibrary: model.removeLibrary,
  addAsset: model.addAsset,
  setAsset: model.setAsset,
  removeAsset: model.removeAsset,
  armAsset: model.armAsset,
  disarmDocuments: model.disarmDocuments,
};

const r = <Name extends keyof typeof functions>(
  name: string,
  fn: Name,
  args: Parameters<(typeof functions)[Name]>,
  covers?: string,
): FunctionCase => ({ name, fn, args, ...(covers === undefined ? {} : { covers }) });

const pr = (pon: number) => ({ kind: 'objectNumber' as const, objectNumber: pon });
const library = (
  id: string,
  assetIds: string[] = [],
  over: Partial<StampLibrary> = {},
): StampLibrary => ({
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
  armed: {},
  libraries: { L: library('L', ['a', 'b'], { categories: ['Team'] }) },
  libraryOrder: ['L'],
  assets: {
    a: asset('a', 'L'),
    b: asset('b', 'L', { kind: 'signature', subject: 'S', categories: ['x'], page: pr(101) }),
  },
};
const TWO: StampState = {
  armed: {},
  libraries: { ...ONE.libraries, M: library('M', ['c'], { kind: 'signatures' }) },
  libraryOrder: ['L', 'M'],
  assets: { ...ONE.assets, c: asset('c', 'M', { page: pr(102) }) },
};
/** An asset whose library is gone, and a library listing an asset that is. */
const ORPHANS: StampState = {
  armed: {},
  libraries: { M: library('M', ['gone', 'c']) },
  libraryOrder: ['M'],
  assets: { c: asset('c', 'M'), stray: asset('stray', 'L') },
};

const cases: FunctionCase[] = [
  { name: 'initial-state', fn: 'initialStampState', args: [] },
  r('library-added-to-empty', 'addLibrary', [EMPTY, library('L')]),
  r('library-added-appends-order', 'addLibrary', [ONE, library('M', [], { kind: 'signatures' })]),
  r(
    'library-added-with-a-taken-id',
    'addLibrary',
    [TWO, library('L', [], { name: 'Replacement' })],
    'Adding a library whose id is already present replaces the library and appends the id to the order again.',
  ),
  r(
    'library-removed',
    'removeLibrary',
    [TWO, 'L'],
    'Removing a library drops it, its order entries and its listed assets.',
  ),
  r(
    'library-removed-unknown',
    'removeLibrary',
    [TWO, 'nope'],
    'An unknown library id returns the same state.',
  ),
  r(
    'library-removed-with-missing-assets',
    'removeLibrary',
    [ORPHANS, 'M'],
    'Deleting a listed asset that is already gone is harmless; an asset of another library survives.',
  ),
  r(
    'library-removed-duplicate-order',
    'removeLibrary',
    [{ ...TWO, libraryOrder: ['L', 'M', 'L'] }, 'L'],
    'Every order entry of the id goes.',
  ),
  r('asset-added', 'addAsset', [TWO, asset('d', 'M', { label: 'JPEG' })]),
  r(
    'asset-added-to-unknown-library',
    'addAsset',
    [TWO, asset('d', 'nope')],
    'An asset for an unknown library returns the same state.',
  ),
  r(
    'asset-added-twice',
    'addAsset',
    [ONE, asset('a', 'L', { name: 'Again' })],
    'An asset id already present is replaced and listed a second time.',
  ),
  r('asset-removed', 'removeAsset', [TWO, 'a']),
  r(
    'asset-removed-unknown',
    'removeAsset',
    [TWO, 'nope'],
    'An unknown asset id returns the same state.',
  ),
  r(
    'asset-removed-without-its-library',
    'removeAsset',
    [ORPHANS, 'stray'],
    'An asset whose library is gone is deleted and the libraries are left as they are.',
  ),
  r('library-updated', 'setLibrary', [
    TWO,
    library('M', ['c'], { name: 'Renamed', kind: 'signatures' }),
  ]),
  r(
    'library-updated-unknown',
    'setLibrary',
    [TWO, library('nope')],
    'An update for an unknown library returns the same state.',
  ),
  r('asset-updated', 'setAsset', [TWO, asset('c', 'M', { label: 'Relabelled', page: pr(102) })]),
  r(
    'asset-updated-unknown',
    'setAsset',
    [TWO, asset('nope', 'M')],
    'An update for an unknown asset returns the same state.',
  ),
  r('already-armed-is-stable', 'armAsset', [{ ...TWO, armed: { a: 'c' } }, 'a', 'c']),
  r('disarm-documents', 'disarmDocuments', [{ ...TWO, armed: { a: 'a', b: 'c' } }, ['a']]),
];

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'plugins/stamp-functions.json',
    describes: 'packages/plugin/stamp named session transitions, called directly.',
    functions,
    cases,
  },
];
