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

const box = (w: number, h: number) => ({ left: 0, bottom: 0, right: w, top: h });
const page = (pon: number, index: number, rotation: number, width: number, height: number) => ({
  index,
  pageObjectNumber: pon,
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
  ok('pages.insertBlank', { pageObjectNumbers: [99] }),
  ok('pages.insert', { pageObjectNumbers: [77, 78] }),
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
      'rotateBy turns a relative turn into the absolute wire from the registry rotation, wrapping into [0, 360); an unknown pon counts as 0; any number is carried through the arithmetic.',
    plugins,
    snapshot: 'none',
    engine: { documents: { a: doc({ responses: answering }) } },
    steps: [
      start,
      open('a'),
      edit('rotateBy', [10, 90]),
      edit('rotateBy', [10, -90]),
      edit('rotateBy', [20, 90]),
      edit('rotateBy', [30, 90]),
      edit('rotateBy', [30, -90]),
      edit('rotateBy', [99, 90]),
      edit('rotateBy', [99, -90]),
      edit('rotateBy', [20, 45]),
      edit('rotateBy', [20, 450]),
      edit('rotateBy', [20, -720]),
      edit('rotateBy', [20, 0.5]),
      edit('rotateBy', [20, { $number: 'NaN' }]),
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
      { op: 'emit', doc: 'a', event: { type: 'pages.rotated', pageObjectNumbers: [10], layout: { pageCount: 3, pages: [page(10, 0, 180, 100, 200), three[1], three[2]] } } },
      edit('rotateBy', [10, 90]),
    ],
  },
  {
    name: 'passthroughs',
    covers: 'setRotation, move and delete forward their arguments unchanged and return the engine promise, which settles with the engine answer.',
    plugins,
    snapshot: 'none',
    engine: { documents: { a: doc({ responses: answering }) } },
    steps: [start, open('a'), edit('setRotation', [[10, 20], 180]), edit('move', [[20, 30], 0]), edit('delete', [[30]]), edit('move', [[], -1])],
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
      edit('rotateBy', [10, 90]),
      edit('setRotation', [[10], 90]),
      edit('move', [[10], 2]),
      edit('delete', [[10]]),
      edit('addBlank'),
      edit('insert', [{ $bytes: 'AQID' }]),
    ],
  },
  {
    name: 'deferred-and-aborted',
    covers: 'The verbs return the engine AbortablePromise itself: a deferred edit settles later, and aborting one logs the abort and rejects with AbortError.',
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
      edit('rotateBy', [10, 90]),
      edit('delete', [[20]]),
      { op: 'settle', label: 'rotate', value: { rotated: [10] } },
      { op: 'abort', promise: 3, reason: 'user cancelled' },
    ],
  },
  {
    name: 'add-blank',
    covers:
      'addBlank: append by default sized like the last page; PON placements land after/before the anchor sized like it; an index placement is forwarded verbatim and sized like its predecessor, clamped into the registry; explicit size and count win; a missing anchor throws.',
    plugins,
    snapshot: 'none',
    engine: { documents: { a: doc({ responses: answering }) } },
    steps: [
      start,
      open('a'),
      edit('addBlank'),
      edit('addBlank', [{}]),
      edit('addBlank', [{ placement: { after: 20 } }]),
      edit('addBlank', [{ placement: { before: 10 } }]),
      edit('addBlank', [{ placement: { before: 30 } }]),
      edit('addBlank', [{ placement: { index: 2 } }]),
      edit('addBlank', [{ placement: { index: 0 } }]),
      edit('addBlank', [{ placement: { index: -5 } }]),
      edit('addBlank', [{ placement: { index: 10 } }]),
      edit('addBlank', [{ size: { width: 612, height: 792 }, count: 3, placement: { after: 10 } }]),
      edit('addBlank', [{ count: 2 }]),
      edit('addBlank', [{ size: { width: 1, height: 2 } }]),
      edit('addBlank', [{ placement: { after: 99 } }]),
      edit('addBlank', [{ placement: { before: 99 } }]),
      edit('addBlank', [{ placement: { index: 1, after: 99 } }]),
      edit('addBlank', [{ placement: { after: 10, before: 30 } }]),
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
      edit('addBlank', [{ placement: { index: 1.5 } }]),
      edit('addBlank', [{ placement: { index: { $number: 'NaN' } } }]),
      edit('addBlank', [{ placement: { index: { $number: 'Infinity' } } }]),
      edit('addBlank', [{ placement: { index: 1.5 }, size: { width: 3, height: 4 } }]),
    ],
  },
  {
    name: 'insert',
    covers: 'insert forwards the bytes and the resolved placement index; a missing anchor throws before any engine call.',
    plugins,
    snapshot: 'none',
    engine: { documents: { a: doc({ responses: answering }) } },
    steps: [
      start,
      open('a'),
      edit('insert', [{ $bytes: 'AQID' }]),
      edit('insert', [{ $bytes: 'AQID' }, {}]),
      edit('insert', [{ $bytes: 'AQID' }, { placement: { after: 20 } }]),
      edit('insert', [{ $bytes: '' }, { placement: { before: 30 } }]),
      edit('insert', [{ $bytes: 'AQID' }, { placement: { index: 7 } }]),
      edit('insert', [{ $bytes: 'AQID' }, { placement: { after: 5 } }]),
    ],
  },
  {
    name: 'a-captured-capability-outlives-its-document',
    covers:
      'A capability captured before its document closed keeps its handle and the last registry it saw: the verbs still reach the engine (the kernel no longer resolves the capability).',
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
      { op: 'call', target: { ref: 1 }, args: [20, 90] },
      { op: 'call', target: { ref: 5 }, args: [] },
      { op: 'call', target: { ref: 5 }, args: [{ placement: { index: 3 } }] },
      { op: 'call', target: { ref: 5 }, args: [{ placement: { after: 20 } }] },
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
      edit('rotateBy', [10, -90]),
      edit('addBlank'),
      edit('addBlank', [{ placement: { index: 4 } }]),
      edit('addBlank', [{ placement: { index: 0.5 } }]),
      edit('addBlank', [{ placement: { after: 10 } }]),
      edit('insert', [{ $bytes: 'AQID' }, { placement: { index: 0 } }]),
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
