/**
 * `packages/plugin/view-manager`. Replayed by
 * `crates/plugins/view-manager/tests/conformance.rs`.
 *
 * The reducer as function cases (with its reference identity made visible),
 * and the plugin as scenarios: the capability, and the effect that
 * reconciles panes with the kernel's open-document order.
 */
import { viewManagerPlugin, ViewManagerToken } from '../../../../packages/plugin/view-manager/src/index';
import { initialViewManagerState, viewManagerReducer } from '../../../../packages/plugin/view-manager/src/reducer';
import type { ViewManagerAction, ViewManagerState } from '../../../../packages/plugin/view-manager/src/types';

import type { FakeDocumentSpec } from '../fake-engine';
import { registerPlugin, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';

registerPlugin('view-manager', { factory: () => viewManagerPlugin(), tokens: [ViewManagerToken] });

// ── function cases: the reducer ───────────────────────────────────────────────

const reduce = (state: ViewManagerState, action: ViewManagerAction) => {
  const next = viewManagerReducer(state, action);
  return { same: next === state, next };
};
const initial = () => initialViewManagerState;

const view = (id: string, documentIds: string[], activeDocumentId: string | null = documentIds[0] ?? null) => ({ id, documentIds, activeDocumentId });
const st = (views: ReturnType<typeof view>[], over: Partial<ViewManagerState> = {}): ViewManagerState => ({
  views: Object.fromEntries(views.map((v) => [v.id, v])),
  order: views.map((v) => v.id),
  focusedViewId: views[0]?.id ?? null,
  seq: views.length,
  ...over,
});
const panes = st([view('view-1', ['a', 'b', 'c'], 'b'), view('view-2', ['d'], 'd'), view('view-3', [], null)], { focusedViewId: 'view-2' });
const nan = { $number: 'NaN' };

const r = (name: string, action: unknown, state: ViewManagerState = panes): FunctionCase => ({ name, fn: 'viewManagerReducer', args: [state, action] });

const reducerCases: FunctionCase[] = [
  { name: 'initial-state', fn: 'initialViewManagerState', args: [] },
  r('create', { type: 'CREATE', view: view('view-4', [], null) }),
  r('create-on-empty', { type: 'CREATE', view: view('view-1', [], null) }, st([])),
  r('remove-unknown-is-ignored', { type: 'REMOVE', id: 'nope' }),
  r('remove-focused-focuses-the-last', { type: 'REMOVE', id: 'view-2' }),
  r('remove-unfocused-keeps-focus', { type: 'REMOVE', id: 'view-1' }),
  r('remove-the-only-view', { type: 'REMOVE', id: 'view-1' }, st([view('view-1', ['a'])])),
  r('reorder', { type: 'REORDER_VIEWS', order: ['view-3', 'view-1', 'view-2'] }),
  r('set-focused', { type: 'SET_FOCUSED', id: 'view-3' }),
  r('set-focused-null', { type: 'SET_FOCUSED', id: null }),
  r('set-focused-same-is-a-new-state', { type: 'SET_FOCUSED', id: 'view-2' }),
  r('set-active', { type: 'SET_ACTIVE_DOC', viewId: 'view-1', documentId: 'c' }),
  r('set-active-null', { type: 'SET_ACTIVE_DOC', viewId: 'view-1', documentId: null }),
  r('set-active-unknown-view', { type: 'SET_ACTIVE_DOC', viewId: 'nope', documentId: 'a' }),
  r('set-active-foreign-document', { type: 'SET_ACTIVE_DOC', viewId: 'view-1', documentId: 'd' }),
  r('add-appends', { type: 'ADD_DOC', viewId: 'view-1', documentId: 'e' }),
  r('add-at-index', { type: 'ADD_DOC', viewId: 'view-1', documentId: 'e', index: 1 }),
  r('add-at-zero', { type: 'ADD_DOC', viewId: 'view-1', documentId: 'e', index: 0 }),
  r('add-negative-clamps', { type: 'ADD_DOC', viewId: 'view-1', documentId: 'e', index: -4 }),
  r('add-past-the-end-clamps', { type: 'ADD_DOC', viewId: 'view-1', documentId: 'e', index: 99 }),
  r('add-fractional-truncates', { type: 'ADD_DOC', viewId: 'view-1', documentId: 'e', index: 2.9 }),
  r('add-nan-lands-first', { type: 'ADD_DOC', viewId: 'view-1', documentId: 'e', index: nan }),
  r('add-infinity-appends', { type: 'ADD_DOC', viewId: 'view-1', documentId: 'e', index: { $number: 'Infinity' } }),
  r('add-negative-infinity-lands-first', { type: 'ADD_DOC', viewId: 'view-1', documentId: 'e', index: { $number: '-Infinity' } }),
  r('add-to-empty-activates', { type: 'ADD_DOC', viewId: 'view-3', documentId: 'e' }),
  r('add-present-is-ignored', { type: 'ADD_DOC', viewId: 'view-1', documentId: 'a' }),
  r('add-unknown-view-is-ignored', { type: 'ADD_DOC', viewId: 'nope', documentId: 'e' }),
  r('remove-doc-inactive', { type: 'REMOVE_DOC', viewId: 'view-1', documentId: 'a' }),
  r('remove-doc-active-picks-the-next', { type: 'REMOVE_DOC', viewId: 'view-1', documentId: 'b' }),
  r('remove-doc-active-last-picks-the-previous', { type: 'REMOVE_DOC', viewId: 'view-1', documentId: 'c' }, st([view('view-1', ['a', 'b', 'c'], 'c')])),
  r('remove-doc-only-leaves-null', { type: 'REMOVE_DOC', viewId: 'view-2', documentId: 'd' }),
  r('remove-doc-absent-is-a-new-state', { type: 'REMOVE_DOC', viewId: 'view-1', documentId: 'zzz' }),
  r('remove-doc-unknown-view', { type: 'REMOVE_DOC', viewId: 'nope', documentId: 'a' }),
  r('move-within', { type: 'MOVE_DOC_WITHIN', viewId: 'view-1', documentId: 'a', toIndex: 2 }),
  r('move-within-to-front', { type: 'MOVE_DOC_WITHIN', viewId: 'view-1', documentId: 'c', toIndex: 0 }),
  r('move-within-clamps', { type: 'MOVE_DOC_WITHIN', viewId: 'view-1', documentId: 'a', toIndex: 50 }),
  r('move-within-nan', { type: 'MOVE_DOC_WITHIN', viewId: 'view-1', documentId: 'c', toIndex: nan }),
  r('move-within-absent', { type: 'MOVE_DOC_WITHIN', viewId: 'view-1', documentId: 'd', toIndex: 0 }),
  r('move-within-unknown-view', { type: 'MOVE_DOC_WITHIN', viewId: 'nope', documentId: 'a', toIndex: 0 }),
  r('move-between', { type: 'MOVE_DOC_BETWEEN', fromViewId: 'view-1', toViewId: 'view-2', documentId: 'b' }),
  r('move-between-at-index', { type: 'MOVE_DOC_BETWEEN', fromViewId: 'view-1', toViewId: 'view-2', documentId: 'a', toIndex: 0 }),
  r('move-between-into-empty', { type: 'MOVE_DOC_BETWEEN', fromViewId: 'view-2', toViewId: 'view-3', documentId: 'd', toIndex: 5 }),
  r('move-between-same-view', { type: 'MOVE_DOC_BETWEEN', fromViewId: 'view-1', toViewId: 'view-1', documentId: 'a' }),
  r('move-between-same-view-at-index', { type: 'MOVE_DOC_BETWEEN', fromViewId: 'view-1', toViewId: 'view-1', documentId: 'c', toIndex: 0 }),
  r('move-between-unknown-from', { type: 'MOVE_DOC_BETWEEN', fromViewId: 'nope', toViewId: 'view-1', documentId: 'a' }),
  r('move-between-unknown-to', { type: 'MOVE_DOC_BETWEEN', fromViewId: 'view-1', toViewId: 'nope', documentId: 'a' }),
  r('move-between-absent-document', { type: 'MOVE_DOC_BETWEEN', fromViewId: 'view-1', toViewId: 'view-2', documentId: 'd' }),
  r('reconcile-prunes-closed', { type: 'RECONCILE', open: ['a', 'd'], preferViewId: 'view-2' }),
  r('reconcile-prunes-the-active', { type: 'RECONCILE', open: ['a', 'c', 'd'], preferViewId: null }),
  r('reconcile-assigns-to-preferred', { type: 'RECONCILE', open: ['a', 'b', 'c', 'd', 'x', 'y'], preferViewId: 'view-3' }),
  r('reconcile-assigns-to-focused', { type: 'RECONCILE', open: ['x', 'a', 'b', 'c', 'd'], preferViewId: 'nope' }),
  r('reconcile-assigns-to-first', { type: 'RECONCILE', open: ['a', 'b', 'c', 'd', 'x'], preferViewId: null }, { ...panes, focusedViewId: 'gone' }),
  r('reconcile-assigns-to-first-no-focus', { type: 'RECONCILE', open: ['x'], preferViewId: '' }, { ...panes, focusedViewId: null }),
  r('reconcile-creates-a-default-pane', { type: 'RECONCILE', open: ['a', 'b'], preferViewId: null }, st([])),
  r('reconcile-creates-and-keeps-a-stale-focus', { type: 'RECONCILE', open: ['a'], preferViewId: 'x' }, st([], { focusedViewId: 'x', seq: 4 })),
  r('reconcile-nothing-open', { type: 'RECONCILE', open: [], preferViewId: null }),
  r('reconcile-nothing-open-no-views', { type: 'RECONCILE', open: [], preferViewId: null }, st([])),
  r('reconcile-empty-active-is-replaced', { type: 'RECONCILE', open: ['a', 'b'], preferViewId: null }, st([view('view-1', ['a', 'b'], '')])),
  r('reconcile-duplicate-assignments', { type: 'RECONCILE', open: ['a', 'b'], preferViewId: null }, st([view('view-1', ['a']), view('view-2', ['a', 'b'], 'b')])),
  r('reconcile-keeps-other-members', { type: 'RECONCILE', open: ['a', 'b', 'c', 'd'], preferViewId: null }),
  r('unknown-action-is-ignored', { type: 'NOPE' }),
];

// ── scenarios ─────────────────────────────────────────────────────────────────

const box = { left: 0, bottom: 0, right: 600, top: 800 };
const pages = [{ index: 0, pageObjectNumber: 1, label: null, size: { width: 600, height: 800 }, rotation: 0, userUnit: 1, boxes: { media: box, crop: box } }];
const doc = (over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec => ({ pages, ...over });

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const open = (id: string, options?: unknown): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args: options === undefined ? [{ kind: 'bytes', id, bytes: { $bytes: '' } }] : [{ kind: 'bytes', id, bytes: { $bytes: '' } }, options],
});
const docs = (method: string, ...args: unknown[]): Step => ({ op: 'call', target: 'documents', method, args });
const vm = (method: string, ...args: unknown[]): Step => ({ op: 'call', target: { token: 'view-manager' }, method, args });
const reads: Step[] = [vm('list'), vm('order'), vm('focusedViewId')];

const plugins = [{ factory: 'view-manager' }];
const snapshot = ['view-manager'];

const cases: ScenarioCase[] = [
  {
    name: 'selectors-before-start',
    covers: 'The capability exists from createKernel(); with no panes the selectors are empty, get() and documentView() are null.',
    plugins,
    snapshot,
    steps: [...reads, vm('get', 'view-1'), vm('documentView', 'a')],
  },
  {
    name: 'first-open-makes-a-pane',
    covers:
      'The effect reconciles whenever the open order changes: the first open (while still loading) creates view-1 and assigns the document to it; later opens land in the focused pane; get/documentView/list read it back as copies.',
    plugins,
    snapshot,
    engine: { documents: { a: doc(), b: doc(), c: doc() }, responses: [{ path: 'open', args: [{ kind: 'bytes', id: 'c', bytes: { $bytes: '' } }, {}], defer: 'open-c' }] },
    steps: [
      start,
      ...reads,
      open('a'),
      ...reads,
      open('b'),
      vm('get', 'view-1'),
      vm('documentView', 'b'),
      vm('documentView', 'zzz'),
      open('c'),
      ...reads,
      { op: 'settle', label: 'open-c', value: { $document: 'c' } },
      ...reads,
    ],
  },
  {
    name: 'closing-prunes',
    covers: 'Closing a document drops it from its pane and picks a neighbour as active; closing every document leaves the empty pane.',
    plugins,
    snapshot,
    engine: { documents: { a: doc(), b: doc(), c: doc() } },
    steps: [
      start,
      open('a'),
      open('b'),
      open('c'),
      vm('setActiveDocument', 'view-1', 'b'),
      docs('close', 'b'),
      ...reads,
      docs('closeAll'),
      ...reads,
    ],
  },
  {
    name: 'panes',
    covers:
      'createView focuses the new pane and later opens land there; removeView refocuses; moveView reorders with clamping and ignores unknown ids; setFocused takes any id.',
    plugins,
    snapshot,
    engine: { documents: { a: doc(), b: doc(), c: doc() } },
    steps: [
      start,
      open('a'),
      vm('createView'),
      open('b'),
      vm('createView'),
      ...reads,
      vm('moveView', 'view-3', 0),
      vm('moveView', 'view-1', 99),
      vm('moveView', 'view-1', -3),
      vm('moveView', 'view-2', 1.5),
      vm('moveView', 'view-2', { $number: 'NaN' }),
      vm('moveView', 'nope', 0),
      vm('order'),
      vm('setFocused', 'view-1'),
      open('c'),
      vm('list'),
      vm('removeView', 'view-1'),
      ...reads,
      vm('removeView', 'view-1'),
      vm('setFocused', 'ghost'),
      vm('focusedViewId'),
      docs('close', 'c'),
      ...reads,
    ],
  },
  {
    name: 'tabs',
    covers:
      'addDocument/removeDocument/setActiveDocument/moveDocumentWithin/moveDocumentBetween dispatch their actions; the reducer ignores the invalid ones.',
    plugins,
    snapshot,
    engine: { documents: { a: doc(), b: doc(), c: doc() } },
    steps: [
      start,
      open('a'),
      open('b'),
      open('c'),
      vm('createView'),
      vm('moveDocumentBetween', 'view-1', 'view-2', 'b'),
      ...reads,
      vm('moveDocumentBetween', 'view-2', 'view-1', 'b', 0),
      vm('moveDocumentWithin', 'view-1', 'b', 2),
      vm('moveDocumentBetween', 'view-1', 'view-1', 'a'),
      vm('setActiveDocument', 'view-1', null),
      vm('setActiveDocument', 'view-1', 'zzz'),
      vm('addDocument', 'view-2', 'a'),
      vm('addDocument', 'view-2', 'c', 0),
      vm('addDocument', 'view-2', 'c'),
      vm('removeDocument', 'view-1', 'c'),
      vm('removeDocument', 'view-1', 'nope'),
      vm('documentView', 'a'),
      ...reads,
    ],
  },
  {
    name: 'reordering-tabs-reconciles',
    covers: 'documents.move changes the open order, so the effect reconciles (a no-op for pane contents); setActive alone does not.',
    plugins,
    snapshot,
    engine: { documents: { a: doc(), b: doc() } },
    steps: [start, open('a'), open('b'), docs('setActive', 'a'), docs('move', 'b', 0), ...reads],
  },
  {
    name: 'a-failed-open',
    covers: 'A document whose open fails keeps its tab in the order (status error), so it stays assigned until it is closed.',
    plugins,
    snapshot,
    engine: { documents: { a: doc() }, responses: [{ path: 'open', args: [{ kind: 'bytes', id: 'x', bytes: { $bytes: '' } }, {}], error: { name: 'EngineError', message: 'bad file', code: 'Format' } }] },
    steps: [start, open('a'), open('x'), ...reads, docs('close', 'x'), ...reads],
  },
  {
    name: 'destroy',
    covers: 'After destroy the effect is gone and the capability no longer resolves.',
    plugins,
    snapshot,
    engine: { documents: { a: doc() } },
    steps: [start, open('a'), { op: 'call', target: 'kernel', method: 'destroy' }, vm('list')],
  },
];

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'plugins/view-manager/reducer.json',
    describes: 'packages/plugin/view-manager: viewManagerReducer and initialViewManagerState (`same` is reference identity).',
    functions: { viewManagerReducer: reduce, initialViewManagerState: initial },
    cases: reducerCases,
  },
  { kind: 'scenario', file: 'plugins/view-manager/plugin.json', describes: 'packages/plugin/view-manager: panes over the open documents.', cases },
];
