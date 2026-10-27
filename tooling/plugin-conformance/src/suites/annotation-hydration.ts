/** The current flat records mirror: loads, cursors, confirmed facts and page reloads. */
import type { ScenarioCase, Step } from '../scenario';
import type { Suite } from '../vectors';
import {
  scenario,
  call,
  doc,
  hydrate,
  snapshot,
  square,
  circle,
  freeText,
  baseRef,
  objRef,
  pr,
  layout,
  PAGES,
  origin,
  LOCAL,
  REMOTE,
  createdFact,
  updatedFact,
  deletedFact,
  restoredFact,
  emit,
  settle,
  fail,
  boot,
} from './annotation-live-fixtures';
const seed = [
  square(1),
  square(2, {
    box: { x: 300, y: 300, width: 100, height: 100 },
    rect: { x: 300, y: 300, width: 100, height: 100 },
  }),
  circle(3, { page: pr(2), ref: objRef(3, 2) }),
];
const remote = (serverId: number | null) => origin('remote', serverId);
// Deliberately malformed external cursor values: these reach the current mirror's JS comparison.
const malformedFact = (n: number, serverId: unknown): Step => ({
  op: 'emit',
  doc: 'a',
  event: { ...createdFact(square(n)), origin: { ...REMOTE, serverId } },
});
const meta = { affectedPages: [pr(2)], cacheDelta: null, opId: 'pages', undoable: true };
const cases: ScenarioCase[] = [
  scenario(
    'hydrates-every-page-at-open',
    'A single flat annotations.list loads current DTOs and canonical page refs; reads and whenSynced see the mirror.',
    [
      call('getStatus'),
      settle('all', snapshot(seed)),
      call('getStatus'),
      call('list', [{ pages: [pr(1)] }]),
      call('list', [{ pages: [pr(2)] }]),
      call('get', [objRef(3, 2)]),
      call('get', [objRef(77)]),
      call('getAppearanceEpoch', [pr(1)]),
      call('whenSynced'),
      call('canRead'),
    ],
    [{ path: 'annotations.list', defer: 'all' }],
  ),
  scenario(
    'no-read-grant-means-forbidden',
    'Without doc.annotate.read the mirror refuses hydration without an engine read; refresh rechecks the gate.',
    [call('getStatus'), call('canRead'), call('refresh'), call('refresh'), call('getStatus')],
    [],
    { engine: { documents: { a: doc([], { security: { allows: ['annotations:create'] } }) } } },
  ),
  scenario(
    'a-failed-hydration-degrades-to-live',
    'Failed bulk load reports error while queued/live facts still fold; a later full refresh returns ready.',
    [
      emit(createdFact(square(5), remote(4))),
      fail('first', 'list failed'),
      call('getStatus'),
      emit(createdFact(square(6), remote(8))),
      call('list'),
      call('refresh'),
      call('getStatus'),
      call('list'),
    ],
    [{ path: 'annotations.list', times: 1, defer: 'first' }, hydrate(seed)],
  ),
  scenario(
    'the-hydration-window-replays-by-cursor',
    'The flat snapshot cursor drops already included facts; newer and cursorless confirmed facts replay after load.',
    [
      emit(deletedFact([objRef(1)], remote(9))),
      emit(createdFact(square(7), remote(11))),
      emit(updatedFact(square(2, { contents: 'old' }), true, remote(10))),
      emit(updatedFact(square(2, { contents: 'new' }), false, remote(12))),
      emit(deletedFact([objRef(3, 2)], remote(null))),
      settle('all', snapshot(seed, 10)),
      call('list'),
    ],
    [{ path: 'annotations.list', defer: 'all' }],
  ),
  scenario(
    'a-local-engine-replays-everything',
    'A snapshot without an audit cursor replays all queued facts in arrival order.',
    [
      emit(createdFact(square(7), remote(1))),
      emit(createdFact(square(8), remote(2))),
      settle('all', snapshot()),
      call('list'),
    ],
    [{ path: 'annotations.list', defer: 'all' }],
  ),
  scenario(
    'mixed-type-cursors-compare-as-javascript',
    'Explicit malformed upstream cursor values retain the current JS comparison behavior; ordinary events remain complete.',
    [
      ...[
        [7, true],
        [8, '3'],
        [9, ' '],
        [10, '6'],
      ].map(([n, cursor]) => malformedFact(n as number, cursor)),
      settle('all', snapshot([], 5)),
      call('list'),
    ],
    [{ path: 'annotations.list', defer: 'all' }],
  ),
  scenario(
    'a-null-cursor-reads-as-zero',
    'A null snapshot cursor means no watermark in the current mirror, so all numeric event cursors replay.',
    [
      emit(createdFact(square(7), remote(0))),
      emit(createdFact(square(8), remote(1))),
      settle('all', { annotations: [], auditHead: null }),
      call('list'),
    ],
    [{ path: 'annotations.list', defer: 'all' }],
  ),
  scenario(
    'string-cursors-compare-as-strings',
    'Explicit malformed string watermark and event cursors exercise the actual current mirror comparison, not an old adapter.',
    [
      malformedFact(7, '11'),
      malformedFact(8, '3'),
      settle('all', { annotations: [], auditHead: '2' }),
      call('list'),
    ],
    [{ path: 'annotations.list', defer: 'all' }],
  ),
  scenario(
    'live-remote-events',
    'Current created/updated/reordered/deleted facts fold from all origins; changed appearance advances the picture epoch.',
    [
      call('onCreated', [{ $fn: 'created' }]),
      call('onUpdated', [{ $fn: 'updated' }]),
      call('onDeleted', [{ $fn: 'deleted' }]),
      emit(createdFact(square(10), remote(20))),
      emit(
        createdFact(
          freeText(11, { reply: { to: objRef(10), type: 'reply' }, contents: 'hi' }),
          remote(21),
        ),
      ),
      emit(updatedFact(square(1, { color: '#010203' }), true, remote(22))),
      call('getAppearanceEpoch', [pr(1)]),
      emit({
        type: 'annotations.reordered',
        page: pr(1),
        order: [objRef(2), objRef(1), objRef(10), objRef(11)],
        meta: { ...meta, affectedPages: [pr(1)], changed: [objRef(2)] },
        origin: remote(24),
      }),
      emit(deletedFact([objRef(2)], remote(26))),
      emit(updatedFact(square(1, { contents: 'mine' }), true, LOCAL)),
      call('list'),
    ],
    [hydrate(seed)],
  ),
  scenario(
    'confirmed-origin-delivery',
    'The actual engine stream delivers complete local/remote origins unchanged to public fact listeners.',
    [
      call('onCreated', [{ $fn: 'created' }]),
      call('onUpdated', [{ $fn: 'updated' }]),
      call('onDeleted', [{ $fn: 'deleted' }]),
      emit(createdFact(square(3), remote(1))),
      emit(createdFact(square(4), { ...remote(2), sub: 'u1' })),
      emit(updatedFact(square(1, { contents: 'x' }), false, LOCAL)),
      emit(deletedFact([objRef(2)], { ...remote(3), sub: null })),
      call('list'),
    ],
    [hydrate([square(1), square(2)])],
  ),
  scenario(
    'records-without-a-ref',
    'Explicit malformed protocol negative: a required Annotation.ref is absent. The current mirror reports a failed fold and reloads; a malformed full load preserves the last confirmed data and status error.',
    [
      {
        op: 'emit',
        doc: 'a',
        event: {
          ...createdFact(square(7)),
          annotation: Object.fromEntries(
            Object.entries(square(7)).filter(([key]) => key !== 'ref'),
          ),
        },
      },
      call('getStatus'),
      call('list'),
      call('refresh'),
      call('getStatus'),
      call('list'),
    ],
    [
      { ...hydrate([square(1)]), times: 2 },
      {
        path: 'annotations.list',
        value: {
          annotations: [
            Object.fromEntries(Object.entries(square(8)).filter(([key]) => key !== 'ref')),
          ],
        },
      },
    ],
  ),
  scenario(
    'stable-base-index-records',
    'Current reads can name a base annotation by immutable baseIndex; get/update identity no longer depends on an index revision or absent ref.',
    [
      call('get', [baseRef(0)]),
      call('selection.set', [[baseRef(0)]]),
      call('selection.list'),
      emit(updatedFact(square(1, { ref: baseRef(0), contents: 'base changed' }))),
      call('get', [baseRef(0)]),
      call('refresh'),
      call('get', [baseRef(0)]),
    ],
    [
      { ...hydrate([square(1, { ref: baseRef(0) })]), times: 1 },
      hydrate([square(1, { ref: baseRef(0), contents: 'base reloaded' })]),
    ],
  ),
  scenario(
    'rehydrate-chains-after-the-flight',
    'A refresh during an in-flight reload schedules one further load, and current stream.desynced starts the reload.',
    [
      emit({ type: 'stream.desynced', reason: 'backlog-overflow', ts: 1 }),
      call('refresh'),
      call('getStatus'),
      settle('second', snapshot([square(1, { contents: 'second' })])),
      call('getStatus'),
      settle('third', snapshot([square(1, { contents: 'third' })])),
      call('getStatus'),
      call('list'),
    ],
    [
      { ...hydrate(seed), times: 1 },
      { path: 'annotations.list', times: 1, defer: 'second' },
      { path: 'annotations.list', defer: 'third' },
    ],
  ),
  scenario(
    'page-membership-events',
    'Current complete layout facts drop retired pages and reload newly inserted pages through annotations.list({pages}).',
    [
      emit({
        type: 'pages.deleted',
        pages: [pr(2)],
        layout: { pageCount: 1, pages: [PAGES[0]!], namedPages: [] },
        meta,
        origin: LOCAL,
      }),
      call('list', [{ pages: [pr(2)] }]),
      emit({
        type: 'pages.inserted',
        insertedPages: [pr(2)],
        layout: { pageCount: 2, pages: PAGES, namedPages: [] },
        meta,
        origin: REMOTE,
      }),
      call('list', [{ pages: [pr(2)] }]),
    ],
    [
      { ...hydrate(seed), times: 1 },
      {
        path: 'annotations.list',
        args: [{ pages: [pr(2)] }],
        value: snapshot([circle(4, { page: pr(2), ref: objRef(4, 2) })]),
      },
    ],
  ),
  scenario(
    'cross-plane-events',
    'Current redaction.applied reloads only applied pages using the flat list; unchanged pages keep records and their picture versions.',
    [
      emit({
        type: 'redaction.applied',
        scope: { pages: [pr(1), pr(2)] },
        results: [
          { status: 'applied', page: pr(1), removedAnnotationCount: 1 },
          { status: 'unchanged', page: pr(2), removedAnnotationCount: 0 },
        ],
        removedAnnotationCount: 1,
        meta: { ...meta, affectedPages: [pr(1)] },
        origin: LOCAL,
      }),
      call('list'),
      call('getAppearanceEpoch', [pr(2)]),
    ],
    [
      { ...hydrate(seed), times: 1 },
      { path: 'annotations.list', args: [{ pages: [pr(1)] }], value: snapshot([square(1)]) },
    ],
  ),
  scenario(
    'restored-page-reloads',
    'The current annotations.restored event requests a page load; a failed reload keeps the previous view and status error.',
    [
      emit(restoredFact(pr(1))),
      call('list'),
      emit(restoredFact(pr(1))),
      call('getStatus'),
      call('list'),
    ],
    [
      { ...hydrate(seed), times: 1 },
      {
        path: 'annotations.list',
        args: [{ pages: [pr(1)] }],
        times: 1,
        value: snapshot([square(2, { contents: 'fresh' })]),
      },
      {
        path: 'annotations.list',
        error: { name: 'EngineError', message: 'intended reload failure', code: 'Unavailable' },
      },
    ],
  ),
  scenario(
    'closing-unsubscribes',
    'Closing releases the annotation mirror event subscription and document-scoped state.',
    [{ op: 'call', target: 'documents', method: 'close', args: ['a'] }],
    [hydrate(seed)],
    { snapshot: 'none', reason: 'Subscription lifetime is visible in engineState.' },
  ),
  scenario(
    'rotated-page-layout',
    'Annotations already live in page space on rotated/offset pages; no removed PDF/content bridge is called.',
    [
      call('list', [{ pages: [pr(3)] }]),
      call('getAt', [pr(3), { x: 25, y: 265 }]),
      call('getAnnotationAnchor', [objRef(1, 3)]),
      call('getAt', [pr(4), { x: 0, y: 0 }]),
    ],
    [
      hydrate([
        square(1, {
          page: pr(3),
          ref: objRef(1, 3),
          box: { x: 10, y: 250, width: 30, height: 30 },
          rect: { x: 10, y: 250, width: 30, height: 30 },
        }),
      ]),
    ],
    {
      engine: {
        documents: {
          a: doc(
            [
              hydrate([
                square(1, {
                  page: pr(3),
                  ref: objRef(1, 3),
                  box: { x: 10, y: 250, width: 30, height: 30 },
                  rect: { x: 10, y: 250, width: 30, height: 30 },
                }),
              ]),
            ],
            { pages: [layout(0, 3, { left: 50, bottom: 50, right: 250, top: 350 }, 90)] },
          ),
        },
      },
    },
  ),
];
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/annotation-hydration.json',
    describes:
      'Current annotation records mirror, flat list loads, cursor replay, current document facts and lifetime.',
    cases,
  },
];
