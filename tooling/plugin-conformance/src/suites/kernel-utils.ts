/**
 * K0's pure helpers: `docInfoEquals` and `docInfoListEquals` from
 * `packages/core/main/src/types.ts`; `createEventHook`, driven by a script
 * since its behaviour is a sequence; `composeApi`; and the error vocabulary
 * (`toPluginError`, `isPluginError`, `toPluginErrorInfo`, `originOf`).
 * Replayed by `crates/plugin-conformance/tests/kernel.rs`.
 *
 * `createSerialQueue` is not here: it is asynchronous, and a function case is
 * synchronous. `crates/epdf-kernel/src/serial_queue.rs` pins it with a unit
 * test that mirrors the TypeScript's own.
 */
import {
  CancelledError,
  PluginError,
  composeApi,
  createEventHook,
  docInfoEquals,
  docInfoListEquals,
  isPluginError,
  originOf,
  toPluginError,
  toPluginErrorInfo,
  type DocInfo,
} from '@embedpdf/core';
import { PermissionDenied } from '@embedpdf/engine-core/runtime';

import { makeError, type ErrorSpec } from '../fake-engine';
import type { FunctionCase, Suite } from '../vectors';

type HookOp =
  | { op: 'on'; id: string }
  | { op: 'onThrow'; id: string; message: string }
  | { op: 'onAdd'; id: string; adds: string }
  | { op: 'off'; id: string }
  | { op: 'offTwice'; id: string }
  | { op: 'emit'; event: unknown }
  | { op: 'dispose' }
  /** Subscribe tied to a named AbortController; `aborted` pre-aborts it. */
  | { op: 'onSignal'; id: string; signal: string }
  | { op: 'abort'; signal: string };

/** Run a script against one event hook and return what happened, in order. */
function eventHookScript(ops: HookOp[], withErrorSink: boolean): unknown[] {
  const log: unknown[] = [];
  const hook = createEventHook<unknown>(
    withErrorSink ? (error) => log.push({ error: (error as Error).message }) : undefined,
  );
  const offs = new Map<string, () => void>();
  const controllers = new Map<string, AbortController>();
  const signalOf = (name: string) => {
    let controller = controllers.get(name);
    if (!controller) {
      controller = new AbortController();
      controllers.set(name, controller);
    }
    return controller.signal;
  };
  const listen = (id: string, then?: () => void, signal?: AbortSignal) => {
    offs.set(
      id,
      hook.on(
        (event) => {
          log.push({ id, event });
          then?.();
        },
        signal ? { signal } : undefined,
      ),
    );
  };
  for (const step of ops) {
    switch (step.op) {
      case 'on':
        listen(step.id);
        break;
      case 'onThrow':
        listen(step.id, () => {
          throw new Error(step.message);
        });
        break;
      case 'onAdd': {
        let added = false;
        listen(step.id, () => {
          if (!added) {
            added = true;
            listen(step.adds);
          }
        });
        break;
      }
      case 'off':
        offs.get(step.id)?.();
        break;
      case 'offTwice':
        offs.get(step.id)?.();
        offs.get(step.id)?.();
        break;
      case 'emit':
        log.push({ emit: step.event });
        hook.emit(step.event);
        break;
      case 'dispose':
        hook.dispose();
        log.push('disposed');
        break;
      case 'onSignal':
        listen(step.id, undefined, signalOf(step.signal));
        break;
      case 'abort':
        signalOf(step.signal);
        controllers.get(step.signal)!.abort();
        log.push({ aborted: step.signal });
        break;
    }
  }
  return log;
}

/** `composeApi` over slices named by their keys; each member's value is its slice index. */
function composeApiScript(slices: string[][]): unknown {
  const api = composeApi(
    'probe',
    slices.map((keys, index) => Object.fromEntries(keys.map((key) => [key, index]))),
  ) as Record<string, number>;
  return Object.entries(api);
}

/** An error the vocabulary can meet, including the classes a table cannot
 *  make: `PermissionDenied`, the kernel's `CancelledError`, a `PluginError`. */
type ErrorInput =
  | ErrorSpec
  | { class: 'PermissionDenied'; required: string; context?: string }
  | { class: 'CancelledError'; what: string }
  | { class: 'PluginError'; code: string; message: string; details?: unknown }
  | { class: 'string'; value: string };

function errorOf(input: ErrorInput): unknown {
  const special = input as { class?: string; required: string; context?: string; what: string; code: string; message: string; details?: unknown; value: string };
  if (special.class === 'PermissionDenied') return new PermissionDenied(special.required, special.context);
  if (special.class === 'CancelledError') return new CancelledError(special.what);
  if (special.class === 'PluginError') {
    return new PluginError(special.code as never, 'origin', special.message, special.details === undefined ? {} : { details: special.details });
  }
  if (special.class === 'string') return special.value;
  return makeError(input as ErrorSpec);
}

const info = (over: Partial<DocInfo> = {}): DocInfo => ({ id: 'a', status: 'ready', pageCount: 3, ...over });

const cases: FunctionCase[] = [
  { name: 'equal', fn: 'docInfoEquals', args: [info(), info()] },
  { name: 'id-differs', fn: 'docInfoEquals', args: [info(), info({ id: 'b' })] },
  { name: 'name-differs', fn: 'docInfoEquals', args: [info({ name: 'A' }), info({ name: 'B' })] },
  { name: 'name-absent-vs-present', fn: 'docInfoEquals', args: [info(), info({ name: 'A' })] },
  { name: 'status-differs', fn: 'docInfoEquals', args: [info(), info({ status: 'loading' })] },
  { name: 'page-count-differs', fn: 'docInfoEquals', args: [info(), info({ pageCount: 4 })] },
  { name: 'password-provided-differs', fn: 'docInfoEquals', args: [info({ passwordProvided: true }), info({ passwordProvided: false })] },
  { name: 'password-provided-absent-vs-false', fn: 'docInfoEquals', args: [info(), info({ passwordProvided: false })] },
  { name: 'lists-equal', fn: 'docInfoListEquals', args: [[info(), info({ id: 'b' })], [info(), info({ id: 'b' })]] },
  { name: 'lists-empty', fn: 'docInfoListEquals', args: [[], []] },
  { name: 'lists-length-differs', fn: 'docInfoListEquals', args: [[info()], [info(), info({ id: 'b' })]] },
  { name: 'lists-order-differs', fn: 'docInfoListEquals', args: [[info(), info({ id: 'b' })], [info({ id: 'b' }), info()]] },
  { name: 'lists-one-member-differs', fn: 'docInfoListEquals', args: [[info()], [info({ status: 'error' })]] },
  { name: 'list-is-itself', fn: 'docInfoListEqualsItself', args: [[info()]] },
  {
    name: 'error-code-differs',
    fn: 'docInfoEquals',
    args: [
      info({ status: 'error', error: { code: 'not-found', message: 'm', capability: 'documents' } }),
      info({ status: 'error', error: { code: 'operation-failed', message: 'm', capability: 'documents' } }),
    ],
  },
  {
    name: 'error-message-differs',
    fn: 'docInfoEquals',
    args: [
      info({ status: 'error', error: { code: 'not-found', message: 'm', capability: 'documents' } }),
      info({ status: 'error', error: { code: 'not-found', message: 'n', capability: 'documents' } }),
    ],
  },
  {
    name: 'error-capability-is-not-compared',
    fn: 'docInfoEquals',
    args: [
      info({ status: 'error', error: { code: 'not-found', message: 'm', capability: 'documents' } }),
      info({ status: 'error', error: { code: 'not-found', message: 'm', capability: 'other' } }),
    ],
  },
  {
    name: 'error-absent-vs-present',
    fn: 'docInfoEquals',
    args: [info({ status: 'error' }), info({ status: 'error', error: { code: 'not-found', message: 'm', capability: 'documents' } })],
  },
  {
    name: 'hook-fans-out-in-order',
    fn: 'eventHookScript',
    args: [[{ op: 'on', id: 'a' }, { op: 'on', id: 'b' }, { op: 'emit', event: 1 }], true],
  },
  {
    name: 'hook-unsubscribe-and-twice',
    fn: 'eventHookScript',
    args: [[{ op: 'on', id: 'a' }, { op: 'on', id: 'b' }, { op: 'offTwice', id: 'a' }, { op: 'emit', event: 2 }], true],
  },
  {
    name: 'hook-snapshot-per-emit',
    covers: 'A listener added during an emit is not called by that emit, and is by the next.',
    fn: 'eventHookScript',
    args: [[{ op: 'onAdd', id: 'a', adds: 'late' }, { op: 'emit', event: 1 }, { op: 'emit', event: 2 }], true],
  },
  {
    name: 'hook-isolates-a-throwing-listener',
    fn: 'eventHookScript',
    args: [[{ op: 'onThrow', id: 'bad', message: 'listener broke' }, { op: 'on', id: 'good' }, { op: 'emit', event: 'x' }], true],
  },
  {
    name: 'hook-swallows-without-a-sink',
    fn: 'eventHookScript',
    args: [[{ op: 'onThrow', id: 'bad', message: 'listener broke' }, { op: 'on', id: 'good' }, { op: 'emit', event: 'x' }], false],
  },
  {
    name: 'hook-dispose',
    covers: 'After dispose nothing is delivered, on() returns an inert unsubscribe, and unsubscribing a pre-dispose listener is harmless.',
    fn: 'eventHookScript',
    args: [
      [
        { op: 'on', id: 'a' },
        { op: 'dispose' },
        { op: 'emit', event: 1 },
        { op: 'on', id: 'b' },
        { op: 'emit', event: 2 },
        { op: 'off', id: 'a' },
        { op: 'off', id: 'b' },
      ],
      true,
    ],
  },
  {
    name: 'hook-signal-ends-a-subscription',
    covers: 'A listener subscribed with a signal is removed when it aborts; one subscribed with a pre-aborted signal never registers.',
    fn: 'eventHookScript',
    args: [
      [
        { op: 'onSignal', id: 'a', signal: 's' },
        { op: 'on', id: 'b' },
        { op: 'emit', event: 1 },
        { op: 'abort', signal: 's' },
        { op: 'emit', event: 2 },
        { op: 'onSignal', id: 'c', signal: 's' },
        { op: 'emit', event: 3 },
        { op: 'off', id: 'c' },
      ],
      true,
    ],
  },
  {
    name: 'hook-signal-after-dispose',
    covers: 'After dispose a subscription with a live signal is inert, and aborting that signal later is harmless.',
    fn: 'eventHookScript',
    args: [[{ op: 'dispose' }, { op: 'onSignal', id: 'a', signal: 's' }, { op: 'abort', signal: 's' }, { op: 'emit', event: 1 }], true],
  },
  ...vocabularyCases(),
  { name: 'compose-disjoint', fn: 'composeApiScript', args: [[['a', 'b'], ['c'], []]] },
  { name: 'compose-empty', fn: 'composeApiScript', args: [[]] },
  { name: 'compose-duplicate-refuses', covers: 'A key two slices define throws naming the capability and the key.', fn: 'composeApiScript', args: [[['a', 'b'], ['c', 'b']]] },
  { name: 'compose-duplicate-within-a-later-slice', fn: 'composeApiScript', args: [[['a'], ['a']]] },
  {
    name: 'origin-local',
    fn: 'originOf',
    args: [{ type: 'metadata.updated', origin: { kind: 'local', sessionId: 's1', sub: 'u1' } }],
  },
  {
    name: 'origin-remote-without-actor',
    fn: 'originOf',
    args: [{ type: 'metadata.updated', origin: { kind: 'remote', sessionId: 's2', sub: null } }],
  },
];

/** `toPluginError` over every row of its engine-code table and every class it
 *  tells apart, then `isPluginError` and `toPluginErrorInfo`. */
function vocabularyCases(): FunctionCase[] {
  const codes = [
    'InvalidArg',
    'MalformedPdf',
    'WireFormat',
    'DocNotOpen',
    'DocPasswordRequired',
    'DocPasswordIncorrect',
    'SharePasswordRequired',
    'Unauthenticated',
    'Forbidden',
    'ProtectedDocument',
    'NotFound',
    'InvalidReference',
    'NotImplemented',
    'Aborted',
    'WeakAnnotationSessionConflict',
    'LayerVersionConflict',
    'StaleBase',
    'SigningPending',
    'SigningExpired',
    'SigningVersionMismatch',
    'Format',
  ];
  const out: FunctionCase[] = codes.map((code) => ({
    name: `engine-code-${code}`,
    fn: 'toPluginError',
    args: [{ name: 'EngineError', message: `engine said ${code}`, code }],
  }));
  out.push(
    { name: 'engine-error-with-details', fn: 'toPluginError', args: [{ name: 'EngineError', message: 'gone', code: 'NotFound', details: { pon: 3 } }] },
    { name: 'engine-error-without-code', fn: 'toPluginError', args: [{ name: 'EngineError', message: 'no code' }] },
    { name: 'permission-denied', fn: 'toPluginError', args: [{ class: 'PermissionDenied', required: 'doc.print' }] },
    { name: 'permission-denied-with-context', fn: 'toPluginError', args: [{ class: 'PermissionDenied', required: 'doc.print', context: 'print' }] },
    { name: 'a-plain-error-named-permission-denied', covers: 'PermissionDenied is matched by class: a plain error that only carries the name is a failure.', fn: 'toPluginError', args: [{ name: 'PermissionDenied', message: 'looks like one' }] },
    { name: 'cancelled', fn: 'toPluginError', args: [{ class: 'CancelledError', what: 'closed while opening: a' }] },
    { name: 'a-plain-error-named-cancelled', fn: 'toPluginError', args: [{ name: 'CancelledError', message: 'looks like one' }] },
    { name: 'abort-error', fn: 'toPluginError', args: [{ name: 'AbortError', message: 'aborted' }] },
    { name: 'plugin-error-passes-through', fn: 'toPluginError', args: [{ class: 'PluginError', code: 'conflict', message: 'stale', details: { at: 1 } }] },
    { name: 'plain-error', fn: 'toPluginError', args: [{ message: 'broke' }] },
    { name: 'type-error', fn: 'toPluginError', args: [{ name: 'TypeError', message: 'x is not a function' }] },
    { name: 'not-an-error', fn: 'toPluginError', args: [{ class: 'string', value: 'a string was thrown' }] },
    { name: 'is-plugin-error', fn: 'isPluginError', args: [{ class: 'PluginError', code: 'conflict', message: 'm' }] },
    { name: 'is-plugin-error-with-code', fn: 'isPluginError', args: [{ class: 'PluginError', code: 'conflict', message: 'm' }, 'conflict'] },
    { name: 'is-plugin-error-other-code', fn: 'isPluginError', args: [{ class: 'PluginError', code: 'conflict', message: 'm' }, 'not-found'] },
    { name: 'is-plugin-error-plain', fn: 'isPluginError', args: [{ message: 'm' }] },
    { name: 'error-info', fn: 'toPluginErrorInfo', args: [{ class: 'PluginError', code: 'not-found', message: 'm', details: { x: 1 } }] },
  );
  return out;
}

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'kernel/utils.json',
    describes:
      'packages/core/main: docInfoEquals, docInfoListEquals, createEventHook, composeApi and the plugin error vocabulary.',
    functions: {
      docInfoEquals,
      docInfoListEquals,
      docInfoListEqualsItself: (list: DocInfo[]) => docInfoListEquals(list, list),
      eventHookScript,
      composeApiScript,
      toPluginError: (input: ErrorInput) => toPluginError('probe', errorOf(input)),
      isPluginError: (input: ErrorInput, code?: string) => isPluginError(errorOf(input), code as never),
      toPluginErrorInfo: (input: ErrorInput) => toPluginErrorInfo(errorOf(input) as PluginError),
      originOf,
    },
    cases,
  },
];
