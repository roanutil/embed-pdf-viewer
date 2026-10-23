/**
 * `packages/plugin/view-manager`. Replayed by
 * `crates/plugins/view-manager/tests/conformance.rs`.
 *
 * Named pure transitions as function cases,
 * and the plugin as scenarios: the capability, and the effect that
 * reconciles panes with the kernel's open-document order.
 */
import {
  viewManagerPlugin,
  ViewManagerToken,
} from '../../../../packages/plugin/view-manager/src/index';
import * as model from '../../../../packages/plugin/view-manager/src/model';
import type { ViewManagerState } from '../../../../packages/plugin/view-manager/src/model';

import type { PageLayout } from '@embedpdf/engine-core';
import type { FakeDocumentSpec } from '../fake-engine';
import { registerPlugin, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';

/** Why these cases record no state; see `ScenarioCase` in `../scenario`. */
const UNSNAPSHOTTED = {
  snapshot: 'none',
  reason:
    'Pins the pane events each write fires, which the event callbacks record. Its committed state changes and callbacks are recorded.',
} as const;

registerPlugin('view-manager', { factory: () => viewManagerPlugin(), tokens: [ViewManagerToken] });
registerPlugin('view-manager-as-shipped', {
  factory: () => viewManagerPlugin(),
  tokens: [ViewManagerToken],
});

// Named shipping transitions receive their actual typed positional arguments.
const fn = <K extends keyof typeof model>(
  name: string,
  target: K,
  ...args: Parameters<(typeof model)[K]>
): FunctionCase => ({ name, fn: target, args });

const view = (
  id: string,
  documentIds: string[],
  activeDocumentId: string | null = documentIds[0] ?? null,
) => ({ id, documentIds, activeDocumentId });
const st = (
  views: ReturnType<typeof view>[],
  over: Partial<ViewManagerState> = {},
): ViewManagerState => ({
  panes: Object.fromEntries(views.map((v) => [v.id, v])),
  order: views.map((v) => v.id),
  focusedPaneId: views[0]?.id ?? null,
  seq: views.length,
  ...over,
});
const panes = st(
  [view('pane-1', ['a', 'b', 'c'], 'b'), view('pane-2', ['d'], 'd'), view('pane-3', [], null)],
  { focusedPaneId: 'pane-2' },
);
const nan = NaN;

const reducerCases: FunctionCase[] = [
  fn('initial-state', 'initialViewManagerState'),
  fn('create', 'createPane', panes),
  fn('create-on-empty', 'createPane', st([])),
  fn('remove-unknown-is-ignored', 'removePane', panes, 'nope'),
  fn('remove-focused-focuses-the-last', 'removePane', panes, 'pane-2'),
  fn('remove-unfocused-keeps-focus', 'removePane', panes, 'pane-1'),
  fn('remove-the-only-view', 'removePane', st([view('pane-1', ['a'])]), 'pane-1'),
  fn('reorder', 'movePane', panes, 'pane-3', 0),
  fn('set-focused', 'setFocusedPane', panes, 'pane-3'),
  fn('set-focused-null', 'setFocusedPane', panes, null),
  fn('set-focused-same-is-ignored', 'setFocusedPane', panes, 'pane-2'),
  fn('set-active', 'setActiveDocument', panes, 'pane-1', 'c'),
  fn('set-active-null', 'setActiveDocument', panes, 'pane-1', null),
  fn('set-active-unknown-view', 'setActiveDocument', panes, 'nope', 'a'),
  fn('set-active-foreign-document', 'setActiveDocument', panes, 'pane-1', 'd'),
  fn('add-appends', 'addDocument', panes, 'pane-1', 'e'),
  fn('add-at-index', 'addDocument', panes, 'pane-1', 'e', 1),
  fn('add-at-zero', 'addDocument', panes, 'pane-1', 'e', 0),
  fn('add-negative-clamps', 'addDocument', panes, 'pane-1', 'e', -4),
  fn('add-past-the-end-clamps', 'addDocument', panes, 'pane-1', 'e', 99),
  fn('add-fractional-truncates', 'addDocument', panes, 'pane-1', 'e', 2.9),
  fn('add-nan-lands-first', 'addDocument', panes, 'pane-1', 'e', nan),
  fn('add-infinity-appends', 'addDocument', panes, 'pane-1', 'e', Infinity),
  fn('add-negative-infinity-lands-first', 'addDocument', panes, 'pane-1', 'e', -Infinity),
  fn('add-to-empty-activates', 'addDocument', panes, 'pane-3', 'e'),
  fn('add-present-is-ignored', 'addDocument', panes, 'pane-1', 'a'),
  fn('add-unknown-view-is-ignored', 'addDocument', panes, 'nope', 'e'),
  fn('add-moves-from-its-holder', 'addDocument', panes, 'pane-1', 'd', 0),
  fn('add-moves-into-an-empty-pane', 'addDocument', panes, 'pane-3', 'b'),
  // Malformed duplicate holders are resolved in explicit pane order, not record-key order.
  fn(
    'add-takes-the-first-holder-in-pane-order',
    'addDocument',
    st([view('0', ['x', 'y']), view('1', ['x']), view('b', [])]),
    'b',
    'x',
  ),
  fn('remove-doc-inactive', 'removeDocument', panes, 'pane-1', 'a'),
  fn('remove-doc-active-picks-the-next', 'removeDocument', panes, 'pane-1', 'b'),
  fn(
    'remove-doc-active-last-picks-the-previous',
    'removeDocument',
    st([view('pane-1', ['a', 'b', 'c'], 'c')]),
    'pane-1',
    'c',
  ),
  fn('remove-doc-only-leaves-null', 'removeDocument', panes, 'pane-2', 'd'),
  fn('remove-doc-absent-is-ignored', 'removeDocument', panes, 'pane-1', 'zzz'),
  fn('remove-doc-unknown-view', 'removeDocument', panes, 'nope', 'a'),
  fn('move-within', 'moveDocumentWithin', panes, 'pane-1', 'a', 2),
  fn('move-within-to-front', 'moveDocumentWithin', panes, 'pane-1', 'c', 0),
  fn('move-within-clamps', 'moveDocumentWithin', panes, 'pane-1', 'a', 50),
  fn('move-within-nan', 'moveDocumentWithin', panes, 'pane-1', 'c', nan),
  fn('move-within-absent', 'moveDocumentWithin', panes, 'pane-1', 'd', 0),
  fn('move-within-unknown-view', 'moveDocumentWithin', panes, 'nope', 'a', 0),
  fn('move-between', 'moveDocumentBetween', panes, 'pane-1', 'pane-2', 'b'),
  fn('move-between-at-index', 'moveDocumentBetween', panes, 'pane-1', 'pane-2', 'a', 0),
  fn('move-between-into-empty', 'moveDocumentBetween', panes, 'pane-2', 'pane-3', 'd', 5),
  fn('move-between-same-view', 'moveDocumentBetween', panes, 'pane-1', 'pane-1', 'a'),
  fn('move-between-same-view-at-index', 'moveDocumentBetween', panes, 'pane-1', 'pane-1', 'c', 0),
  fn('move-between-unknown-from', 'moveDocumentBetween', panes, 'nope', 'pane-1', 'a'),
  fn('move-between-unknown-to', 'moveDocumentBetween', panes, 'pane-1', 'nope', 'a'),
  fn('move-between-absent-document', 'moveDocumentBetween', panes, 'pane-1', 'pane-2', 'd'),
  fn('reconcile-prunes-closed', 'reconcile', panes, ['a', 'd'], 'pane-2'),
  fn('reconcile-prunes-the-active', 'reconcile', panes, ['a', 'c', 'd'], null),
  fn(
    'reconcile-assigns-to-preferred',
    'reconcile',
    panes,
    ['a', 'b', 'c', 'd', 'x', 'y'],
    'pane-3',
  ),
  fn('reconcile-assigns-to-focused', 'reconcile', panes, ['x', 'a', 'b', 'c', 'd'], 'nope'),
  fn(
    'reconcile-assigns-to-first',
    'reconcile',
    { ...panes, focusedPaneId: 'gone' },
    ['a', 'b', 'c', 'd', 'x'],
    null,
  ),
  fn(
    'reconcile-assigns-to-first-no-focus',
    'reconcile',
    { ...panes, focusedPaneId: null },
    ['x'],
    '',
  ),
  fn('reconcile-creates-a-default-pane', 'reconcile', st([]), ['a', 'b'], null),
  fn(
    'reconcile-creates-and-keeps-a-stale-focus',
    'reconcile',
    st([], { focusedPaneId: 'x', seq: 4 }),
    ['a'],
    'x',
  ),
  fn('reconcile-nothing-open', 'reconcile', panes, [], null),
  fn('reconcile-nothing-open-no-views', 'reconcile', st([]), [], null),
  fn(
    'reconcile-empty-active-is-replaced',
    'reconcile',
    st([view('pane-1', ['a', 'b'], '')]),
    ['a', 'b'],
    null,
  ),
  fn(
    'reconcile-duplicate-assignments',
    'reconcile',
    st([view('pane-1', ['a']), view('pane-2', ['a', 'b'], 'b')]),
    ['a', 'b'],
    null,
  ),
  fn('reconcile-keeps-other-members', 'reconcile', panes, ['a', 'b', 'c', 'd'], null),
  fn('next-pane-id', 'nextPaneId', panes),
  fn('create-at-front', 'createPane', panes, 0),
  fn('create-at-fraction', 'createPane', panes, 1.9),
  fn('create-at-negative', 'createPane', panes, -9),
  fn('create-at-infinity', 'createPane', panes, Infinity),
  fn('create-at-nan', 'createPane', panes, NaN),
  fn('create-sequence-survives-removal', 'createPane', st([], { seq: 8 })),
  fn('move-pane-negative', 'movePane', panes, 'pane-2', -1),
  fn('move-pane-infinity', 'movePane', panes, 'pane-1', Infinity),
  fn('move-pane-fractional', 'movePane', panes, 'pane-3', 1.9),
  fn('move-pane-nan', 'movePane', panes, 'pane-3', NaN),
  fn('move-pane-unknown', 'movePane', panes, 'unknown', 0),
  fn('move-pane-same-index', 'movePane', panes, 'pane-2', 1),
  fn('rename-active', 'renameDocument', panes, 'b', 'real'),
  fn('rename-inactive', 'renameDocument', panes, 'a', 'real'),
  fn('rename-absent', 'renameDocument', panes, 'absent', 'real'),
  fn('pane-of-document', 'paneOfDocument', panes, 'd'),
  fn('pane-of-absent-document', 'paneOfDocument', panes, 'absent'),
  fn(
    'holder-follows-pane-order-not-property-order',
    'addDocument',
    st([view('0', ['x', 'y']), view('1', ['x']), view('b', [])], { order: ['1', '0', 'b'] }),
    'b',
    'x',
  ),
];

// ── scenarios ─────────────────────────────────────────────────────────────────

const box = { x: 0, y: 0, width: 600, height: 800 };
const pages: PageLayout[] = [
  {
    index: 0,
    ref: { kind: 'objectNumber', objectNumber: 1 },
    label: null,
    size: { width: 600, height: 800 },
    rotation: 0,
    userUnit: 1,
    tabOrder: null,
    boxes: { media: box, crop: box, bleed: box, trim: box, art: box },
    pdfCropBox: { left: 0, bottom: 0, right: 600, top: 800 },
  },
];
const doc = (over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec => ({ pages, ...over });

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const open = (id: string, options?: unknown): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args:
    options === undefined
      ? [{ kind: 'bytes', id, bytes: { $bytes: '' } }]
      : [{ kind: 'bytes', id, bytes: { $bytes: '' } }, options],
});
const docs = (method: string, ...args: unknown[]): Step => ({
  op: 'call',
  target: 'documents',
  method,
  args,
});
const vm = (method: string, ...args: unknown[]): Step => ({
  op: 'call',
  target: { token: 'view-manager' },
  method,
  args,
});
const reads: Step[] = [vm('listPanes'), vm('getPaneOrder'), vm('getFocusedPaneId')];

const plugins = [{ factory: 'view-manager' }];
const snapshot = ['view-manager'];

const cases: ScenarioCase[] = [
  {
    name: 'pending-rename-preserves-split-and-tab-position',
    covers:
      'A thunk ticket is split before resolving; its real id replaces it in that pane and active slot without losing placement. Later split uses an explicit nonfocused from pane.',
    plugins,
    snapshot,
    engine: { documents: { real: doc(), b: doc() } },
    steps: [
      start,
      docs('open', { $fn: 'source', defer: 'source' }),
      vm('splitPane', 'pending:1'),
      ...reads,
      {
        op: 'settle',
        label: 'source',
        value: { kind: 'bytes', id: 'real', bytes: { $bytes: '' } },
      },
      ...reads,
      open('b'),
      vm('createPane'),
      vm('splitPane', 'b', { from: 'pane-1' }),
      ...reads,
    ],
  },
  {
    name: 'selectors-before-start',
    covers:
      'The capability exists from createKernel(); with no panes the selectors are empty, getPane() and getPaneOfDocument() are null.',
    plugins,
    snapshot,
    steps: [...reads, vm('getPane', 'pane-1'), vm('getPaneOfDocument', 'a')],
  },
  {
    name: 'first-open-makes-a-pane',
    covers:
      'The effect reconciles whenever the open order changes: the first open (while still loading) creates pane-1 and assigns the document to it; later opens land in the focused pane; getPane/getPaneOfDocument/listPanes read the current public view.',
    plugins,
    snapshot,
    engine: {
      documents: { a: doc(), b: doc(), c: doc() },
      responses: [
        {
          path: 'open',
          args: [{ kind: 'bytes', id: 'c', bytes: { $bytes: '' } }, {}],
          defer: 'open-c',
        },
      ],
    },
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
    covers:
      'Closing a document drops it from its pane and picks a neighbour as active; closing every document leaves the empty pane.',
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
      'createPane focuses the new pane and later opens land there; removePane refocuses; movePane reorders with clamping and ignores unknown ids; setFocused takes any id.',
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
      'addDocument/removeDocument/setActiveDocument/moveDocumentWithin/moveDocumentBetween apply current transitions; the model ignores the invalid ones.',
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
    covers:
      'documents.move changes the open order, so the effect reconciles (a no-op for pane contents); setActive alone does not.',
    plugins,
    snapshot,
    engine: { documents: { a: doc(), b: doc() } },
    steps: [start, open('a'), open('b'), docs('setActive', 'a'), docs('move', 'b', 0), ...reads],
  },
  {
    name: 'a-failed-open',
    covers:
      'A document whose open fails keeps its tab in the order (status error), so it stays assigned until it is closed.',
    plugins,
    snapshot,
    engine: {
      documents: { a: doc() },
      responses: [
        {
          path: 'open',
          args: [{ kind: 'bytes', id: 'x', bytes: { $bytes: '' } }, {}],
          error: { name: 'EngineError', message: 'bad file', code: 'Format' },
        },
      ],
    },
    steps: [start, open('a'), open('x'), ...reads, docs('close', 'x'), ...reads],
  },
  {
    name: 'create-populated-and-split',
    covers:
      'createPane with documentIds creates the pane, then moves each document into it (out of the pane that held it); splitPane(documentId) is createPane with that one document, and inserts beside its from option.',
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
    name: 'a-kernel-installs-the-shipping-plugin',
    covers:
      'The unmodified shipping workspace plugin installs successfully; its selectors are available before start.',
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
    describes:
      'packages/plugin/view-manager: named model transitions, generated pane ids and ordered document partition.',
    functions: { ...model },
    cases: reducerCases,
  },
  {
    kind: 'scenario',
    file: 'plugins/view-manager/plugin.json',
    describes: 'packages/plugin/view-manager: panes over the open documents.',
    cases,
  },
];
