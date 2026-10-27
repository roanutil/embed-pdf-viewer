import { LIVE_ANSWERS } from './annotation-live-answers';
import { ix, sample } from './annotation-live-inputs';
/**
 * `packages/plugin/annotation`: the hover state (`hoverAt`, dispatched on
 * change only), the edit handler's hover sampling, and the cursor
 * enter/exit feed onto the actions plugin's hover pump (`hover-feed.ts`),
 * plus the permission twins. Replayed by
 * `crates/plugins/annotation/tests/conformance.rs`.
 */
import type { ScenarioCase, Step } from '../scenario';
import type { Suite } from '../vectors';
import {
  PLUGINS,
  SNAP,
  boot,
  call,
  pr,
  doc,
  hydrate,
  link,
  objRef,
  square,
  widget,
} from './annotation-live-fixtures';
/** Why these cases record no state; see `ScenarioCase` in `../scenario`. */
const SESSION_SNAPSHOT = { snapshot: SNAP };
const tree = (name: string) => ({
  root: { type: 'named' as const, subtype: 'Named', name, next: [] },
  incomplete: false,
  warningFlags: 0,
  warnings: [],
});
const actionsCall = (method: string, ...args: unknown[]): Step => ({
  op: 'call',
  target: { token: 'actions', doc: 'a' },
  method,
  args,
});
const rect = (x: number, y: number) => ({
  x: x,
  y: 800 - (800 - y),
  width: x + 50 - x,
  height: 800 - y - (800 - y - 50),
});
const hoverable = [
  square(1, {
    rect: rect(0, 0),
    actions: { cursorEnter: tree('enter-1'), cursorExit: tree('exit-1') },
    box: rect(0, 0),
  }),
  square(2, { rect: rect(100, 0), actions: { cursorEnter: tree('enter-2') }, box: rect(100, 0) }),
  square(3, { rect: rect(200, 0), box: rect(200, 0) }),
  link(5, { rect: rect(400, 0), actions: { cursorExit: tree('exit-5') } }),
  square(6, { rect: rect(500, 0), actions: { cursorExit: tree('exit-6') }, box: rect(500, 0) }),
];
const lentWidget = widget(4, 'unknown', {
  rect: rect(300, 0),
  box: rect(300, 0),
  actions: { cursorEnter: tree('enter-4') },
});
const annotationsList = { path: 'page.annotations.list', value: { annotations: hoverable } };
const FORMS_EMPTY = {
  path: 'forms.list',
  value: {
    formKind: 'none',
    needsAppearances: false,
    fields: [],
    widgets: [lentWidget],
    calculationOrder: [],
  },
};
const hoverAt = (x: number, y: number) =>
  call('hoverAt', [{ page: pr(1), point: { x, y }, scale: 1 }]);
const cases: ScenarioCase[] = [
  {
    name: 'hover-state-on-change-only',
    covers:
      'hoverAt hit-tests the point and dispatches a hover only when the topmost annotation changes; null (off every page) clears. The edit handler’s hover drives it along with the cursor claim.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([hydrate([...hoverable]), ...LIVE_ANSWERS['hover-state-on-change-only']]),
      },
    },
    snapshot: SNAP,
    steps: [
      ...boot,
      hoverAt(1, 25),
      hoverAt(1, 26),
      hoverAt(101, 25),
      call('hoverAt', [null]),
      call('hoverAt', [null]),
      ix('dispatchPointer', [sample('move', 201, 25)]),
      ix('dispatchPointer', [
        {
          phase: 'move',
          viewport: { x: 0, y: 0 },
          modifiers: { shift: false, alt: false, ctrl: false, meta: false },
        },
      ]),
      call('listPageItems', [pr(1)]),
    ],
  },
  {
    name: 'hover-feeds-the-actions-pump',
    covers:
      'With the actions plugin present, each hover change reports to the shared pump: an annotation with /E or /X trees is a target with per-event flags; drafts, widgets, links and tree-less annotations are not. The pump runs the trees as hover-origin dispatches.',
    plugins: [
      { factory: 'interaction' },
      { factory: 'annotation-live-actions', config: { openSequence: 'off' } },
      { factory: 'annotation-live' },
    ],
    engine: {
      documents: {
        a: doc([
          hydrate([...hoverable]),
          annotationsList,
          FORMS_EMPTY,
          ...LIVE_ANSWERS['hover-feeds-the-actions-pump'],
        ]),
      },
    },
    ...SESSION_SNAPSHOT,
    steps: [
      ...boot,
      call('lend', [
        {
          owner: 'hover-fixture',
          records: {
            $fn: 'lender.records',
            returns: { confirmed: [lentWidget], shown: [lentWidget] },
          },
          opsOf: {
            $fn: 'lender.ops',
            throws: { name: 'Error', message: 'hover fixture is read only' },
          },
          undoOf: { $fn: 'lender.undo', returns: [] },
          reorderOps: {
            $fn: 'lender.reorder',
            throws: { name: 'Error', message: 'hover fixture is read only' },
          },
          authority: { $fn: 'lender.authority', returns: { update: true, delete: true } },
          pictureEpoch: { $fn: 'lender.epoch', returns: 'hover-fixture' },
          pictureVersion: { $fn: 'lender.version', returns: 0 },
          pending: { $fn: 'lender.pending', returns: false },
          painter: { $fn: 'lender.painter', returns: null },
          renderAppearances: { $fn: 'lender.pictures', resolves: [] },
        },
      ]),
      actionsCall('onDiagnosticReported', { $fn: 'actions.diagnostic' }),
      actionsCall('onScriptFailed', { $fn: 'actions.scriptFailed' }),
      actionsCall('onExecuted', { $fn: 'actions.executed' }),
      actionsCall('registerExecutor', 'named', {
        $fn: 'exec.named',
        returns: { status: 'executed' },
      }),
      hoverAt(1, 25),
      hoverAt(101, 25),
      hoverAt(201, 25),
      hoverAt(301, 25),
      hoverAt(401, 25),
      hoverAt(501, 25),
      hoverAt(101, 25),
      call('hoverAt', [null]),
      call('createPointer', ['square', 'down', pr(1), { x: 100, y: 500 }]),
      call('createPointer', ['square', 'up', pr(1), { x: 180, y: 560 }]),
      hoverAt(100, 530),
      hoverAt(140, 530),
    ],
  },
  {
    name: 'permission-twins',
    covers:
      'canCreate is the caller’s create authority; canEdit and canDelete are the fused per-record predicates (authority stamped at ingest, and the locked flag); canRead mirrors doc.annotate.read. An unknown ref can do neither.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          hydrate([square(1), square(2, { ...{ locked: true } }), square(3, { userId: 'u9' })]),
          ...LIVE_ANSWERS['permission-twins'],
        ]),
        b: doc([hydrate([square(1)]), ...LIVE_ANSWERS['permission-twins']], {
          security: {
            allows: ['doc.annotate.read'],
            allowsAnnotation: { update: false, delete: true },
          },
        }),
        c: doc([hydrate([square(1)]), ...LIVE_ANSWERS['permission-twins']], {
          security: {
            allows: ['doc.annotate.read'],
            allowsAnnotation: { create: true, update: true, delete: false },
          },
        }),
      },
    },
    ...SESSION_SNAPSHOT,
    steps: [
      ...boot,
      {
        op: 'call',
        target: 'documents',
        method: 'open',
        args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }],
      },
      {
        op: 'call',
        target: 'documents',
        method: 'open',
        args: [{ kind: 'bytes', id: 'c', bytes: { $bytes: '' } }],
      },
      ...['a', 'b', 'c'].flatMap((d): Step[] => [
        call('canRead', [], d),
        call('canCreate', [], d),
        call('canUpdate', [objRef(1)], d),
        call('canDelete', [objRef(1)], d),
        call('canUpdate', [objRef(2)], d),
        call('canDelete', [objRef(2)], d),
        call('canUpdate', [objRef(99)], d),
        call('canDelete', [objRef(99)], d),
      ]),
      call('selection.set', [[objRef(1)]], 'b'),
      call('getHitKind', [pr(1), { x: 150, y: 150 }], 'b'),
      call('listChromeNodes', [pr(1)], 'b'),
      call('get', [objRef(3)]),
    ],
  },
];
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/annotation-hover.json',
    describes:
      'packages/plugin/annotation: hover state, the cursor enter/exit feed onto the actions hover pump, and the permission twins.',
    cases,
  },
];
