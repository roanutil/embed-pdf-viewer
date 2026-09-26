/**
 * `packages/plugin/metadata`. Replayed by
 * `crates/plugins/metadata/tests/conformance.rs`.
 */
import { MetadataToken, metadataPlugin } from '../../../../packages/plugin/metadata/src/index';

import type { FakeDocumentSpec } from '../fake-engine';
import { registerPlugin, type ScenarioCase, type Step } from '../scenario';
import type { Suite } from '../vectors';

registerPlugin('metadata', { factory: () => metadataPlugin(), tokens: [MetadataToken] });

const box = { left: 0, bottom: 0, right: 600, top: 800 };
const pages = [
  { index: 0, ref: { kind: 'objectNumber', pageObjectNumber: 1 }, label: null, size: { width: 600, height: 800 }, rotation: 0, userUnit: 1, boxes: { media: box, crop: box } },
];
const meta = (over: Record<string, unknown> = {}) => ({
  title: null,
  author: null,
  subject: null,
  keywords: null,
  producer: null,
  creator: null,
  created: null,
  modified: null,
  trapped: 'unknown',
  custom: {},
  ...over,
});
const doc = (over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec => ({ pages, ...over });

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const open = (id: string): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id, bytes: { $bytes: '' } }],
});
const call = (method: string, args: unknown[] = [], doc = 'a'): Step => ({
  op: 'call',
  target: { token: 'metadata', doc },
  method,
  args,
});

const plugins = [{ factory: 'metadata' }];
/** Engine events carry where they came from; the plugin projects it onto `ChangeOrigin`. */
const local = { kind: 'local', sessionId: 's1', sub: null };
const remote = { kind: 'remote', sessionId: 's2', sub: 'u-9' };
const EDIT = { allows: ['doc.metadata.modify'] };

const cases: ScenarioCase[] = [
  {
    name: 'seeds-from-a-read',
    covers: 'The controller reads the Info dict once when the document connects and sets it; getSnapshot() is null until then.',
    plugins,
    engine: {
      documents: {
        a: doc({ responses: [{ path: 'metadata.read', defer: 'read' }] }),
      },
    },
    steps: [
      start,
      open('a'),
      call('getSnapshot'),
      { op: 'settle', label: 'read', value: meta({ title: 'Report', custom: { Dept: 'Ops' } }) },
      call('getSnapshot'),
    ],
  },
  {
    name: 'a-failed-seed-read-is-ignored',
    covers: 'A read that rejects (a closed document, an aborted read) leaves the state null and reports nothing.',
    plugins,
    engine: {
      documents: {
        a: doc({ responses: [{ path: 'metadata.read', error: { name: 'EngineError', message: 'closed', code: 'DocClosed' } }] }),
      },
    },
    steps: [start, open('a'), call('getSnapshot')],
  },
  {
    name: 'live-updates-from-the-event-stream',
    covers:
      'A metadata.updated event, own or remote, replaces the state with the metadata it carries; any other event is ignored; closing the document unsubscribes.',
    plugins,
    engine: { documents: { a: doc({ responses: [{ path: 'metadata.read', value: meta({ title: 'Seed' }) }] }) } },
    steps: [
      start,
      open('a'),
      { op: 'emit', doc: 'a', event: { type: 'metadata.updated', metadata: meta({ title: 'Remote edit' }), origin: remote } },
      call('getSnapshot'),
      { op: 'emit', doc: 'a', event: { type: 'annotations.created', page: { kind: 'objectNumber', pageObjectNumber: 1 }, origin: local } },
      call('getSnapshot'),
      { op: 'call', target: 'documents', method: 'close', args: ['a'] },
    ],
  },
  {
    name: 'update-writes-through-and-sets',
    covers:
      'update(patch) sends the three-state patch to the engine and resolves with its result once the confirmed metadata.updated event has set the snapshot.',
    plugins,
    engine: {
      documents: {
        a: doc({
          security: EDIT,
          responses: [
            { path: 'metadata.read', value: meta() },
            { path: 'metadata.update', args: [{ title: 'New', author: null }], value: { metadata: meta({ title: 'New' }), cache: null } },
          ],
        }),
      },
    },
    steps: [
      start,
      open('a'),
      call('update', [{ title: 'New', author: null }]),
      call('getSnapshot'),
      { op: 'emit', doc: 'a', event: { type: 'metadata.updated', metadata: meta({ title: 'New' }), origin: local } },
      call('getSnapshot'),
    ],
  },
  {
    name: 'update-refused',
    covers: 'An engine refusal rejects update() with the engine error and changes nothing.',
    plugins,
    engine: {
      documents: {
        a: doc({
          security: EDIT,
          responses: [
            { path: 'metadata.read', value: meta({ title: 'Kept' }) },
            {
              path: 'metadata.update',
              error: { name: 'PermissionDenied', message: 'permission denied (local): doc.metadata.modify', required: 'doc.metadata.modify', context: 'local' },
            },
          ],
        }),
      },
    },
    steps: [start, open('a'), call('update', [{ title: 'x' }]), call('getSnapshot')],
  },
  {
    name: 'reload',
    covers: 'refresh() re-reads and sets; a failed refresh rejects.',
    plugins,
    engine: {
      documents: {
        a: doc({
          responses: [
            { path: 'metadata.read', times: 1, value: meta({ title: 'First' }) },
            { path: 'metadata.read', times: 1, value: meta({ title: 'Second' }) },
            { path: 'metadata.read', error: { name: 'EngineError', message: 'gone', code: 'DocClosed' } },
          ],
        }),
      },
    },
    steps: [start, open('a'), call('refresh'), call('getSnapshot'), call('refresh'), call('getSnapshot')],
  },
  {
    name: 'can-edit',
    covers: 'canEdit() is the doc.metadata.modify authority, wildcard included.',
    plugins,
    engine: {
      documents: {
        a: doc({ security: { allows: ['doc.metadata.modify'] }, responses: [{ path: 'metadata.read', value: meta() }] }),
        b: doc({ security: { allows: ['doc.render'] }, responses: [{ path: 'metadata.read', value: meta() }] }),
        c: doc({ security: { allows: '*' }, responses: [{ path: 'metadata.read', value: meta() }] }),
      },
    },
    steps: [start, open('a'), open('b'), open('c'), call('canEdit', [], 'a'), call('canEdit', [], 'b'), call('canEdit', [], 'c')],
  },
];

export const suites: Suite[] = [
  { kind: 'scenario', file: 'plugins/metadata.json', describes: 'packages/plugin/metadata: reactive Info-dict state.', cases },
];
