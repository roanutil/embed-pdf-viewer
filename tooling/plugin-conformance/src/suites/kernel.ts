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
  pageObjectNumber: pon,
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
  docs('order'),
  docs('count'),
  docs('activeId'),
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
    steps: [start, open(bytes('a'), { activate: false }), docs('activeId'), open(bytes('b'), { activate: false }), docs('activeId')],
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
    steps: [start, docs('openAll', [{ source: bytes('a') }, { source: bytes('b') }]), docs('activeId')],
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
    plugins: [probe(docProbe('alpha')), probe(docProbe('beta', { requires: ['alpha'] })), probe({ id: 'ws', token: 'ws', optional: ['alpha'] })],
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
      docs('order'),
      docs('move', 'a', -3),
      docs('order'),
      docs('move', 'zzz', 0),
      docs('swap', 'a', 'd'),
      docs('order'),
      docs('swap', 'a', 'zzz'),
      docs('close', 'b'),
      docs('activeId'),
      docs('setActive', 'd'),
      docs('close', 'd'),
      docs('activeId'),
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
    steps: [start, open(bytes('a')), open(bytes('b')), docs('setActive', 'a'), docs('close', 'a'), docs('activeId'), docs('close', 'b'), docs('activeId')],
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
      docs('download'),
      docs('downloadLayer'),
      docs('allows', 'doc.print'),
      open(bytes('a')),
      docs('download', undefined, { mode: 'rewrite' }),
      docs('downloadLayer'),
      docs('allows', 'doc.print'),
      docs('allows', 'doc.download'),
      open(bytes('b')),
      docs('downloadLayer', 'b'),
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
    steps: [start, open(bytes('a')), { op: 'call', target: 'kernel', method: 'capability', args: [{ $token: 'alpha' }, 'a'] }, docs('close', 'a'), { op: 'call', target: { ref: 10 }, args: ['after-close'] }],
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
      docs('activeId'),
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
    plugins: [probe({ id: 'ws', token: 'ws' }), probe(docProbe('alpha')), probe(docProbe('beta'))],
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
    plugins: [probe(docProbe('alpha', { init: { defer: 'init' } })), probe(docProbe('beta'))],
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
];

export const suites: Suite[] = [
  { kind: 'scenario', file: 'kernel/lifecycle.json', describes: 'packages/core/main: the kernel, its store, plan, scopes and document lifecycle.', cases },
];
