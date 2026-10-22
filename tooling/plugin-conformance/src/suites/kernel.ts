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
import type {
  DocumentEvent,
  DocumentMetadata,
  DocumentSecurityState,
  EventOrigin,
  PageLayout,
  PageRef,
  PageTextSnapshot,
} from '@embedpdf/engine-core/runtime';
const box = { x: 0, y: 0, width: 600, height: 800 };
const origin: EventOrigin = { kind: 'local', sessionId: 'fake', sub: null, ts: 0, serverId: null };
const page = (pon: number, index: number): PageLayout => ({
  index,
  ref: { kind: 'objectNumber', objectNumber: pon },
  label: null,
  size: { width: 600, height: 800 },
  rotation: 0,
  userUnit: 1,
  tabOrder: null,
  pdfCropBox: { left: 0, bottom: 0, right: 600, top: 800 },
  boxes: {
    media: { ...box },
    crop: { ...box },
    bleed: { ...box },
    trim: { ...box },
    art: { ...box },
  },
});
const metadata: DocumentMetadata = {
  title: null,
  author: null,
  subject: null,
  keywords: null,
  producer: null,
  creator: null,
  createdAt: null,
  modifiedAt: null,
  trapped: 'unknown',
};
const unlockedSecurity: DocumentSecurityState = {
  encryption: { state: 'none', requiresPassword: false },
  permissions: {
    known: true,
    bits: null,
    allAllowed: true,
    openedAs: 'none',
    securityHandlerRevision: null,
    canUpgradeToOwner: false,
  },
  access: { required: false, reasons: [] },
};
const mutationMeta = { affectedPages: [], cacheDelta: null, opId: 'write', undoable: true };
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
const docs = (method: string, ...args: unknown[]): Step => ({
  op: 'call',
  target: 'documents',
  method,
  args,
});
const kernel = (method: string, ...args: unknown[]): Step => ({
  op: 'call',
  target: 'kernel',
  method,
  args,
});
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
const listing: Step[] = [docs('list'), docs('getOrder'), docs('getCount'), docs('getActiveId')];
/** A document-scoped probe with every lifecycle member switched on. */
const docProbe = (id: string, extra: Partial<ProbeConfig> = {}): ProbeConfig => ({
  id,
  scope: 'document',
  token: id,
  state: { n: 0 },
  connect: { cleanup: true, watchState: true },
  ...extra,
});
// ── the v3 contract: create/connect controllers, leases, documents API ─────────
/** The members of a `create` probe's api, in object order: a capability
 *  printed as an outcome registers one ref per member, in this order. */
const API = [
  'instanceId',
  'state',
  'replaceState',
  'mergeState',
  'sameState',
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
  'assertPageRef',
  'getPage',
  'get',
  'tryGet',
  'forDocument',
  'tryForDocument',
  'cleanup',
  'signalReason',
  'document',
  'documentId',
  'handleId',
  'handleFor',
  'pageOf',
  'notify',
] as const;
/** The ref of an api member when the capability printed at `base`. */
const at = (member: (typeof API)[number], base = 0) => base + API.indexOf(member);
const callRef = (ref: number, ...args: unknown[]): Step => ({ op: 'call', target: { ref }, args });
const api = (member: (typeof API)[number], ...args: unknown[]) => callRef(at(member), ...args);
const ref1 = (pon: number): PageRef => ({ kind: 'objectNumber', objectNumber: pon });
const U = { $undefined: true };
/** A document-scoped `create` probe. */
const ctl = (id: string, extra: Partial<ProbeConfig> = {}): ProbeConfig => ({
  id,
  scope: 'document',
  token: id,
  state: { n: 0 },
  ...extra,
});
/** A workspace `create` probe. */
const wsCtl = (id: string, extra: Partial<ProbeConfig> = {}): ProbeConfig => ({
  id,
  token: id,
  state: { n: 0 },
  ...extra,
});
const capOf = (token: string, docId?: string): Step =>
  kernel('capability', { $token: token }, ...(docId ? [docId] : []));
const cases: ScenarioCase[] = [
  {
    name: 'open-a-document',
    covers:
      'Open reserves a loading slot, lists pages, creates and connects plugins in dependency order, and publishes one ready document.',
    plugins: [probe(docProbe('alpha')), probe({ id: 'workspace', token: 'ws', connect: {} })],
    engine: { documents: { a: doc({ pages: [page(1, 0), page(7, 1)] }) } },
    steps: [
      start,
      open(bytes('a'), { name: 'Report' }),
      ...listing,
      docs('get', 'a'),
      docs('has', 'a'),
      docs('has', 'zzz'),
      docs('get', 'zzz'),
    ],
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
    steps: [
      start,
      open(bytes('a'), { activate: false }),
      docs('getActiveId'),
      open(bytes('b'), { activate: false }),
      docs('getActiveId'),
    ],
  },
  {
    name: 'open-all',
    covers:
      '`openAll` reserves every slot at once in array order, selects exactly the `active` entry (else the first), and turns a failure into tab state rather than a rejection.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: { a: doc(), b: doc(), c: doc() },
      responses: [
        {
          path: 'open',
          args: [bytes('c'), {}],
          error: { name: 'EngineError', message: 'broken file', code: 'Format' },
        },
      ],
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
    steps: [
      start,
      docs('openAll', [{ source: bytes('a') }, { source: bytes('b') }]),
      docs('getActiveId'),
    ],
  },
  {
    name: 'close-while-opening',
    covers:
      'close() during the engine open: the tab disappears synchronously, open() rejects with the typed cancellation, and a handle that lands afterwards is closed rather than leaked.',
    plugins: [probe(docProbe('alpha'))],
    engine: { documents: { a: doc() }, responses: [{ path: 'open', defer: 'open-a' }] },
    steps: [
      start,
      open(bytes('a')),
      docs('close', 'a'),
      ...listing,
      settle('open-a', { $document: 'a' }),
      docs('close', 'a'),
    ],
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
    name: 'close-during-policy',
    covers:
      'Close during the final asynchronous bring-up read joins the policy call, then cancels open without constructing document plugins.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: { a: doc({ responses: [{ path: 'render.getPolicy', defer: 'policy' }] }) },
    },
    steps: [
      start,
      open(bytes('a')),
      docs('close', 'a'),
      ...listing,
      settle('policy', { kind: 'continuous' }),
    ],
  },
  {
    name: 'close-during-policy-then-policy-fails',
    covers:
      'A policy read that rejects after close is still cancelled at the checkpoint: no ready or error tab and no plugin construction.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: { a: doc({ responses: [{ path: 'render.getPolicy', defer: 'policy' }] }) },
    },
    steps: [
      start,
      open(bytes('a')),
      docs('close', 'a'),
      fail('policy', 'policy failed late'),
      ...listing,
    ],
  },
  {
    name: 'open-failure-parks-an-error-tab',
    covers:
      'An engine refusal after the slot is reserved: rollback, then an `error` tab that is closable and reopenable, with the rejection carried on the pending slot.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: { a: doc() },
      responses: [
        {
          path: 'open',
          times: 1,
          error: { name: 'EngineError', message: 'bad xref', code: 'Format' },
        },
      ],
    },
    steps: [
      start,
      open(bytes('a')),
      ...listing,
      docs('get', 'a'),
      open(bytes('a')),
      docs('close', 'a'),
      ...listing,
      open(bytes('a')),
      ...listing,
    ],
  },
  {
    name: 'bring-up-failure-rolls-back',
    covers:
      'A document create failure unwinds acquired resources, closes its handle, and parks an error tab without publishing ready.',
    plugins: [
      probe(docProbe('alpha')),
      probe(docProbe('beta', { createThrows: { message: 'init broke' }, requires: ['alpha'] })),
    ],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), ...listing, docs('get', 'a')],
  },
  {
    name: 'connection-setup-failure-rolls-back',
    covers: 'A connect failure rolls back document bring-up.',
    plugins: [
      probe(
        docProbe('alpha', { connect: { throws: { message: 'effects broke' }, cleanup: true } }),
      ),
    ],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), ...listing],
  },
  {
    name: 'pages-list-failure',
    covers: 'pages.list rejecting is an open failure like any other.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: {
        a: doc({
          responses: [
            {
              path: 'pages.list',
              error: { name: 'EngineError', message: 'no pages', code: 'Format' },
            },
          ],
        }),
      },
    },
    steps: [start, open(bytes('a')), ...listing],
  },
  {
    name: 'duplicate-open-refuses',
    covers: 'Opening an id that already has a session rejects conflict and changes nothing.',
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
          security: { passwordPrompt: { state: 'required', hint: null, incorrect: false } },
          responses: [
            {
              path: 'security.unlock',
              args: [{ password: 'wrong' }],
              error: {
                name: 'EngineError',
                message: 'incorrect password',
                code: 'DocPasswordIncorrect',
              },
            },
            {
              path: 'security.unlock',
              args: [{ password: 'right' }],
              value: { security: unlockedSecurity },
            },
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
    covers:
      'A password supplied at open, in the options or on the input, marks the locked tab `passwordProvided` (the "incorrect" copy).',
    plugins: [],
    engine: {
      documents: {
        a: doc({
          security: { passwordPrompt: { state: 'required', hint: null, incorrect: false } },
        }),
        b: doc({
          security: { passwordPrompt: { state: 'required', hint: null, incorrect: false } },
        }),
        c: doc({
          security: { passwordPrompt: { state: 'required', hint: null, incorrect: false } },
        }),
      },
    },
    steps: [
      start,
      open(bytes('a'), { password: 'x' }),
      open(bytes('b', { password: 'y' })),
      open(bytes('c')),
      docs('list'),
    ],
  },
  {
    name: 'an-undefined-password-is-not-provided',
    covers:
      'A `password` member that is `undefined`, in the options or on the input, is not a supplied password: the locked tab is not `passwordProvided`.',
    plugins: [],
    engine: {
      documents: {
        a: doc({
          security: { passwordPrompt: { state: 'required', hint: null, incorrect: false } },
        }),
        b: doc({
          security: { passwordPrompt: { state: 'required', hint: null, incorrect: false } },
        }),
      },
    },
    steps: [
      start,
      open(bytes('a'), { password: { $undefined: true } }),
      open(bytes('b', { password: { $undefined: true } })),
      docs('list'),
    ],
  },
  {
    name: 'unlock-concurrency-and-cancellation',
    covers:
      'A second unlock while one is in flight refuses (one transition at a time); closing during an unlock cancels it; unlock on an unknown document refuses.',
    plugins: [],
    engine: {
      documents: {
        a: doc({
          security: { passwordPrompt: { state: 'required', hint: null, incorrect: false } },
          responses: [{ path: 'security.unlock', defer: 'unlock' }],
        }),
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
          security: { passwordPrompt: { state: 'required', hint: null, incorrect: false } },
          responses: [
            { path: 'security.unlock', value: { security: unlockedSecurity } },
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
          security: { passwordPrompt: { state: 'required', hint: null, incorrect: false } },
          responses: [
            { path: 'security.unlock', value: { security: unlockedSecurity } },
            { path: 'pages.list', defer: 'list' },
          ],
        }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      docs('unlock', 'a', { password: 'p' }),
      docs('close', 'a'),
      ...listing,
    ],
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
    steps: [
      start,
      open(bytes('taken')),
      open(bytes('requested')),
      open(bytes('other')),
      ...listing,
    ],
  },
  {
    name: 'input-without-id',
    covers: 'An input that carries no id gets a ticket, then takes the handle id.',
    plugins: [],
    engine: { documents: { h: doc() }, responses: [{ path: 'open', value: { $document: 'h' } }] },
    steps: [start, open({ kind: 'bytes', bytes: { $bytes: '' } }), ...listing],
  },
  {
    name: 'page-registry-events',
    covers:
      'Confirmed page events update the registry and revision; metadata events do not change the registry.',
    plugins: [probe(docProbe('alpha', { connect: {} }))],
    engine: { documents: { a: doc({ pages: [page(1, 0), page(2, 1)] }) } },
    steps: [
      start,
      open(bytes('a')),
      {
        op: 'emit',
        doc: 'a',
        event: {
          origin,
          meta: mutationMeta,
          type: 'pages.rotated',
          pages: [ref1(1)],
          rotation: 90,
          layout: {
            namedPages: [],
            pageCount: 2,
            pages: [{ ...page(1, 0), rotation: 90 }, page(2, 1)],
          },
        } satisfies DocumentEvent,
      },
      {
        op: 'emit',
        doc: 'a',
        event: {
          origin,
          meta: mutationMeta,
          type: 'pages.reordered',
          pages: [ref1(2)],
          layout: { namedPages: [], pageCount: 2, pages: [page(2, 0), page(1, 1)] },
        } satisfies DocumentEvent,
      },
      {
        op: 'emit',
        doc: 'a',
        event: {
          origin,
          meta: mutationMeta,
          type: 'pages.deleted',
          pages: [ref1(1)],
          layout: { namedPages: [], pageCount: 1, pages: [page(2, 0)] },
        } satisfies DocumentEvent,
      },
      {
        op: 'emit',
        doc: 'a',
        event: {
          origin,
          meta: mutationMeta,
          type: 'pages.inserted',
          insertedPages: [ref1(9)],
          layout: { namedPages: [], pageCount: 2, pages: [page(2, 0), page(9, 1)] },
        } satisfies DocumentEvent,
      },
      {
        op: 'emit',
        doc: 'a',
        event: {
          origin,
          meta: mutationMeta,
          type: 'metadata.updated',
          metadata,
        } satisfies DocumentEvent,
      },
      docs('close', 'a'),
    ],
  },
  {
    name: 'render-policy',
    covers:
      'Bring-up reads getPolicy once, falling back to continuous when the read fails or yields null.',
    plugins: [],
    engine: {
      documents: {
        a: doc({
          services: ['render'],
          responses: [
            {
              path: 'render.getPolicy',
              value: { kind: 'lattice', fullPage: { widths: [256, 512] } },
            },
          ],
        }),
        b: doc({
          services: ['render'],
          responses: [{ path: 'render.getPolicy', error: { message: 'offline' } }],
        }),
        c: doc(),
        d: doc({ services: ['render'], responses: [{ path: 'render.getPolicy', value: null }] }),
      },
    },
    steps: [start, open(bytes('a')), open(bytes('b')), open(bytes('c')), open(bytes('d'))],
  },
  {
    name: 'an-undefined-render-policy-is-continuous',
    covers: 'A render service whose policy answers `undefined` means `continuous`, as `null` does.',
    plugins: [],
    engine: {
      documents: {
        a: doc({
          services: ['render'],
          responses: [{ path: 'render.getPolicy', value: { $undefined: true } }],
        }),
      },
    },
    steps: [start, open(bytes('a'))],
  },
  {
    name: 'capability-resolution',
    covers:
      'Workspace singletons; document instances built lazily, once per document; the truthful refusals for no provider, no active document, an unknown document and a loading one; tryCapability counting only ready documents; scopeOf.',
    plugins: [probe({ id: 'ws', token: 'ws' }), probe(docProbe('alpha')), probe({ id: 'bare' })],
    engine: {
      documents: { a: doc(), b: doc() },
      responses: [{ path: 'open', args: [bytes('b'), {}], defer: 'open-b' }],
    },
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
      'Declared capabilities resolve within the bound document and across named documents; workspace document reads have no bound metadata.',
    plugins: [
      probe(docProbe('alpha')),
      probe(docProbe('beta', { requires: ['alpha'], optional: ['missing'] })),
      probe({ id: 'ws', token: 'ws', optional: ['alpha'] }),
    ],
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
      kernel('getState'),
    ],
  },
  {
    name: 'state-transitions-and-watch',
    covers:
      'Pure state transitions commit to the bound slice. Returning the current object emits no state change; replacement and merge emit one committed change each.',
    plugins: [probe(docProbe('alpha', { connect: { watchState: true } }))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      cap('alpha', 'sameState', []),
      cap('alpha', 'mergeState', [{ m: 1 }]),
      cap('alpha', 'replaceState', [{ n: 5 }]),
      cap('alpha', 'sameState', []),
      cap('alpha', 'state'),
    ],
  },
  {
    name: 'active-document-and-order',
    covers:
      'setActive (a pending tab is selectable, an unknown id is ignored), move with clamping, swap, and the next-active rule when the active tab closes: the tab before it, else the first.',
    plugins: [probe({ id: 'ws', token: 'ws', connect: { watchActive: true } })],
    engine: {
      documents: { a: doc(), b: doc(), c: doc(), d: doc() },
      responses: [{ path: 'open', args: [bytes('d'), {}], defer: 'open-d' }],
    },
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
    steps: [
      start,
      open(bytes('a')),
      open(bytes('b')),
      docs('setActive', 'a'),
      docs('close', 'a'),
      docs('getActiveId'),
      docs('close', 'b'),
      docs('getActiveId'),
    ],
  },
  {
    name: 'document-io',
    covers:
      'Downloads enforce permissions, local layer downloads require a local document, and canPrint is false without a host printer.',
    plugins: [],
    engine: {
      documents: {
        a: doc({
          local: true,
          security: { allows: ['doc.print', 'doc.download'] },
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
      docs('canPrint'),
      open(bytes('a')),
      docs('download', undefined, { mode: 'rewrite' }),
      docs('downloadLayer'),
      docs('canPrint'),
      docs('canDownload'),
      open(bytes('b')),
      docs('downloadLayer', 'b'),
      docs('canDownload', 'b'),
      docs('canPrint', 'zzz'),
    ],
  },
  {
    name: 'start-lifecycle',
    covers:
      'Workspace create runs at construction, connect runs at start in dependency order, and subsequent starts join the first.',
    plugins: [
      probe({ id: 'late', token: 'late', requires: ['early'], connect: {} }),
      probe({ id: 'early', token: 'early', connect: {} }),
    ],
    steps: [kernel('status'), start, kernel('status'), start, kernel('status'), start],
  },
  {
    name: 'start-failure',
    covers:
      'A workspace connect failure fails start, unwinds resources, and refuses later use while destroy remains legal.',
    plugins: [
      probe({ id: 'ws', token: 'ws', connect: { throws: { message: 'boot failed' } } }),
      probe({ id: 'teardown', token: 'td', connect: { cleanup: true } }),
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
    name: 'destroy-closes-everything',
    covers:
      'destroy closes every session (a mid-close one included), unwinds document scopes LIFO in reverse dependency order, then the workspace scope, then refuses further use; a teardown registered after disposal runs immediately.',
    plugins: [
      probe(docProbe('alpha', { connect: { cleanup: true } })),
      probe(
        docProbe('beta', { requires: ['alpha'], connect: { cleanup: { defer: 'beta-teardown' } } }),
      ),
      probe({ id: 'ws', token: 'ws', connect: { cleanup: true } }),
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
    steps: [
      start,
      open(bytes('a')),
      { op: 'call', target: 'kernel', method: 'capability', args: [{ $token: 'alpha' }, 'a'] },
      docs('close', 'a'),
      { op: 'call', target: { ref: at('cleanup') }, args: ['after-close'] },
    ],
  },
  {
    name: 'teardown-failures-are-reported',
    covers:
      'A teardown that throws is reported and the rest still run; a listener that throws is reported and its siblings still run.',
    plugins: [
      probe(docProbe('alpha', { connect: { cleanup: { throws: { message: 'teardown broke' } } } })),
      probe(
        docProbe('beta', {
          connect: { cleanup: { rejects: { message: 'async teardown broke' } } },
        }),
      ),
    ],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), docs('close', 'a')],
  },
  {
    name: 'missing-dependency',
    covers:
      "A required token nobody provides fails construction, and the message carries the token's own remedy.",
    plugins: [
      probe({ id: 'needs', requires: ['absent'] }),
      probe({ id: 'provider', token: 'other', hint: 'unused' }),
    ],
    steps: [],
  },
  {
    name: 'missing-dependency-with-hint',
    covers: 'The hint is appended to the missing-dependency message.',
    plugins: [
      probe({
        id: 'needs',
        requires: ['needed-token'],
        requiresHint: { 'needed-token': "add neededPlugin() from '@x/y' to your plugins list" },
      }),
    ],
    steps: [],
  },
  {
    name: 'dependency-cycle',
    covers: 'A requires cycle fails construction naming a plugin in it.',
    plugins: [
      probe({ id: 'a', token: 'ta', requires: ['tb'] }),
      probe({ id: 'b', token: 'tb', requires: ['ta'] }),
    ],
    steps: [],
  },
  {
    name: 'plan-order',
    covers:
      'Required and present optional dependencies construct and connect before dependents; optional self-dependency is not a cycle.',
    plugins: [
      probe({ id: 'c', token: 'tc', requires: ['tb'], connect: true }),
      probe({ id: 'b', token: 'tb', optional: ['ta', 'tb', 'nobody'], connect: true }),
      probe({ id: 'a', token: 'ta', connect: true }),
    ],
    steps: [start],
  },
  {
    name: 'capability-constructor-throws',
    covers: 'A workspace create failure rejects kernel construction.',
    plugins: [probe({ id: 'ws', token: 'ws', createThrows: { message: 'cannot build' } })],
    steps: [],
  },
  {
    name: 'document-capability-constructor-throws',
    covers: 'A document create failure parks an error tab; public resolution refuses that tab.',
    plugins: [
      probe(docProbe('alpha', { connect: undefined, createThrows: { message: 'cannot build' } })),
    ],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      kernel('capability', { $token: 'alpha' }),
      kernel('tryCapability', { $token: 'alpha' }),
    ],
  },
  {
    name: 'listener-isolation',
    covers:
      'A throwing state listener is reported without undoing the committed transition or preventing the trace observer from seeing it.',
    plugins: [
      probe({
        id: 'thrower',
        token: 'thrower',
        connect: { watchState: true, listenerThrows: { message: 'listener broke' } },
      }),
      probe({ id: 'sibling', token: 'sibling', connect: { watchState: true } }),
    ],
    steps: [start, cap('thrower', 'replaceState', [{ n: 1 }])],
  },
  {
    name: 'an-abort-error-from-the-engine-reads-as-cancelled',
    covers:
      'An engine call rejecting with AbortError when nobody closed the tab: the open rejects as cancelled and the tab stays loading, which is what isAbortLike does to an abort the kernel did not start.',
    plugins: [],
    engine: {
      documents: {
        a: doc({
          responses: [
            { path: 'pages.list', error: { name: 'AbortError', message: 'worker aborted' } },
          ],
        }),
      },
    },
    steps: [start, open(bytes('a')), ...listing],
  },
  {
    name: 'an-abort-error-after-unlock-reads-as-cancelled',
    covers: 'The same rule past the password.',
    plugins: [],
    engine: {
      documents: {
        a: doc({
          security: { passwordPrompt: { state: 'required', hint: null, incorrect: false } },
          responses: [
            { path: 'security.unlock', value: { security: unlockedSecurity } },
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
    engine: {
      plainPromises: true,
      documents: { a: doc() },
      responses: [{ path: 'open', defer: 'open-a' }],
    },
    steps: [
      start,
      open(bytes('a')),
      docs('close', 'a'),
      ...listing,
      settle('open-a', { $document: 'a' }),
    ],
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
    plugins: [probe(docProbe('alpha', { connect: { cleanup: { defer: 'teardown' } } }))],
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
            {
              path: 'pages.list',
              error: { name: 'EngineError', message: 'no pages', code: 'Format' },
            },
            { path: 'close', defer: 'handle-close' },
          ],
        }),
      },
    },
    steps: [start, open(bytes('a')), docs('close', 'a'), settle('handle-close'), ...listing],
  },
  {
    name: 'page-event-before-commit',
    covers:
      'A page mutation during the policy read arrives before kernel event subscriptions are attached.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: {
        a: doc({ pages: [page(1, 0)], responses: [{ path: 'render.getPolicy', defer: 'policy' }] }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      {
        op: 'emit',
        doc: 'a',
        event: {
          origin,
          meta: mutationMeta,
          type: 'pages.rotated',
          pages: [ref1(1)],
          rotation: 90,
          layout: { namedPages: [], pageCount: 1, pages: [{ ...page(1, 0), rotation: 90 }] },
        } satisfies DocumentEvent,
      },
      settle('policy', { kind: 'continuous' }),
      docs('get', 'a'),
    ],
  },
  {
    name: 'try-get-across-scopes',
    covers:
      "A document plugin's tryGet resolves a workspace capability; a workspace plugin's tryGet of a document capability is null while the active document is still loading; allows() is false for a loading document.",
    plugins: [
      probe({ id: 'ws', token: 'ws', optional: ['alpha'] }),
      probe(docProbe('alpha')),
      probe(docProbe('beta', { optional: ['ws'] })),
    ],
    engine: {
      documents: { a: doc({ security: { allows: '*' } }), b: doc({ security: { allows: '*' } }) },
      responses: [{ path: 'open', args: [bytes('b'), {}], defer: 'open-b' }],
    },
    steps: [
      start,
      open(bytes('a')),
      cap('beta', 'tryGet', ['ws'], 'a'),
      open(bytes('b')),
      cap('ws', 'tryGet', ['alpha']),
      cap('ws', 'handleFor', []),
      cap('ws', 'document'),
      docs('canPrint'),
      docs('canPrint', 'a'),
      settle('open-b', { $document: 'b' }),
      cap('ws', 'tryGet', ['alpha']),
    ],
  },
  {
    name: 'a-capability-outlives-its-document',
    covers:
      'A retained state setter is revoked at close, reads retain the last state, and document metadata remains staged.',
    plugins: [probe(docProbe('alpha', { connect: undefined }))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      kernel('capability', { $token: 'alpha' }, 'a'),
      docs('close', 'a'),
      api('replaceState', { n: 9 }),
      api('state'),
      api('document'),
    ],
  },
  {
    name: 'a-listener-updating-state',
    covers:
      'A state change listener can apply a second pure transition; committed changes and listener order remain observable.',
    plugins: [
      probe({
        id: 'relay',
        token: 'relay',
        connect: { watchState: true, watchMergeOnce: { relayed: true } },
      }),
      probe({ id: 'after', token: 'after', connect: { watchActive: true } }),
    ],
    steps: [start, cap('relay', 'replaceState', [{ n: 1 }]), cap('relay', 'state')],
  },
  {
    name: 'capability-of-an-error-or-closing-tab',
    covers:
      'Resolution refuses truthfully for a tab that failed and for one whose close is still disposing.',
    plugins: [probe(docProbe('alpha', { connect: { cleanup: { defer: 'teardown' } } }))],
    engine: {
      documents: { a: doc(), bad: doc() },
      responses: [
        {
          path: 'open',
          args: [bytes('bad'), {}],
          error: { name: 'EngineError', message: 'broken', code: 'Format' },
        },
      ],
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
      'Public tryCapability is null during the policy read. During connect, after leases exist but before ready is published, the bound context can tryGet a declared sibling.',
    plugins: [
      probe(docProbe('alpha', { optional: ['beta'], connect: { tryGet: 'beta' } })),
      probe(docProbe('beta')),
    ],
    engine: {
      documents: { a: doc({ responses: [{ path: 'render.getPolicy', defer: 'policy' }] }) },
    },
    steps: [
      start,
      open(bytes('a')),
      kernel('tryCapability', { $token: 'alpha' }, 'a'),
      settle('policy', { kind: 'continuous' }),
      cap('alpha', 'tryGet', ['beta'], 'a'),
      cap('alpha', 'documentId', [], 'a'),
    ],
  },
  {
    name: 'an-abort-error-from-unlock-reads-as-cancelled',
    covers:
      'security.unlock rejecting with AbortError, with no close: the unlock rejects as cancelled and the tab stays locked.',
    plugins: [],
    engine: {
      documents: {
        a: doc({
          security: { passwordPrompt: { state: 'required', hint: null, incorrect: false } },
          responses: [
            { path: 'security.unlock', error: { name: 'AbortError', message: 'worker aborted' } },
          ],
        }),
      },
    },
    steps: [start, open(bytes('a')), docs('unlock', 'a', { password: 'p' }), docs('list')],
  },
  {
    name: 'a-rekeyed-slot-carries-its-new-id',
    covers:
      'A ticket re-keyed to a document that then parks as locked keeps the real id on its pending slot.',
    plugins: [],
    engine: {
      documents: {
        real: doc({
          security: { passwordPrompt: { state: 'required', hint: null, incorrect: false } },
        }),
      },
    },
    steps: [
      start,
      open({ $fn: 'source', resolves: bytes('real') }),
      docs('list'),
      docs('get', 'real'),
    ],
  },
  {
    name: 'closing-one-document-leaves-the-others-listening',
    covers:
      'Closing one document releases only its subscriptions; state changes on other documents remain observable.',
    plugins: [probe(docProbe('alpha', { connect: { watchState: true } }))],
    engine: { documents: { a: doc(), b: doc(), c: doc() } },
    steps: [
      start,
      open(bytes('a')),
      open(bytes('b')),
      open(bytes('c')),
      docs('close', 'b'),
      cap('alpha', 'replaceState', [{ n: 1 }], 'a'),
      cap('alpha', 'replaceState', [{ n: 2 }], 'c'),
    ],
  },
  {
    name: 'a-layout-without-pages',
    covers:
      'A page event whose layout carries a count and no page list: the registry copies what it is given, so the meta has no pages member rather than an empty one.',
    plugins: [],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      {
        op: 'emit',
        doc: 'a',
        event: {
          origin,
          meta: mutationMeta,
          type: 'pages.deleted',
          pages: [ref1(2)],
          layout: { namedPages: [], pageCount: 0 },
        },
      },
    ],
  },
  {
    name: 'a-rekey-onto-a-closing-id-refuses',
    covers:
      'A ticket whose input names a document that is still closing refuses the re-key: the closing session holds the id even though its slot has left the store.',
    plugins: [probe(docProbe('alpha', { connect: { cleanup: { defer: 'teardown' } } }))],
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
    covers:
      'Between the re-key and the engine answering, the pending slot already carries the real id.',
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
const v3Cases: ScenarioCase[] = [
  {
    name: 'create-and-connect-order',
    covers:
      'Workspace create runs at construction and connect at start. Document instances create then connect in dependency order before ready is published.',
    plugins: [
      probe(docProbe('alpha')),
      probe(ctl('ctl', { requires: ['alpha'], connect: {} })),
      probe(wsCtl('wsc', { connect: true })),
    ],
    engine: { documents: { a: doc(), b: doc() } },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      cap('ctl', 'instanceId', [], 'a'),
      cap('wsc', 'instanceId'),
      cap('ctl', 'replaceState', [{ n: 1 }], 'a'),
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
    plugins: [probe(wsCtl('wsc', { createThrows: { message: 'cannot create' } }))],
    steps: [],
  },
  {
    name: 'create-throws-in-bring-up',
    covers: 'A document `create` that throws rolls the open back to an error tab.',
    plugins: [probe(ctl('ctl', { createThrows: { message: 'cannot create' } }))],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), ...listing],
  },
  {
    name: 'connect-throws-in-bring-up',
    covers: 'A document connect that throws rolls the open back too.',
    plugins: [probe(ctl('ctl', { connect: { throws: { message: 'cannot connect' } } }))],
    engine: { documents: { a: doc() } },
    steps: [start, open(bytes('a')), ...listing],
  },
  {
    name: 'connect-throws-at-start',
    covers: 'A workspace connect that throws fails start.',
    plugins: [probe(wsCtl('wsc', { connect: { throws: { message: 'cannot connect' } } }))],
    steps: [start, kernel('status')],
  },
  {
    name: 'connect-updates-and-listens',
    covers:
      'Connect can update its state, read staged pages, and subscribe to document and capability events; all subscriptions end at close.',
    plugins: [
      probe(
        ctl('ctl', {
          connect: { replaceState: { n: 7 }, listenDoc: true, listenActive: true, readPage: 1 },
        }),
      ),
    ],
    engine: { documents: { a: doc(), b: doc() } },
    steps: [
      start,
      open(bytes('a')),
      cap('ctl', 'state', [], 'a'),
      {
        op: 'emit',
        doc: 'a',
        event: {
          origin,
          meta: mutationMeta,
          type: 'metadata.updated',
          metadata,
        } satisfies DocumentEvent,
      },
      open(bytes('b')),
      docs('setActive', 'a'),
      docs('close', 'a'),
      docs('setActive', 'b'),
    ],
  },
  {
    name: 'guarded-handle',
    covers:
      'Guarded calls reach the engine, missing page service calls reject not-found, and pending or later calls reject instance-closed after close.',
    plugins: [probe(ctl('ctl'))],
    engine: {
      documents: {
        a: doc({
          security: { allows: ['doc.print'], scope: ['doc.print'] },
          responses: [
            { path: 'metadata.get', times: 1, value: { ...metadata, title: 'T' } },
            { path: 'metadata.get', defer: 'slow' },
            {
              path: 'page.text.get',
              pon: 1,
              value: { text: '', charCount: 0 } satisfies PageTextSnapshot,
            },
          ],
        }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('docCall', 'metadata.get'),
      api('pageCall', 1, 'text.get'),
      api('pageCall', 99, 'text.get'),
      api('allows', 'doc.print'),
      api('allows', 'doc.annotate'),
      api('docId'),
      api('docRead', 'objectNumbers.held'),
      api('docRead', 'security.scope'),
      api('docRead', 'id'),
      api('docCall', 'metadata.get'),
      docs('close', 'a'),
      settle('slow', { ...metadata, title: 'late' }),
      api('docCall', 'metadata.get'),
      api('replaceState', { n: 3 }),
      api('replaceState', { n: 4 }),
      api('state'),
    ],
  },
  {
    name: 'guarded-handle-maps-engine-errors',
    covers:
      'An engine rejection through the guard is a PluginError from the mapping table; a plain rejection is operation-failed and an AbortError operation-cancelled.',
    plugins: [probe(ctl('ctl'))],
    engine: {
      documents: {
        a: doc({
          responses: [
            {
              path: 'metadata.get',
              times: 1,
              error: {
                name: 'EngineError',
                message: 'no such',
                code: 'NotFound',
                details: { key: 'k' },
              },
            },
            {
              path: 'metadata.get',
              times: 1,
              error: { name: 'EngineError', message: 'nope', code: 'Forbidden' },
            },
            {
              path: 'metadata.get',
              times: 1,
              error: { name: 'EngineError', message: 'odd', code: 'Format' },
            },
            { path: 'metadata.get', times: 1, error: { message: 'plain failure' } },
            {
              path: 'metadata.get',
              times: 1,
              error: { name: 'AbortError', message: 'worker aborted' },
            },
          ],
        }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('docCall', 'metadata.get'),
      api('docCall', 'metadata.get'),
      api('docCall', 'metadata.get'),
      api('docCall', 'metadata.get'),
      api('docCall', 'metadata.get'),
    ],
  },
  {
    name: 'guarded-handle-over-plain-promises',
    covers:
      'Over an engine whose calls cannot be aborted, the guard still settles, maps errors, and rejects a pending call at close without an abort.',
    plugins: [probe(ctl('ctl'))],
    engine: {
      plainPromises: true,
      documents: {
        a: doc({
          responses: [
            { path: 'metadata.get', times: 1, value: { ...metadata, title: 'T' } },
            {
              path: 'metadata.get',
              times: 1,
              error: { name: 'EngineError', message: 'gone', code: 'NotFound' },
            },
            { path: 'metadata.get', defer: 'slow' },
          ],
        }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('docCall', 'metadata.get'),
      api('docCall', 'metadata.get'),
      api('docCall', 'metadata.get'),
      docs('close', 'a'),
      settle('slow', metadata),
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
      api('replaceState', { n: 1 }),
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
    covers:
      'serialQueue(key): one operation at a time per key, in submission order, a failure does not poison the next, and two keys are independent.',
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
    name: 'controller-pages',
    covers:
      'getPage, pageOf and assertPageRef use the live page registry. A workspace context has no bound page or document; declared capabilities cross scopes.',
    plugins: [probe(ctl('ctl')), probe(wsCtl('wsc', { optional: ['ctl'] }))],
    engine: {
      documents: {
        a: doc({
          pages: [
            page(1, 0),
            {
              ...page(2, 1),
              size: { width: 300, height: 400 },
              pdfCropBox: { left: 10, bottom: 20, right: 310, top: 420 },
              boxes: {
                media: { x: 0, y: 0, width: 300, height: 400 },
                crop: { x: 0, y: 0, width: 300, height: 400 },
                bleed: { x: 0, y: 0, width: 300, height: 400 },
                trim: { x: 0, y: 0, width: 300, height: 400 },
                art: { x: 0, y: 0, width: 300, height: 400 },
              },
            },
          ],
        }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('pageOf', ref1(2)),
      api('pageOf', 1),
      api('pageOf', ref1(99)),
      api('assertPageRef', 2),
      api('assertPageRef', 99),
      api('getPage', 2),
      api('getPage', 99),
      {
        op: 'emit',
        doc: 'a',
        event: {
          origin,
          meta: mutationMeta,
          type: 'pages.deleted',
          pages: [ref1(2)],
          layout: { namedPages: [], pageCount: 1, pages: [page(1, 0)] },
        } satisfies DocumentEvent,
      },
      api('getPage', 2),
      api('pageOf', ref1(1)),
      api('pageOf', ref1(2)),
      cap('wsc', 'instanceId'),
      cap('wsc', 'docId'),
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
    name: 'invalid-page-identities',
    covers:
      'Missing object-number addresses return null; a valid page with a zero top edge in PDF space retains its layout and resolves by index or ref.',
    plugins: [probe(ctl('ctl'))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      {
        op: 'emit',
        doc: 'a',
        event: {
          origin,
          meta: mutationMeta,
          type: 'pages.deleted',
          pages: [ref1(2)],
          layout: {
            namedPages: [],
            pageCount: 3,
            pages: [
              { ...page(1, 0), ref: { kind: 'objectNumber', objectNumber: 1.5 } },
              { ...page(1, 1), ref: { kind: 'objectNumber', objectNumber: -1 } },
              {
                ...page(3, 2),
                size: { width: 100, height: 100 },
                pdfCropBox: { left: 5, bottom: -100, right: 105, top: 0 },
                boxes: {
                  media: { x: 0, y: 0, width: 100, height: 100 },
                  crop: { x: 0, y: 0, width: 100, height: 100 },
                  bleed: { x: 0, y: 0, width: 100, height: 100 },
                  trim: { x: 0, y: 0, width: 100, height: 100 },
                  art: { x: 0, y: 0, width: 100, height: 100 },
                },
              },
            ],
          },
        } satisfies DocumentEvent,
      },
      api('getPage', 1),
      api('getPage', 0),
      api('getPage', 3),
      api('pageOf', ref1(3)),
      api('pageOf', 2),
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
      'Guarded security exposes state, scope, identity, password prompts, annotation authorization and the event cursor.',
    plugins: [probe(ctl('ctl'))],
    engine: {
      documents: {
        a: doc({
          security: {
            identity: { userId: 'u1' },
            state: {
              encryption: { state: 'none', requiresPassword: false },
              permissions: {
                known: true,
                bits: null,
                allAllowed: true,
                openedAs: 'owner',
                securityHandlerRevision: null,
                canUpgradeToOwner: false,
              },
              access: { required: false, reasons: [] },
            },
            passwordPrompt: { state: 'none' },
            allowsAnnotation: { create: true, update: true, delete: false, 'set-group': true },
          },
        }),
        b: doc({ security: { allowsAnnotation: { create: false, 'set-group': false } } }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      capOf('ctl', 'a'),
      api('docRead', 'security.identity'),
      api('docRead', 'security.state'),
      api('docRead', 'security.passwordPrompt'),
      api('docRead', 'security.allowsAnnotation', 'create'),
      api('docRead', 'security.allowsAnnotation', 'update', {}),
      api('docRead', 'security.allowsAnnotation', 'delete', {}),
      api('docRead', 'security.allowsAnnotation', 'set-group', { groupId: 'g1' }),
      api('docRead', 'events.lastServerId'),
      open(bytes('b')),
      cap('ctl', 'docRead', ['security.allowsAnnotation', 'create'], 'b'),
      cap('ctl', 'docRead', ['security.allowsAnnotation', 'set-group', { groupId: 'g1' }], 'b'),
    ],
  },
  {
    name: 'workspace-controller-at-destroy',
    covers:
      'The workspace lifetime ends at destroy: a pending lane run and waitFor reject instance-closed, an acquisition that lands afterwards is disposed, cleanups run with the workspace scope, and a state update after destroy is reported once.',
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
      api('replaceState', { n: 1 }),
      api('replaceState', { n: 2 }),
      api('state'),
    ],
  },
  {
    name: 'leases-across-reopen',
    covers:
      'A slice lease ends when its instance closes: a retained state setter is dropped and reported once per context, state reads the last value, and a reopened document gets a new lease the old context cannot reach.',
    plugins: [probe(docProbe('alpha', { connect: undefined }))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      open(bytes('a')),
      capOf('alpha', 'a'),
      cap('alpha', 'replaceState', [{ n: 1 }], 'a'),
      docs('close', 'a'),
      api('replaceState', { n: 9 }),
      api('replaceState', { n: 10 }),
      api('state'),
      open(bytes('a')),
      api('replaceState', { n: 11 }),
      cap('alpha', 'state', [], 'a'),
      cap('alpha', 'replaceState', [{ n: 12 }], 'a'),
      cap('alpha', 'state', [], 'a'),
    ],
  },
  {
    name: 'documents-page-registry',
    covers:
      'listPages, getPage, getPageAt, getPageIndex and getRevision read the active document by default or a named one, answer empty values with no document, and follow a page event.',
    plugins: [],
    engine: {
      documents: { a: doc({ pages: [page(1, 0), page(7, 1)] }), b: doc({ pages: [page(3, 0)] }) },
    },
    steps: [
      start,
      docs('listPages'),
      docs('getPage', ref1(1)),
      docs('getPage', 0),
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
      docs('getPage', 1),
      docs('getPage', 5),
      docs('getPageIndex', ref1(7)),
      docs('getPageIndex', ref1(99)),
      docs('getRevision'),
      {
        op: 'emit',
        doc: 'a',
        event: {
          origin,
          meta: mutationMeta,
          type: 'pages.reordered',
          pages: [ref1(2)],
          layout: { namedPages: [], pageCount: 2, pages: [page(7, 0), page(1, 1)] },
        } satisfies DocumentEvent,
      },
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
        locked: doc({
          security: { passwordPrompt: { state: 'required', hint: null, incorrect: false } },
        }),
      },
      responses: [
        {
          path: 'open',
          args: [bytes('bad'), {}],
          error: { name: 'EngineError', message: 'broken', code: 'MalformedPdf' },
        },
      ],
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
      {
        op: 'emit',
        doc: 'a',
        event: {
          origin,
          meta: mutationMeta,
          type: 'pages.rotated',
          pages: [ref1(1)],
          rotation: 90,
          layout: { namedPages: [], pageCount: 1, pages: [{ ...page(1, 0), rotation: 90 }] },
        } satisfies DocumentEvent,
      },
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
    plugins: [probe(docProbe('alpha', { connect: {} }))],
    engine: {
      documents: { a: doc(), b: doc(), bad: doc() },
      responses: [
        {
          path: 'open',
          args: [bytes('bad'), { password: 'p' }],
          times: 1,
          error: { name: 'EngineError', message: 'broken', code: 'MalformedPdf' },
        },
      ],
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
    covers:
      'openAll returns the reserved ids in order: a thunk source its ticket, and a refused reservation (an id already open) the id it asked for, leaving the existing tab alone.',
    plugins: [],
    engine: { documents: { a: doc(), c: doc(), real: doc() } },
    steps: [
      start,
      open(bytes('a')),
      docs('openAll', [
        { source: bytes('a') },
        { source: bytes('c'), active: true },
        { source: { $fn: 'source', resolves: bytes('real') } },
      ]),
      ...listing,
    ],
  },
  {
    name: 'capability-built-before-its-lease',
    covers:
      'A document capability resolved while bring-up is still reading the render policy, before the slice leases exist, refuses: the context has no lease to read or write through.',
    plugins: [probe(docProbe('alpha')), probe({ id: 'ws', token: 'ws', optional: ['alpha'] })],
    engine: {
      documents: {
        a: doc({
          services: ['render'],
          responses: [{ path: 'render.getPolicy', defer: 'policy' }],
        }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      cap('ws', 'tryForDocument', ['alpha', 'a']),
      settle('policy', null),
      cap('ws', 'tryForDocument', ['alpha', 'a']),
    ],
  },
  {
    name: 'list-and-save-details',
    covers: 'Repeated document reads remain stable, and download forwards the requested mode.',
    plugins: [],
    engine: {
      documents: {
        a: doc({
          security: { allows: ['doc.download'] },
          responses: [{ path: 'download', value: { $bytes: 'JVBERi0=' } }],
        }),
      },
    },
    steps: [
      start,
      open(bytes('a')),
      docs('list'),
      docs('list'),
      docs('download', 'a', {}),
      docs('download', 'a', { mode: 'incremental' }),
    ],
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
    covers:
      'A workspace plugin that requires a document-scoped token fails construction with the remedy.',
    plugins: [probe(docProbe('alpha')), probe({ id: 'ws', requires: ['alpha'] })],
    steps: [],
  },
  {
    name: 'undeclared-resolution-refuses',
    covers:
      'In development a plugin that resolves a token it did not declare is refused, by get, tryGet, forDocument and tryForDocument alike; the documents token and its own token are always declared.',
    plugins: [
      probe(docProbe('alpha')),
      probe(docProbe('beta')),
      probe({ id: 'ws', token: 'ws' }),
      probe({ id: 'ws2', token: 'ws2', optional: ['alpha'] }),
    ],
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
    reason:
      'Pins turn counts: what matters is where `@@settled` falls among the `@@turn` callbacks.',
    plugins: [probe(docProbe('alpha'))],
    engine: { documents: { a: doc() } },
    steps: [start, metered(open(bytes('a')))],
  },
  {
    name: 'a-failed-open-rejects-on-its-turn',
    covers:
      'The same turns on the failure path: the rollback, then the `.finally` and the adoption.',
    snapshot: 'none',
    reason:
      'Pins turn counts: what matters is where `@@settled` falls among the `@@turn` callbacks.',
    engine: {
      documents: { a: doc() },
      responses: [
        {
          path: 'open',
          times: 1,
          error: { name: 'EngineError', message: 'bad xref', code: 'Format' },
        },
      ],
    },
    steps: [start, metered(open(bytes('a')))],
  },
  {
    name: 'a-throwing-thunk-rejects-on-its-turn',
    covers:
      'A thunk source that throws synchronously never reaches `engineCall`: the open fails in its own turn, then the `.finally` and the adoption.',
    snapshot: 'none',
    reason:
      'Pins turn counts: what matters is where `@@settled` falls among the `@@turn` callbacks.',
    steps: [start, metered(open({ $fn: 'throwing', throws: { message: 'bad url' } }))],
  },
  {
    name: 'unlock-settles-on-its-turn',
    covers:
      'unlock() is an `async` function returning the operation, so it adopts it in two turns, like open().',
    snapshot: 'none',
    reason:
      'Pins turn counts: what matters is where `@@settled` falls among the `@@turn` callbacks.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: {
        a: doc({
          security: { passwordPrompt: { state: 'required', hint: null, incorrect: false } },
          responses: [
            {
              path: 'security.unlock',
              args: [{ password: 'wrong' }],
              error: {
                name: 'EngineError',
                message: 'incorrect password',
                code: 'DocPasswordIncorrect',
              },
            },
            {
              path: 'security.unlock',
              args: [{ password: 'right' }],
              value: { security: unlockedSecurity },
            },
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
    covers:
      "retry() closes the failed tab, then returns openDocument's promise from an `async` arrow: the open's turns plus one adoption.",
    snapshot: 'none',
    reason:
      'Pins turn counts: what matters is where `@@settled` falls among the `@@turn` callbacks.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: { a: doc() },
      responses: [
        {
          path: 'open',
          times: 1,
          error: { name: 'EngineError', message: 'bad xref', code: 'Format' },
        },
      ],
    },
    steps: [start, open(bytes('a')), metered(docs('retry', 'a'))],
  },
  {
    name: 'close-joins-a-thunk-open-on-its-turn',
    covers:
      'close() while a thunk source is pending: the open rejects after its `.finally` and adoption, and close joins the operation with `await operation?.catch(...)`, two turns, before it disposes the scope and emits `closed`.',
    snapshot: 'none',
    reason:
      'Pins turn counts: what matters is where `@@settled` and the listener fall among the `@@turn` callbacks.',
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
    reason:
      'Pins turn counts: what matters is where `@@settled` and the teardown fall among the `@@turn` callbacks.',
    plugins: [probe({ id: 'ws', token: 'ws', connect: { cleanup: true } })],
    steps: [start, metered(destroy)],
  },
  {
    name: 'close-joins-an-open-on-its-turn',
    covers:
      'close() during pages.list: the open rejects after its `.finally` and adoption, and close joins the operation with `await operation?.catch(...)`, two turns, before it disposes the scope and emits `closed`.',
    snapshot: 'none',
    reason:
      'Pins turn counts: what matters is where `@@settled` and the listener fall among the `@@turn` callbacks.',
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
    name: 'close-joins-a-pending-policy-on-its-turn',
    covers:
      'Close joins the pending policy read; when it settles the open rejects cancelled and close emits closed on their measured turns.',
    snapshot: 'none',
    reason: 'Pins turn counts: where @@settled and closed fall among @@turn callbacks.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: { a: doc({ responses: [{ path: 'render.getPolicy', defer: 'policy' }] }) },
    },
    steps: [
      start,
      docs('onClosed', { $fn: 'closed' }),
      open(bytes('a')),
      docs('close', 'a'),
      metered(settle('policy', { kind: 'continuous' })),
    ],
  },
  {
    name: 'close-of-a-ready-document-on-its-turn',
    covers:
      'close() with no operation in flight: `await session.operation?.catch(...)` on null is one turn.',
    snapshot: 'none',
    reason:
      'Pins turn counts: what matters is where `@@settled` and the listener fall among the `@@turn` callbacks.',
    plugins: [probe(docProbe('alpha'))],
    engine: { documents: { a: doc() } },
    steps: [
      start,
      docs('onClosed', { $fn: 'closed' }),
      open(bytes('a')),
      metered(docs('close', 'a')),
    ],
  },
  {
    name: 'destroy-closes-a-document-on-its-turn',
    covers:
      'destroy() with a document open: the start join, then the session close, whose scope awaits `handle.close()`, an `AbortablePromise` that `await` adopts in three turns, then the workspace scope.',
    snapshot: 'none',
    reason:
      'Pins turn counts: what matters is where `@@settled` and the teardowns fall among the `@@turn` callbacks.',
    plugins: [
      probe(docProbe('alpha')),
      probe({ id: 'ws', token: 'ws', connect: { cleanup: true } }),
    ],
    engine: { documents: { a: doc() } },
    steps: [start, docs('onClosed', { $fn: 'closed' }), open(bytes('a')), metered(destroy)],
  },
  {
    name: 'bring-up-awaits-the-render-policy-on-its-turn',
    covers:
      'Bring-up awaits the raw render.getPolicy AbortablePromise before constructing document plugins.',
    snapshot: 'none',
    reason:
      'Pins turn counts: what matters is where `@@settled` and create falls among the `@@turn` callbacks.',
    plugins: [probe(docProbe('alpha'))],
    engine: {
      documents: {
        a: doc({
          services: ['render'],
          responses: [{ path: 'render.getPolicy', value: { kind: 'continuous' } }],
        }),
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
      "packages/core/main: the microtask turn on which each document transition and the kernel teardown settle, measured with the runner's turn meter.",
    cases: turnCases,
  },
  {
    kind: 'scenario',
    file: 'kernel/lifecycle.json',
    describes:
      'packages/core/main: the kernel, its store, plan, scopes, document lifecycle and the v3 controller contract.',
    cases: [...cases, ...v3Cases],
  },
];
