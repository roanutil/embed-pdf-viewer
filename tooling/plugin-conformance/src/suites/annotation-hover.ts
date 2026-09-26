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
  FLAGS,
  PLUGINS,
  SNAP,
  boot,
  call,
  doc,
  hydrate,
  ix,
  link,
  objRef,
  sample,
  square,
  widget,
} from './annotation-fixtures';

const tree = (name: string) => ({ root: { type: 'named', subtype: 'Named', name, next: [] }, incomplete: false, warningFlags: 0, warnings: [] });
const actionsCall = (method: string, ...args: unknown[]): Step => ({ op: 'call', target: { token: 'actions', doc: 'a' }, method, args });

const rect = (x: number, y: number) => ({ left: x, bottom: 800 - y - 50, right: x + 50, top: 800 - y });
const hoverable = [
  square(1, { rect: rect(0, 0), actions: { cursorEnter: tree('enter-1'), cursorExit: tree('exit-1') } }),
  square(2, { rect: rect(100, 0), actions: { cursorEnter: tree('enter-2') } }),
  square(3, { rect: rect(200, 0) }),
  widget(4, 'text', { rect: rect(300, 0), actions: { cursorEnter: tree('enter-4') } }),
  link(5, { rect: rect(400, 0), actions: { cursorExit: tree('exit-5') } }),
  square(6, { rect: rect(500, 0), actions: { cursorExit: tree('exit-6') } }),
];
const annotationsList = { path: 'page.annotations.list', value: { annotations: hoverable } };
const FORMS_EMPTY = { path: 'forms.list', value: { formKind: 'none', needsAppearances: false, fields: [], calculationOrder: [] } };

const hoverAt = (x: number, y: number) => call('hoverAt', [{ pon: 1, point: { x, y }, scale: 1 }]);

const cases: ScenarioCase[] = [
  {
    name: 'hover-state-on-change-only',
    covers:
      'hoverAt hit-tests the point and dispatches a hover only when the topmost annotation changes; null (off every page) clears. The edit handler’s hover drives it along with the cursor claim.',
    plugins: PLUGINS,
    engine: { documents: { a: doc([hydrate({ 1: hoverable })]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      hoverAt(1, 25),
      hoverAt(1, 26),
      hoverAt(101, 25),
      call('hoverAt', [null]),
      call('hoverAt', [null]),
      ix('dispatch', [sample("move", 201, 25)]),
      ix('dispatch', [{ phase: 'move', viewport: { x: 0, y: 0 }, modifiers: { shift: false, alt: false, ctrl: false, meta: false } }]),
      call('pageItems', [1]),
    ],
  },
  {
    name: 'hover-feeds-the-actions-pump',
    covers:
      'With the actions plugin present, each hover change reports to the shared pump: an annotation with /E or /X trees is a target with per-event flags; drafts, widgets, links and tree-less annotations are not. The pump runs the trees as hover-origin dispatches.',
    plugins: [{ factory: 'interaction' }, { factory: 'actions', config: { openSequence: 'off' } }, { factory: 'annotation' }],
    engine: { documents: { a: doc([hydrate({ 1: hoverable }), annotationsList, FORMS_EMPTY, { path: 'page.annotations.create', pon: 1, defer: 'draft' }]) } },
    snapshot: 'none',
    steps: [
      ...boot,
      actionsCall('registerExecutor', 'named', { $fn: 'exec.named', returns: { status: 'executed' } }),
      hoverAt(1, 25),
      hoverAt(101, 25),
      hoverAt(201, 25),
      hoverAt(301, 25),
      hoverAt(401, 25),
      hoverAt(501, 25),
      hoverAt(101, 25),
      call('hoverAt', [null]),
      call('createPointer', ['square', 'down', 1, { x: 100, y: 500 }]),
      call('createPointer', ['square', 'up', 1, { x: 180, y: 560 }]),
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
        a: doc([hydrate({ 1: [square(1), square(2, { flags: FLAGS({ locked: true }) }), square(3, { userId: 'u9' })] })]),
        b: doc([hydrate({ 1: [square(1)] })], { security: { allows: ['doc.annotate.read'], allowsAnnotationMutation: { update: false, delete: true } } }),
        c: doc([hydrate({ 1: [square(1)] })], { security: { allows: ['doc.annotate.read'], allowsAnnotationCreate: true, allowsAnnotationMutation: { update: true, delete: false } } }),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }] },
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'c', bytes: { $bytes: '' } }] },
      ...['a', 'b', 'c'].flatMap((d): Step[] => [
        call('canRead', [], d),
        call('canCreate', [], d),
        call('canEdit', [objRef(1)], d),
        call('canDelete', [objRef(1)], d),
        call('canEdit', [objRef(2)], d),
        call('canDelete', [objRef(2)], d),
        call('canEdit', [objRef(99)], d),
        call('canDelete', [objRef(99)], d),
      ]),
      call('select', [objRef(1)], 'b'),
      call('hitKind', [1, { x: 150, y: 150 }], 'b'),
      call('chrome', [1], 'b'),
      call('get', [objRef(3)]),
    ],
  },
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/annotation-hover.json',
    describes: 'packages/plugin/annotation: hover state, the cursor enter/exit feed onto the actions hover pump, and the permission twins.',
    cases,
  },
];
