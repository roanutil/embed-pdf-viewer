/**
 * `packages/plugin/view-manager`. Replayed by
 * `crates/plugins/view-manager/tests/conformance.rs`.
 *
 * The reducer as function cases (with its reference identity made visible),
 * and the plugin as scenarios: the capability, and the effect that
 * reconciles panes with the kernel's open-document order.
 */
import { viewManagerPlugin, ViewManagerToken } from '../../../../packages/plugin/view-manager/src/index';
import * as model from '../../../../packages/plugin/view-manager/src/model';
import type { ViewManagerState } from '../../../../packages/plugin/view-manager/src/model';

import type { FakeDocumentSpec } from '../fake-engine';
import { removedUpstream } from '../removed';
import { registerPlugin, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';

/** Why these cases record no state; see `ScenarioCase` in `../scenario`. */
const UNSNAPSHOTTED = {
  snapshot: 'none',
  reason: "Pins the pane events each write fires, which the event callbacks record. Its dispatched actions are compared; the state they build is not.",
} as const;

/**
 * The shipping controller spreads its context (`{ ...rawCtx, dispatch }`),
 * which reads the controller context's `doc` getter, and for a workspace
 * plugin that getter throws: `createKernel` with `viewManagerPlugin()`
 * refuses, whatever else is installed. This factory hides `doc` from the
 * spread, so the vectors describe the panes as the controller means them.
 * The workaround and the defect are recorded in multiplatform-plugins-status.md.
 */
const withoutDoc = <T extends object>(ctx: T): T =>
  new Proxy(ctx, {
    ownKeys: (target) => Reflect.ownKeys(target).filter((key) => key !== 'doc'),
    getOwnPropertyDescriptor: (target, key) =>
      key === 'doc' ? undefined : Reflect.getOwnPropertyDescriptor(target, key),
  });

registerPlugin('view-manager', {
  factory: () => {
    const plugin = viewManagerPlugin();
    return { ...plugin, create: (ctx) => plugin.create!(withoutDoc(ctx)) } as typeof plugin;
  },
  tokens: [ViewManagerToken],
});
/** The shipping plugin, unmodified: `a-kernel-refuses-the-shipping-plugin` pins the defect. */
registerPlugin('view-manager-as-shipped', { factory: () => viewManagerPlugin(), tokens: [ViewManagerToken] });

// ── function cases: the reducer ───────────────────────────────────────────────

/** The old `ViewManagerAction`: its `type` and fields. */
type ViewManagerAction = { type: string; [field: string]: unknown };

/**
 * 4fc1a32af replaced `viewManagerReducer` with one transition per action.
 * `CREATE_PANE` and `REORDER_PANES` have none that takes their fields:
 * `createPane` makes an empty pane with the next id, and `movePane` moves one
 * pane, where the actions carried a whole pane and a whole order.
 */
function viewManagerReducer(state: ViewManagerState, action: ViewManagerAction): ViewManagerState {
  const a = action as Record<string, never>;
  switch (action.type) {
    case 'CREATE_PANE':
      return removedUpstream("viewManagerReducer 'CREATE_PANE' (a given pane)", '4fc1a32af')();
    case 'REMOVE_PANE':
      return model.removePane(state, a.id);
    case 'REORDER_PANES':
      return removedUpstream("viewManagerReducer 'REORDER_PANES' (a whole order)", '4fc1a32af')();
    case 'SET_FOCUSED':
      return model.setFocusedPane(state, a.id);
    case 'SET_ACTIVE_DOC':
      return model.setActiveDocument(state, a.paneId, a.documentId);
    case 'ADD_DOC':
      return model.addDocument(state, a.paneId, a.documentId, a.index);
    case 'REMOVE_DOC':
      return model.removeDocument(state, a.paneId, a.documentId);
    case 'MOVE_DOC_WITHIN':
      return model.moveDocumentWithin(state, a.paneId, a.documentId, a.toIndex);
    case 'MOVE_DOC_BETWEEN':
      return model.moveDocumentBetween(state, a.fromPaneId, a.toPaneId, a.documentId, a.toIndex);
    case 'RECONCILE':
      return model.reconcile(state, a.open, a.preferPaneId);
    default:
      return state;
  }
}

const reduce = (state: ViewManagerState, action: ViewManagerAction) => {
  const next = viewManagerReducer(state, action);
  return { same: next === state, next };
};
const initial = () => model.initialViewManagerState();

const view = (id: string, documentIds: string[], activeDocumentId: string | null = documentIds[0] ?? null) => ({ id, documentIds, activeDocumentId });
const st = (views: ReturnType<typeof view>[], over: Partial<ViewManagerState> = {}): ViewManagerState => ({
  panes: Object.fromEntries(views.map((v) => [v.id, v])),
  order: views.map((v) => v.id),
  focusedPaneId: views[0]?.id ?? null,
  seq: views.length,
  ...over,
});
const panes = st([view('pane-1', ['a', 'b', 'c'], 'b'), view('pane-2', ['d'], 'd'), view('pane-3', [], null)], { focusedPaneId: 'pane-2' });
const nan = { $number: 'NaN' };

const r = (name: string, action: unknown, state: ViewManagerState = panes): FunctionCase => ({ name, fn: 'viewManagerReducer', args: [state, action] });

const reducerCases: FunctionCase[] = [
  { name: 'initial-state', fn: 'initialViewManagerState', args: [] },
  r('create', { type: 'CREATE_PANE', pane: view('pane-4', [], null) }),
  r('create-on-empty', { type: 'CREATE_PANE', pane: view('pane-1', [], null) }, st([])),
  r('remove-unknown-is-ignored', { type: 'REMOVE_PANE', id: 'nope' }),
  r('remove-focused-focuses-the-last', { type: 'REMOVE_PANE', id: 'pane-2' }),
  r('remove-unfocused-keeps-focus', { type: 'REMOVE_PANE', id: 'pane-1' }),
  r('remove-the-only-view', { type: 'REMOVE_PANE', id: 'pane-1' }, st([view('pane-1', ['a'])])),
  r('reorder', { type: 'REORDER_PANES', order: ['pane-3', 'pane-1', 'pane-2'] }),
  r('set-focused', { type: 'SET_FOCUSED', id: 'pane-3' }),
  r('set-focused-null', { type: 'SET_FOCUSED', id: null }),
  r('set-focused-same-is-a-new-state', { type: 'SET_FOCUSED', id: 'pane-2' }),
  r('set-active', { type: 'SET_ACTIVE_DOC', paneId: 'pane-1', documentId: 'c' }),
  r('set-active-null', { type: 'SET_ACTIVE_DOC', paneId: 'pane-1', documentId: null }),
  r('set-active-unknown-view', { type: 'SET_ACTIVE_DOC', paneId: 'nope', documentId: 'a' }),
  r('set-active-foreign-document', { type: 'SET_ACTIVE_DOC', paneId: 'pane-1', documentId: 'd' }),
  r('add-appends', { type: 'ADD_DOC', paneId: 'pane-1', documentId: 'e' }),
  r('add-at-index', { type: 'ADD_DOC', paneId: 'pane-1', documentId: 'e', index: 1 }),
  r('add-at-zero', { type: 'ADD_DOC', paneId: 'pane-1', documentId: 'e', index: 0 }),
  r('add-negative-clamps', { type: 'ADD_DOC', paneId: 'pane-1', documentId: 'e', index: -4 }),
  r('add-past-the-end-clamps', { type: 'ADD_DOC', paneId: 'pane-1', documentId: 'e', index: 99 }),
  r('add-fractional-truncates', { type: 'ADD_DOC', paneId: 'pane-1', documentId: 'e', index: 2.9 }),
  r('add-nan-lands-first', { type: 'ADD_DOC', paneId: 'pane-1', documentId: 'e', index: nan }),
  r('add-infinity-appends', { type: 'ADD_DOC', paneId: 'pane-1', documentId: 'e', index: { $number: 'Infinity' } }),
  r('add-negative-infinity-lands-first', { type: 'ADD_DOC', paneId: 'pane-1', documentId: 'e', index: { $number: '-Infinity' } }),
  r('add-to-empty-activates', { type: 'ADD_DOC', paneId: 'pane-3', documentId: 'e' }),
  r('add-present-is-ignored', { type: 'ADD_DOC', paneId: 'pane-1', documentId: 'a' }),
  r('add-unknown-view-is-ignored', { type: 'ADD_DOC', paneId: 'nope', documentId: 'e' }),
  r('add-moves-from-its-holder', { type: 'ADD_DOC', paneId: 'pane-1', documentId: 'd', index: 0 }),
  r('add-moves-into-an-empty-pane', { type: 'ADD_DOC', paneId: 'pane-3', documentId: 'b' }),
  // Two holders (a state the reducer never builds): `find` walks `Object.values`, which puts
  // integer-like keys first, so pane '0' is the holder, not '1'.
  r('add-takes-the-first-holder-in-property-order', { type: 'ADD_DOC', paneId: 'b', documentId: 'x' }, st([view('0', ['x', 'y']), view('1', ['x']), view('b', [])])),
  r('remove-doc-inactive', { type: 'REMOVE_DOC', paneId: 'pane-1', documentId: 'a' }),
  r('remove-doc-active-picks-the-next', { type: 'REMOVE_DOC', paneId: 'pane-1', documentId: 'b' }),
  r('remove-doc-active-last-picks-the-previous', { type: 'REMOVE_DOC', paneId: 'pane-1', documentId: 'c' }, st([view('pane-1', ['a', 'b', 'c'], 'c')])),
  r('remove-doc-only-leaves-null', { type: 'REMOVE_DOC', paneId: 'pane-2', documentId: 'd' }),
  r('remove-doc-absent-is-a-new-state', { type: 'REMOVE_DOC', paneId: 'pane-1', documentId: 'zzz' }),
  r('remove-doc-unknown-view', { type: 'REMOVE_DOC', paneId: 'nope', documentId: 'a' }),
  r('move-within', { type: 'MOVE_DOC_WITHIN', paneId: 'pane-1', documentId: 'a', toIndex: 2 }),
  r('move-within-to-front', { type: 'MOVE_DOC_WITHIN', paneId: 'pane-1', documentId: 'c', toIndex: 0 }),
  r('move-within-clamps', { type: 'MOVE_DOC_WITHIN', paneId: 'pane-1', documentId: 'a', toIndex: 50 }),
  r('move-within-nan', { type: 'MOVE_DOC_WITHIN', paneId: 'pane-1', documentId: 'c', toIndex: nan }),
  r('move-within-absent', { type: 'MOVE_DOC_WITHIN', paneId: 'pane-1', documentId: 'd', toIndex: 0 }),
  r('move-within-unknown-view', { type: 'MOVE_DOC_WITHIN', paneId: 'nope', documentId: 'a', toIndex: 0 }),
  r('move-between', { type: 'MOVE_DOC_BETWEEN', fromPaneId: 'pane-1', toPaneId: 'pane-2', documentId: 'b' }),
  r('move-between-at-index', { type: 'MOVE_DOC_BETWEEN', fromPaneId: 'pane-1', toPaneId: 'pane-2', documentId: 'a', toIndex: 0 }),
  r('move-between-into-empty', { type: 'MOVE_DOC_BETWEEN', fromPaneId: 'pane-2', toPaneId: 'pane-3', documentId: 'd', toIndex: 5 }),
  r('move-between-same-view', { type: 'MOVE_DOC_BETWEEN', fromPaneId: 'pane-1', toPaneId: 'pane-1', documentId: 'a' }),
  r('move-between-same-view-at-index', { type: 'MOVE_DOC_BETWEEN', fromPaneId: 'pane-1', toPaneId: 'pane-1', documentId: 'c', toIndex: 0 }),
  r('move-between-unknown-from', { type: 'MOVE_DOC_BETWEEN', fromPaneId: 'nope', toPaneId: 'pane-1', documentId: 'a' }),
  r('move-between-unknown-to', { type: 'MOVE_DOC_BETWEEN', fromPaneId: 'pane-1', toPaneId: 'nope', documentId: 'a' }),
  r('move-between-absent-document', { type: 'MOVE_DOC_BETWEEN', fromPaneId: 'pane-1', toPaneId: 'pane-2', documentId: 'd' }),
  r('reconcile-prunes-closed', { type: 'RECONCILE', open: ['a', 'd'], preferPaneId: 'pane-2' }),
  r('reconcile-prunes-the-active', { type: 'RECONCILE', open: ['a', 'c', 'd'], preferPaneId: null }),
  r('reconcile-assigns-to-preferred', { type: 'RECONCILE', open: ['a', 'b', 'c', 'd', 'x', 'y'], preferPaneId: 'pane-3' }),
  r('reconcile-assigns-to-focused', { type: 'RECONCILE', open: ['x', 'a', 'b', 'c', 'd'], preferPaneId: 'nope' }),
  r('reconcile-assigns-to-first', { type: 'RECONCILE', open: ['a', 'b', 'c', 'd', 'x'], preferPaneId: null }, { ...panes, focusedPaneId: 'gone' }),
  r('reconcile-assigns-to-first-no-focus', { type: 'RECONCILE', open: ['x'], preferPaneId: '' }, { ...panes, focusedPaneId: null }),
  r('reconcile-creates-a-default-pane', { type: 'RECONCILE', open: ['a', 'b'], preferPaneId: null }, st([])),
  r('reconcile-creates-and-keeps-a-stale-focus', { type: 'RECONCILE', open: ['a'], preferPaneId: 'x' }, st([], { focusedPaneId: 'x', seq: 4 })),
  r('reconcile-nothing-open', { type: 'RECONCILE', open: [], preferPaneId: null }),
  r('reconcile-nothing-open-no-views', { type: 'RECONCILE', open: [], preferPaneId: null }, st([])),
  r('reconcile-empty-active-is-replaced', { type: 'RECONCILE', open: ['a', 'b'], preferPaneId: null }, st([view('pane-1', ['a', 'b'], '')])),
  r('reconcile-duplicate-assignments', { type: 'RECONCILE', open: ['a', 'b'], preferPaneId: null }, st([view('pane-1', ['a']), view('pane-2', ['a', 'b'], 'b')])),
  r('reconcile-keeps-other-members', { type: 'RECONCILE', open: ['a', 'b', 'c', 'd'], preferPaneId: null }),
  r('unknown-action-is-ignored', { type: 'NOPE' }),
];

// ── scenarios ─────────────────────────────────────────────────────────────────

const box = { left: 0, bottom: 0, right: 600, top: 800 };
const pages = [{ index: 0, ref: { kind: 'objectNumber', pageObjectNumber: 1 }, label: null, size: { width: 600, height: 800 }, rotation: 0, userUnit: 1, boxes: { media: box, crop: box } }];
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
const reads: Step[] = [vm('listPanes'), vm('getPaneOrder'), vm('getFocusedPaneId')];

const plugins = [{ factory: 'view-manager' }];
const snapshot = ['view-manager'];

const cases: ScenarioCase[] = [
  {
    name: 'selectors-before-start',
    covers: 'The capability exists from createKernel(); with no panes the selectors are empty, get() and documentView() are null.',
    plugins,
    snapshot,
    steps: [...reads, vm('getPane', 'pane-1'), vm('getPaneOfDocument', 'a')],
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
      vm('getPane', 'pane-1'),
      vm('getPaneOfDocument', 'b'),
      vm('getPaneOfDocument', 'zzz'),
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
      vm('setActiveDocument', 'pane-1', 'b'),
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
      vm('createPane'),
      open('b'),
      vm('createPane'),
      ...reads,
      vm('movePane', 'pane-3', 0),
      vm('movePane', 'pane-1', 99),
      vm('movePane', 'pane-1', -3),
      vm('movePane', 'pane-2', 1.5),
      vm('movePane', 'pane-2', { $number: 'NaN' }),
      vm('movePane', 'nope', 0),
      vm('getPaneOrder'),
      vm('setFocusedPane', 'pane-1'),
      open('c'),
      vm('listPanes'),
      vm('removePane', 'pane-1'),
      ...reads,
      vm('removePane', 'pane-1'),
      vm('setFocusedPane', 'ghost'),
      vm('getFocusedPaneId'),
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
      vm('createPane'),
      vm('moveDocumentBetween', 'pane-1', 'pane-2', 'b'),
      ...reads,
      vm('moveDocumentBetween', 'pane-2', 'pane-1', 'b', 0),
      vm('moveDocumentWithin', 'pane-1', 'b', 2),
      vm('moveDocumentBetween', 'pane-1', 'pane-1', 'a'),
      vm('setActiveDocument', 'pane-1', null),
      vm('setActiveDocument', 'pane-1', 'zzz'),
      vm('addDocument', 'pane-2', 'a'),
      vm('addDocument', 'pane-2', 'c', 0),
      vm('addDocument', 'pane-2', 'c'),
      vm('removeDocument', 'pane-1', 'c'),
      vm('removeDocument', 'pane-1', 'nope'),
      vm('getPaneOfDocument', 'a'),
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
    name: 'create-populated-and-split',
    covers:
      'createPane with documentIds creates the pane, then moves each document into it (out of the pane that held it); splitPane(documentId) is createPane with that one document, and ignores its from option.',
    plugins,
    snapshot,
    engine: { documents: { a: doc(), b: doc(), c: doc() } },
    steps: [
      start,
      open('a'),
      open('b'),
      open('c'),
      vm('createPane', { documentIds: ['c', 'a', 'zzz'] }),
      ...reads,
      vm('splitPane', 'b', { from: 'pane-2' }),
      ...reads,
      vm('createPane', {}),
      vm('getPane', 'pane-4'),
    ],
  },
  {
    name: 'remove-pane-targets',
    covers:
      'removePane moves its documents to moveDocumentsTo when given, else to the focused pane when that is another one, else to the last remaining pane; a target that is the pane itself or unknown moves nothing, and removing the only pane drops its documents with it.',
    plugins,
    snapshot,
    engine: { documents: { a: doc(), b: doc(), c: doc(), d: doc() } },
    steps: [
      start,
      open('a'),
      vm('splitPane', 'a'),
      open('b'),
      vm('splitPane', 'b'),
      open('c'),
      vm('splitPane', 'c'),
      open('d'),
      ...reads,
      vm('removePane', 'pane-4', { moveDocumentsTo: 'pane-2' }),
      ...reads,
      vm('setFocusedPane', 'pane-1'),
      vm('removePane', 'pane-3'),
      ...reads,
      vm('removePane', 'pane-1'),
      ...reads,
      vm('createPane'),
      vm('removePane', 'pane-2', { moveDocumentsTo: 'pane-2' }),
      ...reads,
      vm('createPane', { documentIds: ['a'] }),
      vm('removePane', 'pane-6', { moveDocumentsTo: 'ghost' }),
      ...reads,
      vm('removePane', 'pane-5'),
      ...reads,
    ],
  },
  {
    name: 'events',
    covers:
      'Every write is diffed: onPaneCreated for each new pane in order, onPaneRemoved for each removed one, onFocusChanged when the focus changes, and onDocumentMoved for each document whose pane changed (a document opening moves from null, one closing to null). A throwing listener is reported and its siblings still fire; after unsubscribing nothing fires.',
    plugins,
    ...UNSNAPSHOTTED,
    engine: { documents: { a: doc(), b: doc() } },
    steps: [
      vm('onPaneCreated', { $fn: 'created', returns: null }),
      vm('onPaneRemoved', { $fn: 'removed', returns: null }),
      vm('onFocusChanged', { $fn: 'focus', returns: null }),
      vm('onDocumentMoved', { $fn: 'moved', throws: { name: 'Error', message: 'listener broke' } }),
      vm('onDocumentMoved', { $fn: 'moved-2', returns: null }),
      start,
      open('a'),
      open('b'),
      vm('splitPane', 'b'),
      vm('setFocusedPane', 'pane-2'),
      vm('moveDocumentWithin', 'pane-1', 'a', 0),
      vm('removePane', 'pane-2'),
      docs('close', 'a'),
      { op: 'call', target: { ref: 0 } },
      { op: 'call', target: { ref: 1 } },
      { op: 'call', target: { ref: 2 } },
      { op: 'call', target: { ref: 3 } },
      { op: 'call', target: { ref: 4 } },
      vm('createPane', { documentIds: ['b'] }),
    ],
  },
  {
    name: 'a-kernel-refuses-the-shipping-plugin',
    covers:
      'The shipping viewManagerPlugin() cannot be installed: its controller spreads its context, which reads the workspace doc getter, which throws, so createKernel throws. Every other case installs it through a factory that hides doc.',
    plugins: [{ factory: 'view-manager-as-shipped' }],
    steps: [...reads],
  },
  {
    name: 'destroy',
    covers: 'After destroy the effect is gone and the capability no longer resolves.',
    plugins,
    snapshot,
    engine: { documents: { a: doc() } },
    steps: [start, open('a'), { op: 'call', target: 'kernel', method: 'destroy' }, vm('listPanes')],
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
