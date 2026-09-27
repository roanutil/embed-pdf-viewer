/**
 * `packages/plugin/page-edit`. Replayed by
 * `crates/plugins/page-edit/tests/conformance.rs`.
 *
 * Stateless and document-scoped: every case is a scenario over the fake
 * engine, whose `security.allows` answers the `canEdit()` twin and whose
 * response table answers the page service (`pages.rotate` and the rest),
 * including the engine's own `PermissionDenied` refusal: the plugin has no
 * client-side gate, so a refused edit is the engine's rejection, passed
 * through as the promise the verb returns.
 */
import { pageEditPlugin, PageEditToken } from '../../../../packages/plugin/page-edit/src/index';

import type { FakeDocumentSpec } from '../fake-engine';
import { registerPlugin, type ScenarioCase, type Step } from '../scenario';
import type { Suite } from '../vectors';

registerPlugin('page-edit', { factory: () => pageEditPlugin(), tokens: [PageEditToken] });

const pr = (pon: number) => ({ kind: 'objectNumber', pageObjectNumber: pon });
const box = (w: number, h: number) => ({ left: 0, bottom: 0, right: w, top: h });
const page = (pon: number, index: number, rotation: number, width: number, height: number) => ({
  index,
  ref: pr(pon),
  label: null,
  size: { width, height },
  rotation,
  userUnit: 1,
  boxes: { media: box(width, height), crop: box(width, height) },
});
const three = [page(10, 0, 0, 100, 200), page(20, 1, 90, 300, 400), page(30, 2, 270, 500, 600)];
const doc = (over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec => ({
  pages: three,
  security: { allows: ['doc.pages.assemble'] },
  ...over,
});
const ok = (path: string, value: unknown = { ok: true }) => ({ path, value });

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const open = (id: string): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id, bytes: { $bytes: '' } }],
});
const edit = (method: string, args: unknown[] = [], doc = 'a'): Step => ({
  op: 'call',
  target: { token: 'page-edit', doc },
  method,
  args,
});
const denied = (required = 'doc.pages.assemble') => ({
  name: 'PermissionDenied',
  message: `permission denied (local): ${required}`,
  required,
  context: 'local',
});

const plugins = [{ factory: 'page-edit' }];
const answering = [
  ok('pages.rotate'),
  ok('pages.move'),
  ok('pages.delete'),
  ok('pages.insertBlank', { insertedPages: [pr(99)] }),
  ok('pages.insert', { insertedPages: [pr(77), pr(78)] }),
];

const cases: ScenarioCase[] = [
  {
    name: 'can-edit',
    covers: 'canEdit() is the doc.pages.assemble authority, wildcard included; it is the one twin for every structure verb.',
    plugins,
    engine: {
      documents: {
        a: doc(),
        b: doc({ security: { allows: ['doc.render', 'doc.pages.modify'] } }),
        c: doc({ security: { allows: '*' } }),
      },
    },
    steps: [start, open('a'), open('b'), open('c'), edit('canEdit', [], 'a'), edit('canEdit', [], 'b'), edit('canEdit', [], 'c')],
  },
  {
    name: 'rotate-by',
    covers:
      'rotateBy turns a relative turn into the absolute wire from the registry rotation, wrapping into [0, 360); an unknown page rejects not-found; any number is carried through the arithmetic.',
    plugins,
    snapshot: 'none',
    engine: { documents: { a: doc({ responses: answering }) } },
    steps: [
      start,
      open('a'),
      edit('rotateBy', [[pr(10)], 90]),
      edit('rotateBy', [[pr(10)], -90]),
      edit('rotateBy', [[pr(20)], 90]),
      edit('rotateBy', [[pr(30)], 90]),
      edit('rotateBy', [[pr(30)], -90]),
      edit('rotateBy', [[pr(99)], 90]),
      edit('rotateBy', [[pr(99)], -90]),
      edit('rotateBy', [[pr(20)], 45]),
      edit('rotateBy', [[pr(20)], 450]),
      edit('rotateBy', [[pr(20)], -720]),
      edit('rotateBy', [[pr(20)], -900]),
      edit('rotateBy', [[pr(20)], 0.5]),
      edit('rotateBy', [[pr(20)], { $number: 'NaN' }]),
    ],
  },
  {
    name: 'rotate-by-follows-the-registry',
    covers: 'After a pages.rotated event replaces the registry, rotateBy starts from the new rotation.',
    plugins,
    snapshot: 'none',
    engine: { documents: { a: doc({ responses: answering }) } },
    steps: [
      start,
      open('a'),
      { op: 'emit', doc: 'a', event: { type: 'pages.rotated', pages: [pr(10)], origin: { kind: 'local', sessionId: 's1', sub: null }, layout: { pageCount: 3, pages: [page(10, 0, 180, 100, 200), three[1], three[2]] } } },
      edit('rotateBy', [[pr(10)], 90]),
    ],
  },
  {
    name: 'passthroughs',
    covers: 'setRotation, move and delete forward their arguments unchanged and return the engine promise, which settles with the engine answer.',
    plugins,
    snapshot: 'none',
    engine: { documents: { a: doc({ responses: answering }) } },
    steps: [start, open('a'), edit('setRotation', [[pr(10), pr(20)], 180]), edit('move', [[pr(20), pr(30)], { index: 0 }]), edit('delete', [[pr(30)]]), edit('move', [[], { index: -1 }])],
  },
  {
    name: 'refused-by-the-engine',
    covers:
      'Without doc.pages.assemble canEdit() is false, and every verb still calls the engine, whose PermissionDenied rejects the returned promise with its full shape.',
    plugins,
    snapshot: 'none',
    engine: {
      documents: {
        a: doc({
          security: { allows: ['doc.render'] },
          responses: ['pages.rotate', 'pages.move', 'pages.delete', 'pages.insertBlank', 'pages.insert'].map((path) => ({ path, error: denied() })),
        }),
      },
    },
    steps: [
      start,
      open('a'),
      edit('canEdit'),
      edit('rotateBy', [[pr(10)], 90]),
      edit('setRotation', [[pr(10)], 90]),
      edit('move', [[pr(10)], { index: 2 }]),
      edit('delete', [[pr(10)]]),
      edit('insertBlank'),
      edit('insertFromBytes', [{ $bytes: 'AQID' }]),
    ],
  },
  {
    name: 'deferred-and-aborted',
    covers: 'A deferred edit settles when the engine answers, and the verbs run one at a time: the delete reaches the engine only after the rotate settles.',
    plugins,
    snapshot: 'none',
    engine: {
      documents: {
        a: doc({
          responses: [
            { path: 'pages.rotate', defer: 'rotate' },
            { path: 'pages.delete', defer: 'delete' },
          ],
        }),
      },
    },
    steps: [
      start,
      open('a'),
      edit('rotateBy', [[pr(10)], 90]),
      edit('delete', [[pr(20)]]),
      { op: 'settle', label: 'rotate', value: { rotated: [10] } },
      { op: 'settle', label: 'delete', value: { deleted: [20] } },
    ],
  },
  {
    name: 'add-blank',
    covers:
      'insertBlank: append by default sized like the last page; ref placements land after/before the anchor sized like it; an index placement is forwarded verbatim and sized like its predecessor, clamped into the registry; explicit size and count win; a missing anchor rejects not-found.',
    plugins,
    snapshot: 'none',
    engine: { documents: { a: doc({ responses: answering }) } },
    steps: [
      start,
      open('a'),
      edit('insertBlank'),
      edit('insertBlank', [{}]),
      edit('insertBlank', [{ placement: { after: pr(20) } }]),
      edit('insertBlank', [{ placement: { before: pr(10) } }]),
      edit('insertBlank', [{ placement: { before: pr(30) } }]),
      edit('insertBlank', [{ placement: { index: 2 } }]),
      edit('insertBlank', [{ placement: { index: 0 } }]),
      edit('insertBlank', [{ placement: { index: -5 } }]),
      edit('insertBlank', [{ placement: { index: 10 } }]),
      edit('insertBlank', [{ size: { width: 612, height: 792 }, count: 3, placement: { after: pr(10) } }]),
      edit('insertBlank', [{ count: 2 }]),
      edit('insertBlank', [{ size: { width: 1, height: 2 } }]),
      edit('insertBlank', [{ placement: { after: pr(99) } }]),
      edit('insertBlank', [{ placement: { before: pr(99) } }]),
      edit('insertBlank', [{ placement: { index: 1, after: pr(99) } }]),
      edit('insertBlank', [{ placement: { after: pr(10), before: pr(30) } }]),
    ],
  },
  {
    name: 'add-blank-fractional-index',
    covers:
      'An index placement that is not an integer (or NaN) makes the predecessor lookup read a page that does not exist, which throws the TypeError the TypeScript throws; an explicit size skips the lookup.',
    plugins,
    snapshot: 'none',
    engine: { documents: { a: doc({ responses: answering }) } },
    steps: [
      start,
      open('a'),
      edit('insertBlank', [{ placement: { index: 1.5 } }]),
      edit('insertBlank', [{ placement: { index: { $number: 'NaN' } } }]),
      edit('insertBlank', [{ placement: { index: { $number: 'Infinity' } } }]),
      edit('insertBlank', [{ placement: { index: 1.5 }, size: { width: 3, height: 4 } }]),
    ],
  },
  {
    name: 'insert',
    covers: 'insertFromBytes forwards the bytes and the resolved placement index; a missing anchor rejects not-found before any engine call.',
    plugins,
    snapshot: 'none',
    engine: { documents: { a: doc({ responses: answering }) } },
    steps: [
      start,
      open('a'),
      edit('insertFromBytes', [{ $bytes: 'AQID' }]),
      edit('insertFromBytes', [{ $bytes: 'AQID' }, {}]),
      edit('insertFromBytes', [{ $bytes: 'AQID' }, { placement: { after: pr(20) } }]),
      edit('insertFromBytes', [{ $bytes: '' }, { placement: { before: pr(30) } }]),
      edit('insertFromBytes', [{ $bytes: 'AQID' }, { placement: { index: 7 } }]),
      edit('insertFromBytes', [{ $bytes: 'AQID' }, { placement: { after: pr(5) } }]),
    ],
  },
  {
    name: 'a-captured-capability-outlives-its-document',
    covers:
      'A capability captured before its document closed keeps its guarded handle and the last registry it saw: canEdit still answers, and each verb still reaches the engine, but its promise rejects instance-closed (the kernel no longer resolves the capability).',
    plugins,
    snapshot: 'none',
    engine: { documents: { a: doc({ responses: answering }) } },
    steps: [
      start,
      open('a'),
      { op: 'call', target: 'kernel', method: 'capability', args: [{ $token: 'page-edit' }, 'a'] },
      { op: 'call', target: 'documents', method: 'close', args: ['a'] },
      edit('canEdit'),
      { op: 'call', target: { ref: 0 } },
      { op: 'call', target: { ref: 1 }, args: [[pr(20)], 90] },
      { op: 'call', target: { ref: 5 }, args: [] },
      { op: 'call', target: { ref: 5 }, args: [{ placement: { index: 3 } }] },
      { op: 'call', target: { ref: 5 }, args: [{ placement: { after: pr(20) } }] },
    ],
  },
  {
    name: 'an-empty-registry',
    covers: 'A document with no pages: rotation reads as 0, a PON anchor is not found, and a blank page defaults to US Letter wherever it goes.',
    plugins,
    snapshot: 'none',
    engine: { documents: { a: doc({ pages: [], responses: answering }) } },
    steps: [
      start,
      open('a'),
      edit('rotateBy', [[pr(10)], -90]),
      edit('insertBlank'),
      edit('insertBlank', [{ placement: { index: 4 } }]),
      edit('insertBlank', [{ placement: { index: 0.5 } }]),
      edit('insertBlank', [{ placement: { after: pr(10) } }]),
      edit('insertFromBytes', [{ $bytes: 'AQID' }, { placement: { index: 0 } }]),
    ],
  },
  {
    name: 'rotate-by-groups-and-empty',
    covers:
      'rotateBy groups pages by their resulting rotation and makes one engine call per group, in first-seen order, resolving with the last answer; no pages rejects invalid-input without an engine call.',
    plugins,
    snapshot: 'none',
    engine: {
      documents: {
        a: doc({
          pages: [...three, page(40, 3, 0, 700, 800)],
          responses: [
            { path: 'pages.rotate', args: [[pr(10), pr(40)], 90], value: { rotated: 'first' } },
            { path: 'pages.rotate', args: [[pr(20)], 180], value: { rotated: 'second' } },
            { path: 'pages.rotate', args: [[pr(30)], 0], value: { rotated: 'third' } },
          ],
        }),
      },
    },
    steps: [start, open('a'), edit('rotateBy', [[pr(10), pr(20), pr(30), pr(40)], 90]), edit('rotateBy', [[], 90])],
  },
  {
    name: 'move-to-the-end',
    covers: "move with 'end' (or no placement) sends the registry's page count as the destination; ref placements resolve after or before their anchor.",
    plugins,
    snapshot: 'none',
    engine: { documents: { a: doc({ responses: answering }) } },
    steps: [
      start,
      open('a'),
      edit('move', [[pr(10)], 'end']),
      edit('move', [[pr(10)]]),
      edit('move', [[pr(30)], { after: pr(10) }]),
      edit('move', [[pr(10)], { before: pr(30) }]),
      edit('move', [[pr(10)], { before: pr(99) }]),
    ],
  },
  {
    name: 'insert-a-subset-of-another-pdf',
    covers:
      'insertFromBytes with pageIndexes opens the bytes beside the document (full scope, an id from the clock), lists its pages, extracts the chosen ones by ref, inserts the extract at the placement and closes the source. A missing index rejects not-found and still closes the source; a refused open rejects with the mapped error.',
    plugins,
    snapshot: 'none',
    engine: {
      defaultDocument: 'src',
      documents: {
        a: doc({ responses: answering }),
        src: {
          pages: [page(5, 0, 0, 10, 20), page(6, 1, 0, 30, 40), page(7, 2, 0, 50, 60)],
          responses: [{ path: 'pages.extract', value: { $bytes: 'BAUG' } }],
        },
      },
      responses: [
        { path: 'open', args: [{ kind: 'bytes', id: 'page-edit-source-0', bytes: { $bytes: 'ERET' } }, { scope: ['*'] }], times: 1, error: denied('doc.open') },
      ],
    },
    steps: [
      start,
      open('a'),
      edit('insertFromBytes', [{ $bytes: 'AQID' }, { pageIndexes: [2, 0], placement: { after: pr(10) } }]),
      edit('insertFromBytes', [{ $bytes: 'AQID' }, { pageIndexes: [] }]),
      edit('insertFromBytes', [{ $bytes: 'AQID' }, { pageIndexes: [1, 3] }]),
      edit('insertFromBytes', [{ $bytes: 'ERET' }, { pageIndexes: [0] }]),
    ],
  },
  {
    name: 'insert-a-subset-names-the-missing-index',
    covers:
      "The source id is Date.now() in base 36. A pageIndexes entry that is negative, fractional or not finite names no page and rejects not-found, the message spelling the index as String() does (1e+21, 1e-7 and all).",
    plugins,
    snapshot: 'none',
    engine: {
      defaultDocument: 'src',
      documents: {
        a: doc({ responses: answering }),
        src: {
          pages: [page(5, 0, 0, 10, 20), page(6, 1, 0, 30, 40), page(7, 2, 0, 50, 60)],
          responses: [{ path: 'pages.extract', value: { $bytes: 'BAUG' } }],
        },
      },
    },
    steps: [
      start,
      open('a'),
      { op: 'advance', ms: 1234567890 },
      edit('insertFromBytes', [{ $bytes: 'AQID' }, { pageIndexes: [1] }]),
      ...[
        -1,
        1.5,
        -0.5,
        { $number: 'Infinity' },
        { $number: '-Infinity' },
        { $number: 'NaN' },
        1e20,
        1e21,
        1e-7,
      ].map((index) => edit('insertFromBytes', [{ $bytes: 'AQID' }, { pageIndexes: [index] }])),
    ],
  },
  {
    name: 'rotate-by-a-page-without-a-rotation',
    covers:
      'A registry page with no rotation turns into NaN, which is its own Map key: the numbered pages still group apart from it, one engine call per group.',
    plugins,
    snapshot: 'none',
    engine: {
      documents: {
        a: doc({
          pages: [three[0], { ...three[1], rotation: undefined }, three[2]],
          responses: answering,
        }),
      },
    },
    steps: [start, open('a'), edit('rotateBy', [[pr(10), pr(20), pr(30)], 90])],
  },
  {
    name: 'insert-from-another-document',
    covers:
      "insertFromDocument extracts the pages from the other open document's handle and inserts the bytes at the placement ('end' by default); a document that is not open rejects not-found; a refused extract rejects with the mapped error.",
    plugins,
    snapshot: 'none',
    engine: {
      documents: {
        a: doc({ responses: answering }),
        b: doc({ responses: [{ path: 'pages.extract', args: [[pr(20)]], value: { $bytes: 'BwgJ' } }, { path: 'pages.extract', error: denied('doc.pages.extract') }] }),
      },
    },
    steps: [
      start,
      open('a'),
      open('b'),
      edit('insertFromDocument', ['b', [pr(20)]]),
      edit('insertFromDocument', ['b', [pr(20)], { placement: { index: 0 } }]),
      edit('insertFromDocument', ['zzz', [pr(20)]]),
      edit('insertFromDocument', ['b', [pr(30)]]),
    ],
  },
  {
    name: 'duplicate',
    covers:
      'duplicate extracts the pages and inserts the copy right after the last of them by default, or at the placement given; no pages rejects invalid-input before any engine call.',
    plugins,
    snapshot: 'none',
    engine: { documents: { a: doc({ responses: [...answering, { path: 'pages.extract', value: { $bytes: 'CgsM' } }] }) } },
    steps: [
      start,
      open('a'),
      edit('duplicate', [[pr(10), pr(20)]]),
      edit('duplicate', [[pr(30)], { placement: 'end' }]),
      edit('duplicate', [[pr(99)]]),
      edit('duplicate', [[]]),
      edit('extract', [[pr(10)]]),
    ],
  },
  {
    name: 'state-and-scope',
    covers: 'The plugin keeps an empty slice per document and has no capability before a document opens.',
    plugins,
    engine: { documents: { a: doc() } },
    steps: [edit('canEdit'), start, edit('canEdit'), open('a'), edit('canEdit')],
  },
];

export const suites: Suite[] = [
  { kind: 'scenario', file: 'plugins/page-edit.json', describes: 'packages/plugin/page-edit: PON-addressed page edits over the engine handle.', cases },
];
