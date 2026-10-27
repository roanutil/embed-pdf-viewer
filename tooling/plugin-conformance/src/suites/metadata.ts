/** Standard and custom metadata through the shipping lazy mirrors and guarded services. */
import { MetadataToken, metadataPlugin } from '../../../../packages/plugin/metadata/src/index';
import { registerPlugin, type ScenarioCase } from '../scenario';
import type { Suite } from '../vectors';
import {
  meta,
  doc,
  reads,
  start,
  open,
  close,
  call,
  custom,
  hook,
  fact,
  customFact,
  emit,
  settle,
  result,
  customResult,
  local,
  remote,
  pages,
} from './metadata-current-fixtures';
registerPlugin('metadata', { factory: () => metadataPlugin(), tokens: [MetadataToken] });
const plugins = [{ factory: 'metadata' }];
const denied = {
  name: 'PermissionDenied',
  message: 'permission denied (local): doc.metadata.modify',
  required: 'doc.metadata.modify',
  context: 'local',
  code: 'Forbidden',
  details: { required: 'doc.metadata.modify', context: 'local' },
};
const gone = { name: 'EngineError', message: 'gone', code: 'Unknown' };
const cases: ScenarioCase[] = [
  {
    name: 'seeds-from-a-read',
    covers:
      'Opening and synchronous getters do no reads. ensureLoaded starts both reads, keeps snapshots null until settled, and emits resynced separately.',
    plugins,
    engine: {
      documents: {
        a: doc([
          { path: 'metadata.get', args: [], defer: 'read' },
          { path: 'metadata.custom.get', args: [], defer: 'custom' },
        ]),
      },
    },
    steps: [
      start,
      open(),
      call('getSnapshot', []),
      call('getStatus', []),
      custom('getSnapshot', []),
      custom('getStatus', []),
      hook('onResynced'),
      hook('custom.onResynced'),
      call('ensureLoaded', []),
      call('getSnapshot', []),
      settle('read', meta({ title: 'Report' })),
      call('getSnapshot', []),
      custom('getSnapshot', []),
      settle('custom', { Dept: 'Ops' }),
      call('getStatus', []),
      custom('getStatus', []),
      call('ensureLoaded', []),
      custom('getSnapshot', []),
    ],
  },
  {
    name: 'a-failed-seed-read-is-ignored',
    covers:
      'An on-demand read failure rejects its caller, leaves null metadata/error status, and reports nothing; custom success stays ready.',
    plugins,
    engine: {
      documents: {
        a: doc([
          {
            path: 'metadata.get',
            args: [],
            error: { name: 'EngineError', message: 'closed', code: 'DocClosed' },
          },
          ...reads().slice(1),
        ]),
      },
    },
    steps: [
      start,
      open(),
      call('ensureLoaded', []),
      call('getSnapshot', []),
      call('getStatus', []),
      custom('getStatus', []),
    ],
  },
  {
    name: 'live-updates-from-the-event-stream',
    covers:
      'Both origins fold confirmed standard/custom facts. Unrelated facts are ignored; close releases the instance and reopen begins idle.',
    plugins,
    engine: { documents: { a: doc(reads(meta({ title: 'Seed' }))) } },
    steps: [
      start,
      open(),
      call('ensureLoaded', []),
      emit(fact(meta({ title: 'Remote edit' }))),
      call('getSnapshot', []),
      emit(customFact({ Dept: 'Ops' }, local)),
      custom('getSnapshot', []),
      emit({
        type: 'pages.reordered',
        origin: local,
        pages: [{ kind: 'objectNumber', objectNumber: 1 }],
        layout: { pageCount: 1, pages, namedPages: [] },
        meta: result(meta()).meta,
      }),
      call('getSnapshot', []),
      close,
      open(),
      call('getSnapshot', []),
      call('getStatus', []),
      call('ensureLoaded', []),
      call('getSnapshot', []),
    ],
  },
  {
    name: 'update-writes-through-and-sets',
    covers:
      'The three-state standard patch reaches the engine unchanged. Only its confirmed event changes the mirror, before the structured write result settles.',
    plugins,
    engine: {
      documents: {
        a: doc([
          ...reads(),
          {
            path: 'metadata.update',
            args: [{ title: 'New', author: null }],
            value: result(meta({ title: 'New' })),
            events: [fact(meta({ title: 'New' }), local)],
          },
        ]),
      },
    },
    steps: [
      start,
      open(),
      call('ensureLoaded', []),
      hook('onUpdated'),
      call('update', [{ title: 'New', author: null }]),
      call('getSnapshot', []),
    ],
  },
  {
    name: 'update-refused',
    covers: 'An engine refusal rejects update and preserves the complete ready snapshots.',
    plugins,
    engine: {
      documents: {
        a: doc([
          ...reads(meta({ title: 'Kept' })),
          { path: 'metadata.update', args: [{ title: 'x' }], error: denied },
        ]),
      },
    },
    steps: [
      start,
      open(),
      call('ensureLoaded', []),
      call('update', [{ title: 'x' }]),
      call('getSnapshot', []),
      call('getStatus', []),
    ],
  },
  {
    name: 'reload',
    covers:
      'refresh re-reads only the standard mirror; a failed refresh rejects and leaves its last value with error status.',
    plugins,
    engine: {
      documents: {
        a: doc([
          { path: 'metadata.get', args: [], times: 1, value: meta({ title: 'First' }) },
          { path: 'metadata.get', args: [], times: 1, value: meta({ title: 'Second' }) },
          { path: 'metadata.get', args: [], error: { ...gone, code: 'DocClosed' } },
          ...reads().slice(1),
        ]),
      },
    },
    steps: [
      start,
      open(),
      call('ensureLoaded', []),
      hook('onResynced'),
      call('refresh', []),
      call('getSnapshot', []),
      call('refresh', []),
      call('getSnapshot', []),
      call('getStatus', []),
      custom('getStatus', []),
    ],
  },
  {
    name: 'can-edit',
    covers:
      'canUpdate is the doc.metadata.modify authority for both write surfaces, wildcard included, without loading.',
    plugins,
    engine: {
      documents: {
        a: doc([], { security: { allows: ['doc.metadata.modify'] } }),
        b: doc([], { security: { allows: ['doc.render'] } }),
        c: doc([], { security: { allows: '*' } }),
      },
    },
    steps: [
      start,
      open(),
      open('b'),
      open('c'),
      call('canUpdate', [], 'a'),
      call('canUpdate', [], 'b'),
      call('canUpdate', [], 'c'),
    ],
  },
  {
    name: 'update-without-the-capability-is-refused-locally',
    covers:
      'Both writes reject permission-denied before any engine write and preserve snapshots/status.',
    plugins,
    engine: {
      documents: {
        a: doc(reads(meta({ title: 'Kept' }), { Dept: 'Ops' }), {
          security: { allows: ['doc.render'] },
        }),
      },
    },
    steps: [
      start,
      open(),
      call('ensureLoaded', []),
      call('update', [{ title: 'x' }]),
      custom('update', [{ Dept: 'Other' }]),
      call('getSnapshot', []),
      call('getStatus', []),
      custom('getSnapshot', []),
      custom('getStatus', []),
    ],
  },
  {
    name: 'a-forbidden-seed-read',
    covers:
      'An explicit first load may be forbidden; plain refresh failure is error; a later successful refresh recovers ready.',
    plugins,
    engine: {
      documents: {
        a: doc([
          {
            path: 'metadata.get',
            args: [],
            times: 1,
            error: {
              ...denied,
              message: 'permission denied (local): doc.metadata.read',
              required: 'doc.metadata.read',
              details: { required: 'doc.metadata.read', context: 'local' },
            },
          },
          { path: 'metadata.get', args: [], times: 1, error: gone },
          { path: 'metadata.get', args: [], value: meta({ title: 'Back' }) },
          ...reads().slice(1),
        ]),
      },
    },
    steps: [
      start,
      open(),
      call('ensureLoaded', []),
      call('getStatus', []),
      call('getSnapshot', []),
      call('refresh', []),
      call('getStatus', []),
      call('refresh', []),
      call('getStatus', []),
      call('getSnapshot', []),
    ],
  },
  {
    name: 'status-while-the-seed-read-is-in-flight',
    covers:
      'Loading is explicit. Concurrent ensureLoaded shares both reads; refresh joins the promise and queues one standard re-read.',
    plugins,
    engine: {
      documents: {
        a: doc([
          { path: 'metadata.get', args: [], times: 1, defer: 'read' },
          { path: 'metadata.get', args: [], times: 1, defer: 'refresh' },
          { path: 'metadata.custom.get', args: [], defer: 'custom' },
        ]),
      },
    },
    steps: [
      start,
      open(),
      call('getStatus', []),
      call('ensureLoaded', []),
      call('getStatus', []),
      call('ensureLoaded', []),
      call('refresh', []),
      settle('read', meta({ title: 'Once' })),
      call('getStatus', []),
      custom('getStatus', []),
      settle('custom', {}),
      settle('refresh', meta({ title: 'Refreshed' })),
      call('getSnapshot', []),
      custom('getStatus', []),
    ],
  },
  {
    name: 'an-event-during-the-seed-read-wins',
    covers: 'New standard and custom confirmed values win over both older in-flight load answers.',
    plugins,
    engine: {
      documents: {
        a: doc([
          { path: 'metadata.get', args: [], defer: 'read' },
          { path: 'metadata.custom.get', args: [], defer: 'custom' },
        ]),
      },
    },
    steps: [
      start,
      open(),
      call('ensureLoaded', []),
      emit(fact(meta({ title: 'Event' }))),
      emit(customFact({ Dept: 'New' })),
      settle('read', meta({ title: 'Stale read' })),
      settle('custom', { Dept: 'Old' }),
      call('getSnapshot', []),
      call('getStatus', []),
      custom('getSnapshot', []),
      custom('getStatus', []),
    ],
  },
  {
    name: 'closing-during-the-seed-read',
    covers:
      'Close cancels the explicit load, suppresses late results/callbacks, and reopen starts with fresh idle mirrors.',
    plugins,
    engine: {
      documents: {
        a: doc([
          { path: 'metadata.get', args: [], defer: 'read' },
          { path: 'metadata.custom.get', args: [], defer: 'custom' },
        ]),
      },
    },
    steps: [
      start,
      open(),
      hook('onResynced'),
      hook('custom.onResynced'),
      call('ensureLoaded', []),
      close,
      settle('read', meta({ title: 'Late' })),
      settle('custom', { Late: 'old' }),
      open(),
      call('getSnapshot', []),
      custom('getSnapshot', []),
      call('getStatus', []),
      custom('getStatus', []),
    ],
  },
  {
    name: 'on-updated-reports-each-confirmed-change',
    covers:
      'Loads emit resynced; facts emit updated with full previous/changedKeys/origin. Custom key reordering compares equal; unsubscribing silences listeners.',
    plugins,
    engine: { documents: { a: doc(reads(meta({ title: 'Seed' }), { A: '1', B: '2' })) } },
    steps: [
      start,
      open(),
      hook('onUpdated', 'updated'),
      hook('custom.onUpdated', 'customUpdated'),
      hook('onResynced', 'resynced'),
      hook('custom.onResynced', 'customResynced'),
      call('ensureLoaded', []),
      emit(fact(meta({ title: 'Local' }), local)),
      emit(customFact({ B: '2', A: '1' }, remote)),
      emit(fact(meta({ title: 'Local' }), remote)),
      emit(customFact({ B: '3', C: '' }, remote)),
      emit(fact(meta({ title: 'Sparse' }), local)),
      { op: 'call', target: { ref: 0 } },
      { op: 'call', target: { ref: 1 } },
      emit(fact(meta({ title: 'Unheard' }), local)),
      emit(customFact({ Unheard: '1' }, local)),
      call('getSnapshot', []),
      custom('getSnapshot', []),
    ],
  },
  {
    name: 'changed-keys-beyond-the-info-dict',
    covers:
      'Custom keys are flat strings; shallow comparison records changed, added and removed keys and ignores equal/reordered entries.',
    plugins,
    engine: {
      documents: {
        a: doc(reads(meta(), { pages: '3', ratio: '1.5', extra: 'object', list: 'array' })),
      },
    },
    steps: [
      start,
      open(),
      call('ensureLoaded', []),
      hook('custom.onUpdated', 'updated'),
      emit(customFact({ pages: '3', ratio: '2.5', extra: 'object', list: 'array' }, local)),
      emit(customFact({ pages: '4', ratio: '2.5', extra: 'x', list: 'y' }, local)),
      emit(customFact({ ratio: '2.5', extra: '', added: 'yes' }, remote)),
    ],
  },
  {
    name: 'custom-write-confirms-before-settlement',
    covers:
      'Custom patches set, clear and omit independently of standard fields; deferred engine confirmation fires before the returned result settles.',
    plugins,
    engine: {
      documents: {
        a: doc([
          ...reads(meta({ title: 'Kept' }), { A: '1', B: '2', C: '3' }),
          {
            path: 'metadata.custom.update',
            args: [{ A: '', B: null }],
            defer: 'write',
            events: [customFact({ A: '', C: '3' }, local)],
          },
        ]),
      },
    },
    steps: [
      start,
      open(),
      call('ensureLoaded', []),
      hook('custom.onUpdated'),
      custom('update', [{ A: '', B: null }]),
      custom('getSnapshot', []),
      settle('write', customResult({ A: '', C: '3' })),
      custom('getSnapshot', []),
      call('getSnapshot', []),
    ],
  },
  {
    name: 'standard-write-does-not-project-unconfirmed-result',
    covers: 'A returned engine value alone does not update the mirror; only a confirmed fact does.',
    plugins,
    engine: {
      documents: {
        a: doc([
          ...reads(meta({ title: 'Kept' })),
          {
            path: 'metadata.update',
            args: [{ title: 'Unconfirmed' }],
            value: result(meta({ title: 'Unconfirmed' })),
          },
        ]),
      },
    },
    steps: [
      start,
      open(),
      call('ensureLoaded', []),
      hook('onUpdated'),
      call('update', [{ title: 'Unconfirmed' }]),
      call('getSnapshot', []),
      emit(fact(meta({ title: 'Confirmed' }), local)),
      call('getSnapshot', []),
    ],
  },
  {
    name: 'custom-refresh-and-refusal',
    covers: 'Custom refresh and engine write refusal leave the standard mirror intact.',
    plugins,
    engine: {
      documents: {
        a: doc([
          ...reads(meta({ title: 'Kept' }), { A: '1' }).map((r) => ({ ...r, times: 1 })),
          { path: 'metadata.custom.get', args: [], value: { A: '2' } },
          { path: 'metadata.custom.update', args: [{ A: null }], error: gone },
        ]),
      },
    },
    steps: [
      start,
      open(),
      call('ensureLoaded', []),
      hook('custom.onResynced'),
      custom('refresh', []),
      custom('getSnapshot', []),
      custom('update', [{ A: null }]),
      custom('getSnapshot', []),
      custom('getStatus', []),
      call('getSnapshot', []),
    ],
  },
];
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/metadata.json',
    describes: 'packages/plugin/metadata: lazy standard and custom Info-dict mirrors.',
    cases,
  },
];
