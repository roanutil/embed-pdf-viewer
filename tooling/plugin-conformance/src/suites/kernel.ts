/**
 * K0: `packages/core/main`, the kernel. Replayed by
 * `crates/epdf-kernel/tests/kernel_conformance.rs`.
 *
 * Every case uses probes (`probe.ts`) rather than real plugins, so a failure
 * here is the kernel's and nobody else's.
 */
import '../probe';

import type { ProbeConfig } from '../probe';
import type { ScenarioCase, Step } from '../scenario';
import type { FakeDocumentSpec } from '../fake-engine';
import type { Suite } from '../vectors';

const box = { left: 0, bottom: 0, right: 600, top: 800 };
const page = (pon: number, index: number) => ({
  index,
  ref: { kind: 'objectNumber', pageObjectNumber: pon },
  label: null,
  size: { width: 600, height: 800 },
  rotation: 0,
  userUnit: 1,
  boxes: { media: { ...box }, crop: { ...box } },
});
const doc = (over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec => ({
  pages: [page(1, 0)],
  ...over,
});
const bytes = (id: string, extra: Record<string, unknown> = {}) => ({
  kind: 'bytes',
  id,
  bytes: { $bytes: '' },
  ...extra,
});
const probe = (config: ProbeConfig) => ({ factory: 'probe', config });

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const destroy: Step = { op: 'call', target: 'kernel', method: 'destroy' };
const open = (input: unknown, options?: unknown): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args: options === undefined ? [input] : [input, options],
});
const docs = (method: string, ...args: unknown[]): Step => ({ op: 'call', target: 'documents', method, args });
const kernel = (method: string, ...args: unknown[]): Step => ({ op: 'call', target: 'kernel', method, args });
const cap = (token: string, method: string, args: unknown[] = [], docId?: string): Step => ({
  op: 'call',
  target: docId === undefined ? { token } : { token, doc: docId },
  method,
  args,
});
const settle = (label: string, value?: unknown): Step => ({ op: 'settle', label, value });
const fail = (label: string, message: string, name = 'Error'): Step => ({
  op: 'settle',
  label,
  error: { name, message },
});
const listing: Step[] = [
  docs('list'),
  docs('getOrder'),
  docs('getCount'),
  docs('getActiveId'),
];

/** A document-scoped probe with every lifecycle member switched on. */
const docProbe = (id: string, extra: Partial<ProbeConfig> = {}): ProbeConfig => ({
  id,
  scope: 'document',
  token: id,
  initialState: { n: 0 },
  reduce: { SET: 'replace', MERGE: 'merge', SAME: 'same' },
  init: true,
  effects: { onAction: ['@@core/document-added', '@@core/active-changed'], cleanup: true, watchState: true },
  ...extra,
});

const cases: ScenarioCase[] = [
  {
    name: 'open-a-document',
    covers:
      'The ready path end to end: slot reserved synchronously as loading, the engine open, pages.list, bring-up (slices, init, effects) in plugin order, and the one commit that swaps the pending slot for the document.',
    plugins: [probe(docProbe('alpha')), probe({ id: 'workspace', token: 'ws', init: true, effects: { onAction: ['@@core/document-opening'] } })],
    engine: { documents: { a: doc({ pages: [page(1, 0), page(7, 1)] }) } },
    steps: [start, open(bytes('a'), { name: 'Report' }), ...listing, docs('get', 'a'), docs('has', 'a'), docs('has', 'zzz'), docs('get', 'zzz')],
  },
  {
    name: 'tabs-keep-request-order',
    covers:
      'Two opens whose engine calls complete in the reverse order: the tab strip keeps request order, and activation was decided at request time (the second open activates, the first does not steal focus back).',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: { a: doc(), b: doc({ pages: [page(3, 0)] }) },
      responses: [
        { path: 'open', args: [bytes('a'), {}], defer: 'open-a', times: 1 },
        { path: 'open', args: [bytes('b'), {}], defer: 'open-b', times: 1 },
      ],
    },
    steps: [
      start,
      open(bytes('a')),
      open(bytes('b')),
      ...listing,
      settle('open-b', { $document: 'b' }),
      ...listing,
      settle('open-a', { $document: 'a' }),
      ...listing,
    ],
  },
  {
    name: 'activate-false-and-first-open',
    covers:
      '`activate: false` leaves the active tab alone unless there is none, in which case the first tab is always selected.',
    plugins: [probe(docProbe('alpha'))],
    engine: { documents: { a: doc(), b: doc() } },
    steps: [start, open(bytes('a'), { activate: false }), docs('getActiveId'), open(bytes('b'), { activate: false }), docs('getActiveId')],
  },
  {
    name: 'open-all',
    covers:
      '`openAll` reserves every slot at once in array order, selects exactly the `active` entry (else the first), and turns a failure into tab state rather than a rejection.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: { a: doc(), b: doc(), c: doc() },
      responses: [{ path: 'open', args: [bytes('c'), {}], error: { name: 'EngineError', message: 'broken file', code: 'Format' } }],
    },
    steps: [
      start,
      docs('openAll', [
        { source: bytes('a'), name: 'A' },
        { source: bytes('b'), active: true },
        { source: bytes('c') },
      ]),
      ...listing,
      docs('openAll', []),
    ],
  },
  {
    name: 'open-all-defaults-to-first',
    covers: 'With no `active` entry the first document is selected.',
    plugins: [probe(docProbe('alpha'))],
    engine: { documents: { a: doc(), b: doc() } },
    steps: [start, docs('openAll', [{ source: bytes('a') }, { source: bytes('b') }]), docs('getActiveId')],
  },
  {
    name: 'close-while-opening',
    covers:
      'close() during the engine open: the tab disappears synchronously, open() rejects with the typed cancellation, and a handle that lands afterwards is closed rather than leaked.',
    plugins: [probe(docProbe('alpha'))],
    engine: { documents: { a: doc() }, responses: [{ path: 'open', defer: 'open-a' }] },
    steps: [start, open(bytes('a')), docs('close', 'a'), ...listing, settle('open-a', { $document: 'a' }), docs('close', 'a')],
  },
  {
    name: 'close-during-pages-list',
    covers:
      'close() while pages.list is in flight aborts the engine call (the kernel retains it for exactly this) and the open rejects as cancelled.',
    plugins: [probe(docProbe('alpha'))],
    engine: { documents: { a: doc({ responses: [{ path: 'pages.list', defer: 'list-a' }] }) } },
    steps: [start, open(bytes('a')), docs('close', 'a'), ...listing],
  },
  {
    name: 'close-during-init',
    covers:
      'close() while a plugin init is pending: inits are joined, not raced, so close waits for the init; the session then disposes and open rejects as cancelled.',
    plugins: [probe(docProbe('alpha', { init: { defer: 'init' } }))],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), docs('close', 'a'), ...listing, settle('init')],
  },
  {
    name: 'open-failure-parks-an-error-tab',
    covers:
      'An engine refusal after the slot is reserved: rollback, then an `error` tab that is closable and reopenable, with the rejection carried on the pending slot.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: { a: doc() },
      responses: [{ path: 'open', times: 1, error: { name: 'EngineError', message: 'bad xref', code: 'Format' } }],
    },
    steps: [start, open(bytes('a')), ...listing, docs('get', 'a'), open(bytes('a')), docs('close', 'a'), ...listing, open(bytes('a')), ...listing],
  },
  {
    name: 'bring-up-failure-rolls-back',
    covers:
      'A plugin init that throws after the handle is open: the session scope unwinds (slices removed, the handle closed) and the tab parks as `error`; the document was never ready.',
    plugins: [probe(docProbe('alpha')), probe(docProbe('beta', { init: { rejects: { message: 'init broke' } }, requires: ['alpha'] }))],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), ...listing, docs('get', 'a')],
  },
  {
    name: 'effect-setup-failure-rolls-back',
    covers: 'Effect setup is part of the transaction: a throwing effects() rolls the open back like a failed init.',
    plugins: [probe(docProbe('alpha', { effects: { throws: { message: 'effects broke' }, cleanup: true } }))],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), ...listing],
  },
  {
    name: 'pages-list-failure',
    covers: 'pages.list rejecting is an open failure like any other.',
    plugins: [probe(docProbe('alpha'))],
    engine: { documents: { a: doc({ responses: [{ path: 'pages.list', error: { name: 'EngineError', message: 'no pages', code: 'Format' } }] }) } },
    steps: [start, open(bytes('a')), ...listing],
  },
  {
    name: 'duplicate-open-refuses',
    covers: 'Opening an id that already has a session throws synchronously and changes nothing.',
    plugins: [probe(docProbe('alpha'))],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), open(bytes('a')), ...listing],
  },
  {
    name: 'password-locked-then-unlocked',
    covers:
      'A handle whose prompt is `required` parks as `locked` before pages.list; a wrong password rejects and changes nothing; the right one runs bring-up and promotes to ready.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: {
        a: doc({
          security: { passwordPrompt: { state: 'required' } },
          responses: [
            { path: 'security.unlock', args: [{ password: 'wrong' }], error: { name: 'EngineError', message: 'incorrect password', code: 'DocPasswordIncorrect' } },
            { path: 'security.unlock', args: [{ password: 'right' }], value: { security: {} } },
          ],
        }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      ...listing,
      docs('get', 'a'),
      { op: 'call', target: { token: 'alpha', doc: 'a' }, method: 'state' },
      kernel('tryCapability', { $token: 'alpha' }, 'a'),
      docs('unlock', 'a', { password: 'wrong' }),
      docs('get', 'a'),
      docs('unlock', 'a', { password: 'right' }),
      ...listing,
      docs('unlock', 'a', { password: 'right' }),
    ],
  },
  {
    name: 'password-provided-is-remembered',
    covers: 'A password supplied at open, in the options or on the input, marks the locked tab `passwordProvided` (the "incorrect" copy).',
    plugins: [],
    engine: {
      documents: {
        a: doc({ security: { passwordPrompt: { state: 'required' } } }),
        b: doc({ security: { passwordPrompt: { state: 'required' } } }),
        c: doc({ security: { passwordPrompt: { state: 'required' } } }),
      },
    },
    steps: [start, open(bytes('a'), { password: 'x' }), open(bytes('b', { password: 'y' })), open(bytes('c')), docs('list')],
  },
  {
    name: 'unlock-concurrency-and-cancellation',
    covers:
      'A second unlock while one is in flight refuses (one transition at a time); closing during an unlock cancels it; unlock on an unknown document refuses.',
    plugins: [],
    engine: {
      documents: {
        a: doc({ security: { passwordPrompt: { state: 'required' } }, responses: [{ path: 'security.unlock', defer: 'unlock' }] }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      docs('unlock', 'a', { password: 'p' }),
      docs('unlock', 'a', { password: 'p' }),
      docs('close', 'a'),
      docs('unlock', 'nope', { password: 'p' }),
    ],
  },
  {
    name: 'unlock-then-bring-up-fails',
    covers: 'Past the password, a failure is an open failure: rollback and an `error` tab.',
    plugins: [],
    engine: {
      documents: {
        a: doc({
          security: { passwordPrompt: { state: 'required' } },
          responses: [
            { path: 'security.unlock', value: {} },
            { path: 'pages.list', error: { name: 'EngineError', message: 'gone', code: 'Format' } },
          ],
        }),
      },
    },
    steps: [start, open(bytes('a')), docs('unlock', 'a', { password: 'p' }), ...listing],
  },
  {
    name: 'unlock-then-close-during-pages-list',
    covers: 'Closing after the password but before bring-up commits cancels the unlock.',
    plugins: [],
    engine: {
      documents: {
        a: doc({
          security: { passwordPrompt: { state: 'required' } },
          responses: [
            { path: 'security.unlock', value: {} },
            { path: 'pages.list', defer: 'list' },
          ],
        }),
      },
    },
    steps: [start, open(bytes('a')), docs('unlock', 'a', { password: 'p' }), docs('close', 'a'), ...listing],
  },
  {
    name: 'thunk-source',
    covers:
      'A thunk source reserves a ticket slot (`pending:N`), runs under the loading tab with the cancellation signal, and the slot is re-keyed to the real id atomically once the input names one.',
    plugins: [probe(docProbe('alpha'))],
    engine: { documents: { real: doc() } },
    steps: [
      start,
      open({ $fn: 'source', defer: 'fetch' }),
      ...listing,
      settle('fetch', bytes('real')),
      ...listing,
    ],
  },
  {
    name: 'thunk-source-failures',
    covers:
      'A thunk that rejects parks its ticket as `error`; a thunk that throws synchronously does too; closing a ticket while its fetch runs aborts the signal the thunk was handed.',
    plugins: [],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open({ $fn: 'rejecting', rejects: { message: 'network down' } }),
      open({ $fn: 'throwing', throws: { message: 'bad url' } }),
      open({ $fn: 'slow', defer: 'slow' }),
      ...listing,
      docs('close', 'pending:3'),
      ...listing,
      settle('slow', bytes('a')),
    ],
  },
  {
    name: 'handle-id-differs',
    covers:
      'An engine that answers with a handle whose id differs from the requested one re-keys the session; a re-key onto an id already in use refuses and parks the tab as error.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: { canonical: doc(), taken: doc() },
      responses: [
        { path: 'open', args: [bytes('requested'), {}], value: { $document: 'canonical' } },
        { path: 'open', args: [bytes('other'), {}], value: { $document: 'taken' } },
      ],
    },
    steps: [start, open(bytes('taken')), open(bytes('requested')), open(bytes('other')), ...listing],
  },
  {
    name: 'input-without-id',
    covers: 'An input that carries no id gets a ticket, then takes the handle id.',
    plugins: [],
    engine: { documents: { h: doc() }, responses: [{ path: 'open', value: { $document: 'h' } }] },
    steps: [start, open({ kind: 'url', url: 'https://example.test/x.pdf' }), ...listing],
  },
  {
    name: 'page-registry-events',
    covers:
      'A pages.* mutation event swaps the registry in place and bumps the revision; any other event is ignored; an event for a closed document is ignored.',
    plugins: [probe(docProbe('alpha', { effects: { onAction: ['@@core/document-pages-updated'] } }))],
    engine: { documents: { a: doc({ pages: [page(1, 0), page(2, 1)] }) } },
    steps: [
      start,
      open(bytes('a')),
      {
        op: 'emit',
        doc: 'a',
        event: { type: 'pages.rotated', layout: { pageCount: 2, pages: [{ ...page(1, 0), rotation: 90 }, page(2, 1)] } },
      },
      { op: 'emit', doc: 'a', event: { type: 'pages.moved', layout: { pageCount: 2, pages: [page(2, 0), page(1, 1)] } } },
      { op: 'emit', doc: 'a', event: { type: 'pages.deleted', layout: { pageCount: 1, pages: [page(2, 0)] } } },
      { op: 'emit', doc: 'a', event: { type: 'pages.inserted', layout: { pageCount: 2, pages: [page(2, 0), page(9, 1)] } } },
      { op: 'emit', doc: 'a', event: { type: 'metadata.updated', metadata: {} } },
      docs('close', 'a'),
    ],
  },
  {
    name: 'render-policy',
    covers:
      'The render policy is read once in bring-up and lands on the meta; an engine without a render service, or a failed read, means `continuous`.',
    plugins: [],
    engine: {
      documents: {
        a: doc({ services: ['render'], responses: [{ path: 'render.policy', value: { kind: 'lattice', fullPage: { widths: [256, 512] } } }] }),
        b: doc({ services: ['render'], responses: [{ path: 'render.policy', error: { message: 'offline' } }] }),
        c: doc(),
        d: doc({ services: ['render'], responses: [{ path: 'render.policy', value: null }] }),
      },
    },
    steps: [start, open(bytes('a')), open(bytes('b')), open(bytes('c')), open(bytes('d'))],
  },
  {
    name: 'capability-resolution',
    covers:
      'Workspace singletons; document instances built lazily, once per document; the truthful refusals for no provider, no active document, an unknown document and a loading one; tryCapability counting only ready documents; scopeOf.',
    plugins: [probe({ id: 'ws', token: 'ws' }), probe(docProbe('alpha')), probe({ id: 'bare' })],
    engine: { documents: { a: doc(), b: doc() }, responses: [{ path: 'open', args: [bytes('b'), {}], defer: 'open-b' }] },
    steps: [
      start,
      kernel('capability', { $token: 'ws' }),
      kernel('capability', { $token: 'alpha' }),
      kernel('tryCapability', { $token: 'alpha' }),
      kernel('capability', { $token: 'nobody' }),
      kernel('tryCapability', { $token: 'nobody' }),
      kernel('scopeOf', { $token: 'alpha' }),
      kernel('scopeOf', { $token: 'ws' }),
      kernel('scopeOf', { $token: 'nobody' }),
      kernel('scopeOf', { $token: 'documents' }),
      open(bytes('a')),
      cap('alpha', 'documentId'),
      cap('alpha', 'handleId'),
      cap('alpha', 'handleFor', ['a']),
      cap('alpha', 'handleFor', []),
      cap('alpha', 'document'),
      kernel('capability', { $token: 'alpha' }, 'nope'),
      open(bytes('b')),
      kernel('capability', { $token: 'alpha' }, 'b'),
      kernel('tryCapability', { $token: 'alpha' }, 'b'),
      kernel('tryCapability', { $token: 'ws' }),
      cap('ws', 'documentId'),
      cap('ws', 'handleId'),
      cap('ws', 'document'),
      settle('open-b', { $document: 'b' }),
      cap('alpha', 'documentId', [], 'b'),
      kernel('status'),
    ],
  },
  {
    name: 'context-resolution-from-plugins',
    covers:
      "A plugin's own get/tryGet/forDocument: bring-up counts for tryGet inside the plugin, sibling document capabilities resolve for the bound document, and forDocument crosses to another one.",
    plugins: [probe(docProbe('alpha')), probe(docProbe('beta', { requires: ['alpha'], optional: ['missing'] })), probe({ id: 'ws', token: 'ws', optional: ['alpha'] })],
    engine: { documents: { a: doc(), b: doc() } },
    steps: [
      start,
      open(bytes('a')),
      open(bytes('b')),
      cap('beta', 'get', ['alpha'], 'a'),
      cap('beta', 'tryGet', ['alpha'], 'a'),
      cap('beta', 'tryGet', ['missing'], 'a'),
      cap('beta', 'forDocument', ['alpha', 'b'], 'a'),
      cap('ws', 'tryGet', ['alpha']),
      cap('ws', 'get', ['alpha']),
      docs('setActive', 'a'),
      cap('ws', 'handleId'),
      cap('ws', 'handleFor', ['b']),
      cap('ws', 'core'),
    ],
  },
  {
    name: 'dispatch-and-watch',
    covers:
      'dispatch targets the bound slice; a reducer returning the same state emits no change (so a reference watch does not fire), a new state does; action listeners see every dispatch either way.',
    plugins: [probe(docProbe('alpha', { effects: { watchState: true, onAction: ['SET', 'SAME', 'MERGE'] } }))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      cap('alpha', 'dispatch', [{ type: 'SAME' }]),
      cap('alpha', 'dispatch', [{ type: 'MERGE', state: { m: 1 } }]),
      cap('alpha', 'dispatch', [{ type: 'SET', state: { n: 5 } }]),
      cap('alpha', 'dispatch', [{ type: 'UNKNOWN' }]),
      cap('alpha', 'state'),
    ],
  },
  {
    name: 'active-document-and-order',
    covers:
      'setActive (a pending tab is selectable, an unknown id is ignored), move with clamping, swap, and the next-active rule when the active tab closes: the tab before it, else the first.',
    plugins: [probe({ id: 'ws', token: 'ws', effects: { watchActive: true } })],
    engine: { documents: { a: doc(), b: doc(), c: doc(), d: doc() }, responses: [{ path: 'open', args: [bytes('d'), {}], defer: 'open-d' }] },
    steps: [
      start,
      open(bytes('a')),
      open(bytes('b')),
      open(bytes('c')),
      open(bytes('d')),
      docs('setActive', 'd'),
      docs('setActive', 'zzz'),
      docs('setActive', 'b'),
      docs('move', 'a', 10),
      docs('getOrder'),
      docs('move', 'a', -3),
      docs('getOrder'),
      docs('move', 'zzz', 0),
      docs('swap', 'a', 'd'),
      docs('getOrder'),
      docs('swap', 'a', 'zzz'),
      docs('close', 'b'),
      docs('getActiveId'),
      docs('setActive', 'd'),
      docs('close', 'd'),
      docs('getActiveId'),
      docs('closeAll'),
      ...listing,
      docs('close', 'never'),
    ],
  },
  {
    name: 'close-first-active',
    covers: 'Closing the active tab when it is first selects the new first tab.',
    plugins: [],
    engine: { documents: { a: doc(), b: doc() } },
    steps: [start, open(bytes('a')), open(bytes('b')), docs('setActive', 'a'), docs('close', 'a'), docs('getActiveId'), docs('close', 'b'), docs('getActiveId')],
  },
  {
    name: 'document-io',
    covers:
      'download and downloadLayer go straight to the handle; both refuse with no document; downloadLayer refuses on an engine without layers; allows answers from the handle and is false with no document.',
    plugins: [],
    engine: {
      documents: {
        a: doc({
          services: ['downloadLayer'],
          security: { allows: ['doc.print'] },
          responses: [
            { path: 'download', value: { $bytes: 'JVBERi0=' } },
            { path: 'downloadLayer', value: { $bytes: 'TEFZRVI=' } },
          ],
        }),
        b: doc({ security: { allows: '*' } }),
      },
    },
    steps: [
      start,
      docs('save'),
      docs('saveLayer'),
      docs('allows', 'doc.print'),
      open(bytes('a')),
      docs('save', undefined, { mode: 'rewrite' }),
      docs('saveLayer'),
      docs('allows', 'doc.print'),
      docs('allows', 'doc.download'),
      open(bytes('b')),
      docs('saveLayer', 'b'),
      docs('allows', 'doc.download', 'b'),
      docs('allows', 'doc.print', 'zzz'),
    ],
  },
  {
    name: 'start-lifecycle',
    covers:
      'start runs workspace inits in plan order and then workspace effects; a second start joins the first; status moves created → starting → started.',
    plugins: [
      probe({ id: 'late', token: 'late', requires: ['early'], init: { defer: 'late-init' }, effects: {} }),
      probe({ id: 'early', token: 'early', init: true, effects: {} }),
    ],
    steps: [kernel('status'), start, kernel('status'), start, { op: 'settle', label: 'late-init' }, kernel('status'), start],
  },
  {
    name: 'start-failure',
    covers:
      'A workspace init that rejects fails the kernel terminally: the workspace scope unwinds, start rejects, a later start refuses, capability and open refuse, destroy stays legal.',
    plugins: [
      probe({ id: 'ws', token: 'ws', effects: {}, init: { rejects: { message: 'boot failed' } } }),
      probe({ id: 'teardown', token: 'td', effects: { cleanup: true } }),
    ],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      kernel('status'),
      start,
      kernel('capability', { $token: 'ws' }),
      kernel('tryCapability', { $token: 'ws' }),
      open(bytes('a')),
      destroy,
      kernel('status'),
    ],
  },
  {
    name: 'destroy-during-start',
    covers: 'destroy while a workspace init is pending: start stops at its next check, destroy joins it, and nothing after the pending init runs.',
    plugins: [
      probe({ id: 'first', token: 'first', init: { defer: 'first-init' }, effects: {} }),
      probe({ id: 'second', token: 'second', init: true, effects: {} }),
    ],
    steps: [start, destroy, kernel('status'), settle('first-init'), kernel('status'), destroy, start],
  },
  {
    name: 'destroy-closes-everything',
    covers:
      'destroy closes every session (a mid-close one included), unwinds document scopes LIFO in reverse dependency order, then the workspace scope, then refuses further use; a teardown registered after disposal runs immediately.',
    plugins: [
      probe(docProbe('alpha', { effects: { cleanup: true } })),
      probe(docProbe('beta', { requires: ['alpha'], effects: { cleanup: { defer: 'beta-teardown' } } })),
      probe({ id: 'ws', token: 'ws', effects: { cleanup: true } }),
    ],
    engine: { documents: { a: doc(), b: doc() } },
    steps: [
      start,
      open(bytes('a')),
      open(bytes('b')),
      cap('alpha', 'cleanup', ['late-registered'], 'a'),
      destroy,
      kernel('status'),
      settle('beta-teardown'),
      settle('beta-teardown'),
      kernel('status'),
      kernel('getState'),
      kernel('capability', { $token: 'ws' }),
      kernel('tryCapability', { $token: 'ws' }),
      docs('open', bytes('a')),
      docs('openAll', []),
      docs('unlock', 'a', { password: '' }),
      start,
    ],
  },
  {
    name: 'teardown-after-dispose',
    covers:
      'A cleanup registered on a scope that already disposed runs as soon as it is registered, instead of being dropped.',
    plugins: [probe(docProbe('alpha'))],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), { op: 'call', target: 'kernel', method: 'capability', args: [{ $token: 'alpha' }, 'a'] }, docs('close', 'a'), { op: 'call', target: { ref: 11 }, args: ['after-close'] }],
  },
  {
    name: 'teardown-failures-are-reported',
    covers:
      'A teardown that throws is reported and the rest still run; a listener that throws is reported and its siblings still run.',
    plugins: [
      probe(docProbe('alpha', { effects: { cleanup: { throws: { message: 'teardown broke' } } } })),
      probe(docProbe('beta', { effects: { cleanup: { rejects: { message: 'async teardown broke' } } } })),
    ],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), docs('close', 'a')],
  },
  {
    name: 'missing-dependency',
    covers: 'A required token nobody provides fails construction, and the message carries the token\'s own remedy.',
    plugins: [probe({ id: 'needs', requires: ['absent'] }), probe({ id: 'provider', token: 'other', hint: 'unused' })],
    steps: [],
  },
  {
    name: 'missing-dependency-with-hint',
    covers: 'The hint is appended to the missing-dependency message.',
    plugins: [probe({ id: 'needs', requires: ['needed-token'], requiresHint: { 'needed-token': "add neededPlugin() from '@x/y' to your plugins list" } })],
    steps: [],
  },
  {
    name: 'dependency-cycle',
    covers: 'A requires cycle fails construction naming a plugin in it.',
    plugins: [probe({ id: 'a', token: 'ta', requires: ['tb'] }), probe({ id: 'b', token: 'tb', requires: ['ta'] })],
    steps: [],
  },
  {
    name: 'plan-order',
    covers:
      'Dependencies are initialised before dependents regardless of list order, optional dependencies order too, a plugin optional on itself is not a cycle, and a token without a capability provides nothing.',
    plugins: [
      probe({ id: 'c', token: 'tc', requires: ['tb'], init: true }),
      probe({ id: 'b', token: 'tb', optional: ['ta', 'tb', 'nobody'], init: true }),
      probe({ id: 'a', token: 'ta', init: true }),
    ],
    steps: [start],
  },
  {
    name: 'capability-constructor-throws',
    covers: 'A workspace capability that throws while being built fails construction; a document one throws at resolution and is not cached.',
    plugins: [probe({ id: 'ws', token: 'ws', capabilityThrows: { message: 'cannot build' } })],
    steps: [],
  },
  {
    name: 'document-capability-constructor-throws',
    covers: 'A document capability that throws at build is a bug and propagates out of resolution, every time.',
    plugins: [probe(docProbe('alpha', { init: undefined, effects: undefined, capabilityThrows: { message: 'cannot build' } }))],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), kernel('capability', { $token: 'alpha' }), kernel('tryCapability', { $token: 'alpha' })],
  },
  {
    name: 'listener-isolation',
    covers: 'A watcher that throws is reported; the change still reaches the other listeners and the dispatch completes.',
    plugins: [
      probe({ id: 'thrower', token: 'thrower', reduce: { SET: 'replace' }, effects: { watchState: true, onAction: ['SET'], listenerThrows: { message: 'listener broke' } } }),
      probe({ id: 'sibling', token: 'sibling', effects: { watchState: true, onAction: ['SET'] } }),
    ],
    steps: [start, cap('thrower', 'dispatch', [{ type: 'SET', state: { $undefined: true } }])],
  },
  {
    name: 'close-during-init-then-init-fails',
    covers:
      'An init that rejects after close() won is still a cancellation, not a failure: no error tab, nothing reported.',
    plugins: [probe(docProbe('alpha', { init: { defer: 'init' } }))],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), docs('close', 'a'), fail('init', 'init broke late'), ...listing],
  },
  {
    name: 'an-abort-error-from-the-engine-reads-as-cancelled',
    covers:
      'An engine call rejecting with AbortError when nobody closed the tab: the open rejects as cancelled and the tab stays loading, which is what isAbortLike does to an abort the kernel did not start.',
    plugins: [],
    engine: { documents: { a: doc({ responses: [{ path: 'pages.list', error: { name: 'AbortError', message: 'worker aborted' } }] }) } },
    steps: [start, open(bytes('a')), ...listing],
  },
  {
    name: 'an-abort-error-after-unlock-reads-as-cancelled',
    covers: 'The same rule past the password.',
    plugins: [],
    engine: {
      documents: {
        a: doc({
          security: { passwordPrompt: { state: 'required' } },
          responses: [
            { path: 'security.unlock', value: {} },
            { path: 'pages.list', error: { name: 'AbortError', message: 'worker aborted' } },
          ],
        }),
      },
    },
    steps: [start, open(bytes('a')), docs('unlock', 'a', { password: 'p' }), ...listing],
  },
  {
    name: 'plain-promise-engine-late-handle',
    covers:
      'An engine whose calls cannot be aborted: close() does not wait for the open, and the handle that lands afterwards is closed through the disposed scope rather than leaked.',
    plugins: [probe(docProbe('alpha'))],
    engine: { plainPromises: true, documents: { a: doc() }, responses: [{ path: 'open', defer: 'open-a' }] },
    steps: [start, open(bytes('a')), docs('close', 'a'), ...listing, settle('open-a', { $document: 'a' })],
  },
  {
    name: 'rekey-keeps-another-active-tab',
    covers: 'Re-keying a ticket that is not the active tab leaves the active tab alone.',
    plugins: [],
    engine: { documents: { a: doc(), real: doc() } },
    steps: [
      start,
      open(bytes('a')),
      open({ $fn: 'source', defer: 'fetch' }, { activate: false }),
      docs('getActiveId'),
      settle('fetch', bytes('real')),
      ...listing,
    ],
  },
  {
    name: 'close-joins-a-close-in-flight',
    covers: 'A second close while the first is still disposing returns the same close.',
    plugins: [probe(docProbe('alpha', { effects: { cleanup: { defer: 'teardown' } } }))],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), docs('close', 'a'), docs('close', 'a'), settle('teardown')],
  },
  {
    name: 'close-during-rollback-reports-the-failure',
    covers:
      'close() arriving while a failed open is still rolling back joins that operation, and the real failure it rejects with is reported.',
    plugins: [],
    engine: {
      documents: {
        a: doc({
          responses: [
            { path: 'pages.list', error: { name: 'EngineError', message: 'no pages', code: 'Format' } },
            { path: 'close', defer: 'handle-close' },
          ],
        }),
      },
    },
    steps: [start, open(bytes('a')), docs('close', 'a'), settle('handle-close'), ...listing],
  },
  {
    name: 'page-event-before-commit',
    covers: 'A page mutation event that arrives during bring-up, before the document is published, changes nothing.',
    plugins: [probe(docProbe('alpha', { init: { defer: 'init' } }))],
    engine: { documents: { a: doc({ pages: [page(1, 0)] }) } },
    steps: [
      start,
      open(bytes('a')),
      { op: 'emit', doc: 'a', event: { type: 'pages.rotated', layout: { pageCount: 1, pages: [{ ...page(1, 0), rotation: 90 }] } } },
      settle('init'),
      docs('get', 'a'),
    ],
  },
  {
    name: 'try-get-across-scopes',
    covers:
      "A document plugin's tryGet resolves a workspace capability; a workspace plugin's tryGet of a document capability is null while the active document is still loading; allows() is false for a loading document.",
    plugins: [probe({ id: 'ws', token: 'ws', optional: ['alpha'] }), probe(docProbe('alpha')), probe(docProbe('beta', { optional: ['ws'] }))],
    engine: { documents: { a: doc({ security: { allows: '*' } }), b: doc({ security: { allows: '*' } }) }, responses: [{ path: 'open', args: [bytes('b'), {}], defer: 'open-b' }] },
    steps: [
      start,
      open(bytes('a')),
      cap('beta', 'tryGet', ['ws'], 'a'),
      open(bytes('b')),
      cap('ws', 'tryGet', ['alpha']),
      cap('ws', 'handleFor', []),
      cap('ws', 'document'),
      docs('allows', 'doc.print'),
      docs('allows', 'doc.print', 'a'),
      settle('open-b', { $document: 'b' }),
      cap('ws', 'tryGet', ['alpha']),
    ],
  },
  {
    name: 'a-capability-outlives-its-document',
    covers:
      "A capability kept after its document closed: dispatch reaches no slice and does nothing, state reads undefined, and document() falls back to the meta staged at bring-up.",
    plugins: [probe(docProbe('alpha', { effects: undefined, init: undefined }))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      kernel('capability', { $token: 'alpha' }, 'a'),
      docs('close', 'a'),
      { op: 'call', target: { ref: 0 }, args: [{ type: 'SET', state: { n: 9 } }] },
      { op: 'call', target: { ref: 1 } },
      { op: 'call', target: { ref: 2 } },
    ],
  },
  {
    name: 'a-listener-dispatching-mid-pass',
    covers:
      'A state watcher that dispatches from inside its handler: the store finishes the pass it is in, then runs another, so listener order stays deterministic.',
    plugins: [
      probe({ id: 'relay', token: 'relay', reduce: { SET: 'replace', MERGE: 'merge' }, effects: { watchState: true, watchDispatchOnce: { type: 'MERGE', state: { relayed: true } } } }),
      probe({ id: 'after', token: 'after', effects: { watchActive: true } }),
    ],
    steps: [start, cap('relay', 'dispatch', [{ type: 'SET', state: { n: 1 } }]), cap('relay', 'state')],
  },
  {
    name: 'a-plugin-with-no-reducer',
    covers: 'A plugin without a reducer keeps its initial state whatever it dispatches.',
    plugins: [probe({ id: 'inert', token: 'inert', reduce: null, initialState: { fixed: true } })],
    steps: [start, cap('inert', 'dispatch', [{ type: 'SET', state: {} }]), cap('inert', 'state')],
  },
  {
    name: 'destroy-during-the-last-init',
    covers: 'destroy() while the last workspace init is pending: start stops before running any effects.',
    plugins: [probe({ id: 'only', token: 'only', init: { defer: 'init' }, effects: {} })],
    steps: [start, destroy, settle('init'), kernel('status')],
  },
  {
    name: 'destroy-during-a-start-that-fails',
    covers: 'destroy() while start is pending, and start then fails: destroy reports the failure, since the kernel was not already failed when destroy began.',
    plugins: [probe({ id: 'only', token: 'only', init: { defer: 'init' } })],
    steps: [start, destroy, fail('init', 'boot failed late'), kernel('status')],
  },
  {
    name: 'capability-of-an-error-or-closing-tab',
    covers: 'Resolution refuses truthfully for a tab that failed and for one whose close is still disposing.',
    plugins: [probe(docProbe('alpha', { effects: { cleanup: { defer: 'teardown' } } }))],
    engine: {
      documents: { a: doc(), bad: doc() },
      responses: [{ path: 'open', args: [bytes('bad'), {}], error: { name: 'EngineError', message: 'broken', code: 'Format' } }],
    },
    steps: [
      start,
      open(bytes('bad')),
      kernel('capability', { $token: 'alpha' }, 'bad'),
      open(bytes('a')),
      docs('close', 'a'),
      kernel('capability', { $token: 'alpha' }, 'a'),
      kernel('tryCapability', { $token: 'alpha' }, 'a'),
      settle('teardown'),
    ],
  },
  {
    name: 'try-get-during-bring-up',
    covers:
      "A capability resolved while its document is still in bring-up (a plugin's init pending) is usable, and its tryGet counts bring-up; the public tryCapability does not.",
    plugins: [probe(docProbe('alpha', { init: { defer: 'init' }, optional: ['beta'] })), probe(docProbe('beta'))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      kernel('tryCapability', { $token: 'alpha' }, 'a'),
      cap('alpha', 'tryGet', ['beta'], 'a'),
      cap('alpha', 'documentId', [], 'a'),
      settle('init'),
    ],
  },
  {
    name: 'an-abort-error-from-unlock-reads-as-cancelled',
    covers: 'security.unlock rejecting with AbortError, with no close: the unlock rejects as cancelled and the tab stays locked.',
    plugins: [],
    engine: {
      documents: {
        a: doc({
          security: { passwordPrompt: { state: 'required' } },
          responses: [{ path: 'security.unlock', error: { name: 'AbortError', message: 'worker aborted' } }],
        }),
      },
    },
    steps: [start, open(bytes('a')), docs('unlock', 'a', { password: 'p' }), docs('list')],
  },
  {
    name: 'a-rekeyed-slot-carries-its-new-id',
    covers: 'A ticket re-keyed to a document that then parks as locked keeps the real id on its pending slot.',
    plugins: [],
    engine: { documents: { real: doc({ security: { passwordPrompt: { state: 'required' } } }) } },
    steps: [start, open({ $fn: 'source', resolves: bytes('real') }), docs('list'), docs('get', 'real')],
  },
  {
    name: 'closing-one-document-leaves-the-others-listening',
    covers:
      "Closing a document releases its plugins' watches and action listeners and nobody else's: the other document's probe still hears its own dispatches.",
    plugins: [probe(docProbe('alpha', { effects: { watchState: true, onAction: ['SET'] } }))],
    engine: { documents: { a: doc(), b: doc(), c: doc() } },
    steps: [
      start,
      open(bytes('a')),
      open(bytes('b')),
      open(bytes('c')),
      docs('close', 'b'),
      cap('alpha', 'dispatch', [{ type: 'SET', state: { n: 1 } }], 'a'),
      cap('alpha', 'dispatch', [{ type: 'SET', state: { n: 2 } }], 'c'),
    ],
  },
  {
    name: 'a-layout-without-pages',
    covers:
      'A page event whose layout carries a count and no page list: the registry copies what it is given, so the meta has no pages member rather than an empty one.',
    plugins: [],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), { op: 'emit', doc: 'a', event: { type: 'pages.deleted', layout: { pageCount: 0 } } }],
  },
  {
    name: 'a-rekey-onto-a-closing-id-refuses',
    covers:
      'A ticket whose input names a document that is still closing refuses the re-key: the closing session holds the id even though its slot has left the store.',
    plugins: [probe(docProbe('alpha', { effects: { cleanup: { defer: 'teardown' } } }))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      open({ $fn: 'source', defer: 'fetch' }),
      docs('close', 'a'),
      ...listing,
      settle('fetch', bytes('a')),
      ...listing,
      settle('teardown'),
      ...listing,
    ],
  },
  {
    name: 'a-rekeyed-ticket-lists-its-new-id-while-the-engine-opens',
    covers: 'Between the re-key and the engine answering, the pending slot already carries the real id.',
    plugins: [],
    engine: { documents: { real: doc() }, responses: [{ path: 'open', defer: 'open-real' }] },
    steps: [
      start,
      open({ $fn: 'source', defer: 'fetch' }),
      settle('fetch', bytes('real')),
      docs('list'),
      docs('get', 'real'),
      settle('open-real', { $document: 'real' }),
      docs('list'),
    ],
  },
];

// ── the v3 contract: create/connect controllers, leases, documents API ─────────

/** The members of a `create` probe's api, in object order: a capability
 *  printed as an outcome registers one ref per member, in this order. */
const API = [
  'instanceId',
  'state',
  'dispatch',
  'docCall',
  'pageCall',
  'allows',
  'docId',
  'docRead',
  'on',
  'emit',
  'onThrowing',
  'abortSignal',
  'waitFor',
  'waitForThrowing',
  'queue',
  'latest',
  'latestCancel',
  'latestRunning',
  'acquire',
  'forPage',
  'tryForPage',
  'assertPageRef',
  'getPage',
  'get',
  'tryGet',
  'forDocument',
  'tryForDocument',
  'cleanup',
  'signalReason',
] as const;
/** The ref of an api member when the capability printed at `base`. */
const at = (member: (typeof API)[number], base = 0) => base + API.indexOf(member);
const callRef = (ref: number, ...args: unknown[]): Step => ({ op: 'call', target: { ref }, args });
const api = (member: (typeof API)[number], ...args: unknown[]) => callRef(at(member), ...args);
const ref1 = (pon: number) => ({ kind: 'objectNumber', pageObjectNumber: pon });
const U = { $undefined: true };

/** A document-scoped `create` probe. */
const ctl = (id: string, extra: Partial<ProbeConfig> = {}): ProbeConfig => ({
  id,
  scope: 'document',
  token: id,
  initialState: { n: 0 },
  reduce: { SET: 'replace', MERGE: 'merge' },
  create: {},
  ...extra,
});
/** A workspace `create` probe. */
const wsCtl = (id: string, extra: Partial<ProbeConfig> = {}): ProbeConfig => ({
  id,
  token: id,
  initialState: { n: 0 },
  reduce: { SET: 'replace' },
  create: {},
  ...extra,
});
const capOf = (token: string, docId?: string): Step => kernel('capability', { $token: token }, ...(docId ? [docId] : []));

const v3Cases: ScenarioCase[] = [
  {
    name: 'create-and-connect-order',
    covers:
      'A document `create` plugin is built eagerly in bring-up, in dependency order, right after its effects, and its connect runs there; a workspace one is built at construction and connects at start after its effects. Each instance gets its own id.',
    plugins: [
      probe(docProbe('alpha')),
      probe(ctl('ctl', { requires: ['alpha'], create: { connect: true }, effects: { onAction: ['SET'] } })),
      probe(wsCtl('wsc', { init: true, effects: {}, create: { connect: true } })),
    ],
    engine: { documents: { a: doc(), b: doc() } },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      cap('ctl', 'instanceId', [], 'a'),
      cap('wsc', 'instanceId'),
      cap('ctl', 'dispatch', [{ type: 'SET', state: { n: 1 } }], 'a'),
      cap('ctl', 'state', [], 'a'),
      open(bytes('b')),
      cap('ctl', 'instanceId', [], 'b'),
      docs('close', 'a'),
      open(bytes('a')),
      cap('ctl', 'instanceId', [], 'a'),
    ],
  },
  {
    name: 'create-throws-at-construction',
    covers: 'A workspace `create` that throws fails kernel construction.',
    plugins: [probe(wsCtl('wsc', { create: { throws: { message: 'cannot create' } } }))],
    steps: [],
  },
  {
    name: 'create-throws-in-bring-up',
    covers: 'A document `create` that throws rolls the open back to an error tab.',
    plugins: [probe(ctl('ctl', { create: { throws: { message: 'cannot create' } } }))],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), ...listing],
  },
  {
    name: 'connect-throws-in-bring-up',
    covers: 'A document connect that throws rolls the open back too.',
    plugins: [probe(ctl('ctl', { create: { connect: { throws: { message: 'cannot connect' } } } }))],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), ...listing],
  },
  {
    name: 'connect-throws-at-start',
    covers: 'A workspace connect that throws fails start.',
    plugins: [probe(wsCtl('wsc', { create: { connect: { throws: { message: 'cannot connect' } } } }))],
    steps: [start, kernel('status')],
  },
  {
    name: 'connect-dispatches-and-listens',
    covers:
      'A connect may dispatch into its slice, read the page registry before the document is published (from the staged meta), and listen: to the guarded handle event stream (an object source) and to a capability event (a function source). Both subscriptions end when the instance closes.',
    plugins: [
      probe(ctl('ctl', { create: { connect: { dispatch: { type: 'SET', state: { n: 7 } }, listenDoc: true, listenActive: true, readPage: 1 } } })),
    ],
    engine: { documents: { a: doc(), b: doc() } },
    steps: [
      start,
      open(bytes('a')),
      cap('ctl', 'state', [], 'a'),
      { op: 'emit', doc: 'a', event: { type: 'metadata.updated', metadata: {} } },
      open(bytes('b')),
      docs('setActive', 'a'),
      docs('close', 'a'),
      docs('setActive', 'b'),
    ],
  },
  {
    name: 'guarded-handle',
    covers:
      'ctx.doc is the guarded handle: calls reach the engine, synchronous reads pass through, a synchronous engine throw becomes a PluginError, a call pending at close rejects instance-closed and is aborted, a call after close still reaches the engine and is aborted at once, and a late answer is dropped.',
    plugins: [probe(ctl('ctl'))],
    engine: {
      documents: {
        a: doc({
          capabilities: { canRender: true },
          security: { allows: ['doc.print'], effectiveScope: ['doc.print'] },
          responses: [
            { path: 'metadata.read', value: { title: 'T' } },
            { path: 'metadata.slow', defer: 'slow' },
            { path: 'page.text.read', pon: 1, value: { runs: [] } },
          ],
        }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('docCall', 'metadata.read'),
      api('pageCall', 1, 'text.read', { from: 0 }),
      api('pageCall', 99, 'text.read'),
      api('allows', 'doc.print'),
      api('allows', 'doc.annotate'),
      api('docId'),
      api('docRead', 'capabilities'),
      api('docRead', 'security.effectiveScope'),
      api('docRead', 'id'),
      api('docCall', 'metadata.slow'),
      docs('close', 'a'),
      settle('slow', { title: 'late' }),
      api('docCall', 'metadata.read'),
      api('dispatch', { type: 'SET', state: { n: 3 } }),
      api('dispatch', { type: 'SET', state: { n: 4 } }),
      api('state'),
    ],
  },
  {
    name: 'guarded-handle-maps-engine-errors',
    covers: 'An engine rejection through the guard is a PluginError from the mapping table; a plain rejection is operation-failed and an AbortError operation-cancelled.',
    plugins: [probe(ctl('ctl'))],
    engine: {
      documents: {
        a: doc({
          responses: [
            { path: 'metadata.notFound', error: { name: 'EngineError', message: 'no such', code: 'NotFound', details: { key: 'k' } } },
            { path: 'metadata.forbidden', error: { name: 'EngineError', message: 'nope', code: 'Forbidden' } },
            { path: 'metadata.unknown', error: { name: 'EngineError', message: 'odd', code: 'Format' } },
            { path: 'metadata.plain', error: { message: 'plain failure' } },
            { path: 'metadata.aborted', error: { name: 'AbortError', message: 'worker aborted' } },
          ],
        }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('docCall', 'metadata.notFound'),
      api('docCall', 'metadata.forbidden'),
      api('docCall', 'metadata.unknown'),
      api('docCall', 'metadata.plain'),
      api('docCall', 'metadata.aborted'),
    ],
  },
  {
    name: 'guarded-handle-over-plain-promises',
    covers: 'Over an engine whose calls cannot be aborted, the guard still settles, maps errors, and rejects a pending call at close without an abort.',
    plugins: [probe(ctl('ctl'))],
    engine: {
      plainPromises: true,
      documents: {
        a: doc({
          responses: [
            { path: 'metadata.read', value: { title: 'T' } },
            { path: 'metadata.bad', error: { name: 'EngineError', message: 'gone', code: 'NotFound' } },
            { path: 'metadata.slow', defer: 'slow' },
          ],
        }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('docCall', 'metadata.read'),
      api('docCall', 'metadata.bad'),
      api('docCall', 'metadata.slow'),
      docs('close', 'a'),
      settle('slow', {}),
    ],
  },
  {
    name: 'controller-events',
    covers:
      'events.source: synchronous fan-out, a signal ends a subscription and a pre-aborted one never registers, a throwing listener is reported, and the source is disposed with the instance. A signal keeps the reason it was first aborted with, the platform AbortError when none.',
    plugins: [probe(ctl('ctl'))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('on', 'first'),
      api('on', 'second', 'sig'),
      api('emit', { v: 1 }),
      api('signalReason', 'sig'),
      api('abortSignal', 'sig'),
      api('signalReason', 'sig'),
      api('abortSignal', 'why', { $error: { name: 'Error', message: 'user left', code: 7 } }),
      api('abortSignal', 'why', { $error: { name: 'Error', message: 'second abort is ignored' } }),
      api('signalReason', 'why'),
      api('emit', { v: 2 }),
      callRef(API.length),
      api('emit', { v: 3 }),
      api('on', 'third', 'sig'),
      api('emit', { v: 4 }),
      api('onThrowing', 'listener broke'),
      api('emit', { v: 5 }),
      docs('close', 'a'),
      api('emit', { v: 6 }),
      api('on', 'late'),
      api('emit', { v: 7 }),
    ],
  },
  {
    name: 'controller-wait-for',
    covers:
      'waitFor resolves on the store change that makes it hold, or at once when it already holds; it rejects operation-cancelled on its signal (before or during), a throwing predicate rejects with that error, and closing rejects instance-closed.',
    plugins: [probe(ctl('ctl'))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('waitFor', 'n', 1),
      api('dispatch', { type: 'SET', state: { n: 1 } }),
      api('waitFor', 'n', 1),
      api('waitFor', 'n', 2, 'sig'),
      api('abortSignal', 'sig'),
      api('waitFor', 'n', 2, 'sig'),
      api('waitForThrowing', 'predicate broke'),
      api('waitFor', 'n', 9),
      docs('close', 'a'),
      api('waitFor', 'n', 9),
    ],
  },
  {
    name: 'controller-serial-queue',
    covers: 'serialQueue(key): one operation at a time per key, in submission order, a failure does not poison the next, and two keys are independent.',
    plugins: [probe(ctl('ctl'))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('queue', 'k', 'q1'),
      api('queue', 'k', 'q2'),
      api('queue', 'other', 'q3'),
      fail('q1', 'first failed'),
      settle('q2', 'second'),
      settle('q3', 'third'),
    ],
  },
  {
    name: 'controller-latest-lane',
    covers:
      'latest(key): a newer run supersedes the older one, which rejects operation-cancelled and cannot commit; cancel() and the caller signal cancel a run; a pre-aborted signal refuses to start; a failing run maps through toPluginError; closing rejects instance-closed, and a run after close refuses.',
    plugins: [probe(ctl('ctl'))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('latest', 's', 'l1'),
      api('latestRunning', 's'),
      api('latest', 's', 'l2'),
      settle('l1', 'late'),
      settle('l2', 'fresh'),
      api('latestRunning', 's'),
      api('latest', 's', 'l3'),
      api('latestCancel', 's'),
      settle('l3', 'after cancel'),
      api('latest', 't', 'm1'),
      api('latestCancel', 't', 'user gave up'),
      settle('m1', 'after cancel'),
      api('latestCancel', 't'),
      api('latest', 's', 'l4', 'sig'),
      api('abortSignal', 'sig'),
      settle('l4', 'after abort'),
      api('latest', 's', 'l5', 'sig'),
      api('latest', 's', 'l6', U, { message: 'run broke' }),
      settle('l6'),
      api('latest', 's', 'l7', U, { class: 'PermissionDenied', required: 'doc.print' }),
      settle('l7'),
      api('latest', 's', 'l8', U, { class: 'CancelledError', what: 'inner' }),
      settle('l8'),
      api('latest', 's', 'l9', U, { class: 'PluginError', code: 'conflict', message: 'stale' }),
      settle('l9'),
      api('latest', 's', 'l10'),
      docs('close', 'a'),
      settle('l10', 'too late'),
      api('latest', 's', 'l11'),
    ],
  },
  {
    name: 'controller-acquire',
    covers:
      'acquire: the resource is returned and its disposal deferred into the instance scope; a rejected acquisition rejects as it is; one that lands after close is disposed at once and rejects instance-closed.',
    plugins: [probe(ctl('ctl'))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('acquire', 'r1'),
      settle('r1', { handle: 1 }),
      api('acquire', 'r3'),
      fail('r3', 'could not acquire'),
      api('acquire', 'r2'),
      docs('close', 'a'),
      settle('r2', { handle: 2 }),
    ],
  },
  {
    name: 'controller-geometry-and-pages',
    covers:
      'geometry.forPage builds a PageSpace from the page crop, tryForPage answers null and forPage throws not-found for a foreign ref; assertPageRef and getPage read the same registry, which a page event replaces. A workspace controller has no bound document: doc throws, page reads are empty, forDocument and tryForDocument cross to a document.',
    plugins: [probe(ctl('ctl')), probe(wsCtl('wsc', { optional: ['ctl'] }))],
    engine: {
      documents: {
        a: doc({
          pages: [
            page(1, 0),
            { ...page(2, 1), boxes: { media: { ...box }, crop: { left: 10, bottom: 20, right: 310, top: 420 } } },
          ],
        }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('forPage', 1),
      api('forPage', 2),
      api('tryForPage', 2),
      api('tryForPage', 99),
      api('forPage', 99),
      api('assertPageRef', 2),
      api('assertPageRef', 99),
      api('getPage', 2),
      api('getPage', 99),
      {
        op: 'emit',
        doc: 'a',
        event: { type: 'pages.deleted', layout: { pageCount: 1, pages: [{ ...page(1, 0), boxes: { media: { ...box }, crop: { left: 0, bottom: 0, right: 100, top: 100 } } }] } },
      },
      api('forPage', 1),
      api('forPage', 2),
      cap('wsc', 'instanceId'),
      cap('wsc', 'docId'),
      cap('wsc', 'tryForPage', [1]),
      cap('wsc', 'getPage', [1]),
      cap('wsc', 'assertPageRef', [1]),
      cap('wsc', 'get', ['ctl']),
      cap('wsc', 'forDocument', ['ctl', 'a']),
      cap('wsc', 'tryForDocument', ['ctl', 'a']),
      cap('wsc', 'tryForDocument', ['ctl', 'zzz']),
      cap('wsc', 'tryGet', ['documents']),
    ],
  },
  {
    name: 'page-space-at-the-origin',
    covers:
      'A page space maps the origin both ways, including for a crop whose top edge is 0; a registry entry whose ref is not a whole non-negative number names no page.',
    plugins: [probe(ctl('ctl'))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('forPage', 1),
      {
        op: 'emit',
        doc: 'a',
        event: {
          type: 'pages.deleted',
          layout: {
            pageCount: 3,
            pages: [
              { ...page(1, 0), ref: { kind: 'objectNumber', pageObjectNumber: 1.5 } },
              { ...page(1, 1), ref: { kind: 'objectNumber', pageObjectNumber: -1 } },
              { ...page(3, 2), boxes: { media: { ...box }, crop: { left: 5, bottom: -100, right: 105, top: 0 } } },
            ],
          },
        },
      },
      api('getPage', 1),
      api('getPage', 0),
      api('tryForPage', 1),
      api('forPage', 3),
    ],
  },
  {
    name: 'lane-cancel-reasons',
    covers:
      "cancel(reason) aborts with String(reason ?? 'cancelled'): null is 'cancelled', and numbers, booleans, objects and arrays print as JavaScript prints them. A superseded run that finishes leaves the newer run active.",
    plugins: [probe(ctl('ctl'))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('latest', 's', 'old'),
      api('latest', 's', 'new'),
      settle('old', 'late'),
      api('latestRunning', 's'),
      settle('new', 'fresh'),
      ...(
        [null, 1.5, -2, 1e20, 1e21, 1.5e-7, 0.000001, true, {}, [1, null, [2, U], 'x']] as unknown[]
      ).flatMap((reason, i) => [
        api('latest', 's', `r${i}`),
        api('latestCancel', 's', reason),
        settle(`r${i}`, 'after cancel'),
      ]),
    ],
  },
  {
    name: 'guarded-handle-security-reads',
    covers:
      'The guarded security passes every synchronous read straight through: identity, current, the password prompt, the annotation checks, and the server id.',
    plugins: [probe(ctl('ctl'))],
    engine: {
      documents: {
        a: doc({
          security: {
            identity: { user: 'u1' },
            current: { level: 'owner' },
            passwordPrompt: { state: 'unlocked' },
            allowsAnnotationCreate: true,
            allowsAnnotationMutation: { update: true, delete: false },
            allowsAnnotationGroupAssignment: true,
          },
        }),
        b: doc({ security: { allowsAnnotationCreate: false, allowsAnnotationGroupAssignment: false } }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('docRead', 'security.identity'),
      api('docRead', 'security.current'),
      api('docRead', 'security.passwordPrompt'),
      api('docRead', 'security.allowsAnnotationCreate'),
      api('docRead', 'security.allowsAnnotationMutation', 'update', {}),
      api('docRead', 'security.allowsAnnotationMutation', 'delete', {}),
      api('docRead', 'security.allowsAnnotationGroupAssignment', 'g1'),
      api('docRead', 'events.lastServerId'),
      open(bytes('b')),
      cap('ctl', 'docRead', ['security.allowsAnnotationCreate'], 'b'),
      cap('ctl', 'docRead', ['security.allowsAnnotationGroupAssignment', 'g1'], 'b'),
    ],
  },
  {
    name: 'workspace-controller-at-destroy',
    covers:
      'The workspace lifetime ends at destroy: a pending lane run and waitFor reject instance-closed, an acquisition that lands afterwards is disposed, cleanups run with the workspace scope, and a dispatch after destroy is reported once.',
    plugins: [probe(wsCtl('wsc'))],
    steps: [
      start,
      capOf('wsc'),
      api('latest', 's', 'w1'),
      api('waitFor', 'n', 5),
      api('acquire', 'wr'),
      api('cleanup', 'workspace-cleanup'),
      destroy,
      settle('wr', { r: 1 }),
      settle('w1', 'late'),
      api('dispatch', { type: 'SET', state: { n: 1 } }),
      api('dispatch', { type: 'SET', state: { n: 2 } }),
      api('state'),
    ],
  },
  {
    name: 'leases-across-reopen',
    covers:
      'A slice lease ends when its instance closes: a retained dispatch is dropped and reported once per context, state reads the last value, and a reopened document gets a new lease the old context cannot reach.',
    plugins: [probe(docProbe('alpha', { effects: undefined, init: undefined }))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      capOf('alpha', 'a'),
      cap('alpha', 'dispatch', [{ type: 'SET', state: { n: 1 } }], 'a'),
      docs('close', 'a'),
      callRef(0, { type: 'SET', state: { n: 9 } }),
      callRef(0, { type: 'SET', state: { n: 10 } }),
      callRef(1),
      open(bytes('a')),
      callRef(0, { type: 'SET', state: { n: 11 } }),
      cap('alpha', 'state', [], 'a'),
      cap('alpha', 'dispatch', [{ type: 'SET', state: { n: 12 } }], 'a'),
      cap('alpha', 'state', [], 'a'),
    ],
  },
  {
    name: 'documents-page-registry',
    covers:
      'listPages, getPage, getPageAt, getPageIndex and getRevision read the active document by default or a named one, answer empty values with no document, and follow a page event.',
    plugins: [],
    engine: { documents: { a: doc({ pages: [page(1, 0), page(7, 1)] }), b: doc({ pages: [page(3, 0)] }) } },
    steps: [
      start,
      docs('listPages'),
      docs('getPage', ref1(1)),
      docs('getPageAt', 0),
      docs('getPageIndex', ref1(1)),
      docs('getRevision'),
      docs('getActive'),
      open(bytes('a')),
      open(bytes('b'), { activate: false }),
      docs('getActive'),
      docs('listPages'),
      docs('getPage', ref1(7)),
      docs('getPage', ref1(3)),
      docs('getPage', ref1(3), 'b'),
      docs('getPageAt', 1),
      docs('getPageAt', 5),
      docs('getPageIndex', ref1(7)),
      docs('getPageIndex', ref1(99)),
      docs('getRevision'),
      { op: 'emit', doc: 'a', event: { type: 'pages.moved', layout: { pageCount: 2, pages: [page(7, 0), page(1, 1)] } } },
      docs('getRevision', 'a'),
      docs('getPageIndex', ref1(7), 'a'),
      docs('listPages', 'b'),
      docs('listPages', 'zzz'),
      docs('getRevision', 'zzz'),
    ],
  },
  {
    name: 'documents-lifecycle-events',
    covers:
      'onOpened, onOpenFailed, onLocked, onClosed, onActiveChanged and onPagesChanged fire once per transition with their payloads; an unsubscribed listener hears nothing more, and a throwing listener is reported.',
    plugins: [],
    engine: {
      documents: {
        a: doc(),
        b: doc(),
        locked: doc({ security: { passwordPrompt: { state: 'required' } } }),
      },
      responses: [{ path: 'open', args: [bytes('bad'), {}], error: { name: 'EngineError', message: 'broken', code: 'MalformedPdf' } }],
    },
    steps: [
      start,
      docs('onOpened', { $fn: 'opened' }),
      docs('onOpenFailed', { $fn: 'openFailed' }),
      docs('onLocked', { $fn: 'locked' }),
      docs('onClosed', { $fn: 'closed' }),
      docs('onActiveChanged', { $fn: 'activeChanged' }),
      docs('onPagesChanged', { $fn: 'pagesChanged' }),
      open(bytes('a')),
      open(bytes('bad')),
      open(bytes('locked')),
      { op: 'emit', doc: 'a', event: { type: 'pages.rotated', layout: { pageCount: 1, pages: [{ ...page(1, 0), rotation: 90 }] } } },
      docs('onClosed', { $fn: 'closedThrows', throws: { message: 'listener broke' } }),
      docs('close', 'bad'),
      callRef(0),
      open(bytes('b')),
      docs('setActive', 'a'),
      docs('closeAll'),
    ],
  },
  {
    name: 'documents-rename-order-retry',
    covers:
      'rename a ready tab, a pending one and an unknown id; setOrder takes a permutation and refuses anything else; retry reopens a failed tab with its source and options and refuses a tab that did not fail.',
    plugins: [probe(docProbe('alpha', { effects: { onAction: ['@@core/document-renamed'] } }))],
    engine: {
      documents: { a: doc(), b: doc(), bad: doc() },
      responses: [{ path: 'open', args: [bytes('bad'), { password: 'p' }], times: 1, error: { name: 'EngineError', message: 'broken', code: 'MalformedPdf' } }],
    },
    steps: [
      start,
      open(bytes('a')),
      docs('rename', 'a', 'Renamed'),
      docs('get', 'a'),
      open(bytes('bad'), { name: 'Bad', password: 'p' }),
      docs('rename', 'bad', 'Broken'),
      docs('get', 'bad'),
      docs('rename', 'zzz', 'x'),
      open(bytes('b'), { activate: false }),
      docs('setOrder', ['bad', 'b', 'a']),
      docs('getOrder'),
      docs('setOrder', ['a', 'b']),
      docs('setOrder', ['a', 'b', 'zzz']),
      docs('retry', 'a'),
      docs('retry', 'zzz'),
      docs('retry', 'bad'),
      ...listing,
    ],
  },
  {
    name: 'open-all-returns-ids',
    covers: 'openAll returns the reserved ids in order: a thunk source its ticket, and a refused reservation (an id already open) the id it asked for, leaving the existing tab alone.',
    plugins: [],
    engine: { documents: { a: doc(), c: doc(), real: doc() } },
    steps: [
      start,
      open(bytes('a')),
      docs('openAll', [{ source: bytes('a') }, { source: bytes('c'), active: true }, { source: { $fn: 'source', resolves: bytes('real') } }]),
      ...listing,
    ],
  },
  {
    name: 'capability-built-before-its-lease',
    covers:
      'A document capability resolved while bring-up is still reading the render policy, before the slice leases exist, refuses: the context has no lease to read or write through.',
    plugins: [probe(docProbe('alpha')), probe({ id: 'ws', token: 'ws', optional: ['alpha'] })],
    engine: { documents: { a: doc({ services: ['render'], responses: [{ path: 'render.policy', defer: 'policy' }] }) } },
    steps: [start, open(bytes('a')), cap('ws', 'tryForDocument', ['alpha', 'a']), settle('policy', null), cap('ws', 'tryForDocument', ['alpha', 'a'])],
  },
  {
    name: 'list-and-save-details',
    covers: 'list() answers the same array until the registry changes; save() passes a mode only when one is given.',
    plugins: [],
    engine: { documents: { a: doc({ responses: [{ path: 'download', value: { $bytes: 'JVBERi0=' } }] }) } },
    steps: [start, open(bytes('a')), docs('list'), docs('list'), docs('save', 'a', {}), docs('save', 'a', { mode: 'incremental' })],
  },
  {
    name: 'same-plugin-installed-twice',
    covers: 'The same plugin definition twice in the list fails construction.',
    plugins: [probe({ id: 'x', token: 'x' }), probe({ id: 'x', sameAs: 'x' })],
    steps: [],
  },
  {
    name: 'two-plugins-share-an-id',
    covers: 'Two different definitions with one id fail construction.',
    plugins: [probe({ id: 'x' }), probe({ id: 'x' })],
    steps: [],
  },
  {
    name: 'two-providers-for-a-token',
    covers: 'Two plugins providing one token fail construction, naming both.',
    plugins: [probe({ id: 'x', token: 't' }), probe({ id: 'y', token: 't' })],
    steps: [],
  },
  {
    name: 'workspace-requires-document-scoped',
    covers: 'A workspace plugin that requires a document-scoped token fails construction with the remedy.',
    plugins: [probe(docProbe('alpha')), probe({ id: 'ws', requires: ['alpha'] })],
    steps: [],
  },
  {
    name: 'undeclared-resolution-refuses',
    covers:
      'In development a plugin that resolves a token it did not declare is refused, by get, tryGet, forDocument and tryForDocument alike; the documents token and its own token are always declared.',
    plugins: [probe(docProbe('alpha')), probe(docProbe('beta')), probe({ id: 'ws', token: 'ws' }), probe({ id: 'ws2', token: 'ws2', optional: ['alpha'] })],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      cap('beta', 'get', ['alpha'], 'a'),
      cap('beta', 'tryGet', ['alpha'], 'a'),
      cap('beta', 'forDocument', ['alpha', 'a'], 'a'),
      cap('beta', 'tryForDocument', ['alpha', 'a'], 'a'),
      cap('beta', 'get', ['documents'], 'a'),
      cap('beta', 'get', ['beta'], 'a'),
      cap('ws', 'tryForDocument', ['alpha', 'a']),
      cap('ws2', 'tryForDocument', ['alpha', 'a']),
      cap('ws2', 'tryForDocument', ['alpha', 'zzz']),
    ],
  },
];

/** `meter` (see `Metered` in `scenario.ts`): enough turns to outlast every
 *  chain these cases start. */
const metered = (step: Step, turns = 40): Step => ({ ...step, meter: turns }) as Step;

const turnCases: ScenarioCase[] = [
  {
    name: 'open-settles-on-its-turn',
    covers:
      "open() resolves on the turn TypeScript's does: the operation's `.finally` takes three turns after the open body, and the `async` openDocument wrapper two more to adopt it.",
    snapshot: 'none',
    reason: 'Pins turn counts: what matters is where `@@settled` falls among the `@@turn` callbacks.',
    plugins: [probe(docProbe('alpha'))],
    engine: { documents: { a: doc() } },
    steps: [start, metered(open(bytes('a')))],
  },
  {
    name: 'a-failed-open-rejects-on-its-turn',
    covers: 'The same turns on the failure path: the rollback, then the `.finally` and the adoption.',
    snapshot: 'none',
    reason: 'Pins turn counts: what matters is where `@@settled` falls among the `@@turn` callbacks.',
    engine: {
      documents: { a: doc() },
      responses: [{ path: 'open', times: 1, error: { name: 'EngineError', message: 'bad xref', code: 'Format' } }],
    },
    steps: [start, metered(open(bytes('a')))],
  },
  {
    name: 'unlock-settles-on-its-turn',
    covers: 'unlock() is an `async` function returning the operation, so it adopts it in two turns, like open().',
    snapshot: 'none',
    reason: 'Pins turn counts: what matters is where `@@settled` falls among the `@@turn` callbacks.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: {
        a: doc({
          security: { passwordPrompt: { state: 'required' } },
          responses: [
            { path: 'security.unlock', args: [{ password: 'wrong' }], error: { name: 'EngineError', message: 'incorrect password', code: 'DocPasswordIncorrect' } },
            { path: 'security.unlock', args: [{ password: 'right' }], value: { security: {} } },
          ],
        }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      metered(docs('unlock', 'a', { password: 'wrong' })),
      metered(docs('unlock', 'a', { password: 'right' })),
    ],
  },
  {
    name: 'retry-settles-on-its-turn',
    covers: "retry() closes the failed tab, then returns openDocument's promise from an `async` arrow: the open's turns plus one adoption.",
    snapshot: 'none',
    reason: 'Pins turn counts: what matters is where `@@settled` falls among the `@@turn` callbacks.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: { a: doc() },
      responses: [{ path: 'open', times: 1, error: { name: 'EngineError', message: 'bad xref', code: 'Format' } }],
    },
    steps: [start, open(bytes('a')), metered(docs('retry', 'a'))],
  },
  {
    name: 'close-joins-a-thunk-open-on-its-turn',
    covers:
      'close() while a thunk source is pending: the open rejects after its `.finally` and adoption, and close joins the operation with `await operation?.catch(...)`, two turns, before it disposes the scope and emits `closed`.',
    snapshot: 'none',
    reason: 'Pins turn counts: what matters is where `@@settled` and the listener fall among the `@@turn` callbacks.',
    plugins: [probe(docProbe('alpha'))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      docs('onClosed', { $fn: 'closed' }),
      open({ $fn: 'source', defer: 'source' }),
      metered(docs('close', 'pending:1')),
    ],
  },
  {
    name: 'destroy-joins-start-on-its-turn',
    covers:
      'destroy() after a settled start: `await startPromise?.catch(...)` is two turns before the workspace scope disposes.',
    snapshot: 'none',
    reason: 'Pins turn counts: what matters is where `@@settled` and the teardown fall among the `@@turn` callbacks.',
    plugins: [probe({ id: 'ws', token: 'ws', effects: { cleanup: true } })],
    steps: [start, metered(destroy)],
  },
  {
    name: 'close-joins-an-open-on-its-turn',
    covers:
      'close() during pages.list: the open rejects after its `.finally` and adoption, and close joins the operation with `await operation?.catch(...)`, two turns, before it disposes the scope and emits `closed`.',
    snapshot: 'none',
    reason: 'Pins turn counts: what matters is where `@@settled` and the listener fall among the `@@turn` callbacks.',
    plugins: [probe(docProbe('alpha'))],
    engine: { documents: { a: doc({ responses: [{ path: 'pages.list', defer: 'list-a' }] }) } },
    steps: [
      start,
      docs('onClosed', { $fn: 'closed' }),
      open(bytes('a')),
      metered(docs('close', 'a')),
    ],
  },
  {
    name: 'close-joins-a-pending-init-on-its-turn',
    covers:
      'close() while a plugin init is pending waits for the init; when it settles, the open rejects and close finishes, each on the turn TypeScript reaches it.',
    snapshot: 'none',
    reason: 'Pins turn counts: what matters is where `@@settled` and the listener fall among the `@@turn` callbacks.',
    plugins: [probe(docProbe('alpha', { init: { defer: 'init' } }))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      docs('onClosed', { $fn: 'closed' }),
      open(bytes('a')),
      docs('close', 'a'),
      metered(settle('init')),
    ],
  },
  {
    name: 'close-of-a-ready-document-on-its-turn',
    covers: 'close() with no operation in flight: `await session.operation?.catch(...)` on null is one turn.',
    snapshot: 'none',
    reason: 'Pins turn counts: what matters is where `@@settled` and the listener fall among the `@@turn` callbacks.',
    plugins: [probe(docProbe('alpha'))],
    engine: { documents: { a: doc() } },
    steps: [start, docs('onClosed', { $fn: 'closed' }), open(bytes('a')), metered(docs('close', 'a'))],
  },
  {
    name: 'destroy-closes-a-document-on-its-turn',
    covers:
      'destroy() with a document open: the start join, then the session close, whose scope awaits `handle.close()`, an `AbortablePromise` that `await` adopts in three turns, then the workspace scope.',
    snapshot: 'none',
    reason: 'Pins turn counts: what matters is where `@@settled` and the teardowns fall among the `@@turn` callbacks.',
    plugins: [probe(docProbe('alpha')), probe({ id: 'ws', token: 'ws', effects: { cleanup: true } })],
    engine: { documents: { a: doc() } },
    steps: [start, docs('onClosed', { $fn: 'closed' }), open(bytes('a')), metered(destroy)],
  },
  {
    name: 'bring-up-awaits-the-render-policy-on-its-turn',
    covers:
      'Bring-up reads `await session.handle.render?.policy()` from the raw engine handle: an `AbortablePromise`, which `await` adopts as a thenable, two turns more than a plain promise.',
    snapshot: 'none',
    reason: 'Pins turn counts: what matters is where `@@settled` and the init fall among the `@@turn` callbacks.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: {
        a: doc({ services: ['render'], responses: [{ path: 'render.policy', value: { kind: 'continuous' } }] }),
      },
    },
    steps: [start, metered(open(bytes('a')))],
  },
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'kernel/turns.json',
    describes:
      'packages/core/main: the microtask turn on which each document transition and the kernel teardown settle, measured with the runner\'s turn meter.',
    cases: turnCases,
  },
  {
    kind: 'scenario',
    file: 'kernel/lifecycle.json',
    describes: 'packages/core/main: the kernel, its store, plan, scopes, document lifecycle and the v3 controller contract.',
    cases: [...cases, ...v3Cases],
  },
];
