/**
 * K0's pure helpers: `docInfoEquals` and `docInfoListEquals` from
 * `packages/core/main/src/types.ts`, and `createEventHook`, driven by a
 * script since its behaviour is a sequence. Replayed by
 * `crates/plugin-conformance/tests/kernel.rs`.
 *
 * `createSerialQueue` is not here: it is asynchronous, and a function case is
 * synchronous. `crates/epdf-kernel/src/serial_queue.rs` pins it with a unit
 * test that mirrors the TypeScript's own.
 */
import { createEventHook, docInfoEquals, docInfoListEquals, type DocInfo } from '@embedpdf/core';

import type { FunctionCase, Suite } from '../vectors';

type HookOp =
  | { op: 'on'; id: string }
  | { op: 'onThrow'; id: string; message: string }
  | { op: 'onAdd'; id: string; adds: string }
  | { op: 'off'; id: string }
  | { op: 'offTwice'; id: string }
  | { op: 'emit'; event: unknown }
  | { op: 'dispose' };

/** Run a script against one event hook and return what happened, in order. */
function eventHookScript(ops: HookOp[], withErrorSink: boolean): unknown[] {
  const log: unknown[] = [];
  const hook = createEventHook<unknown>(
    withErrorSink ? (error) => log.push({ error: (error as Error).message }) : undefined,
  );
  const offs = new Map<string, () => void>();
  const listen = (id: string, then?: () => void) => {
    offs.set(
      id,
      hook.on((event) => {
        log.push({ id, event });
        then?.();
      }),
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
    }
  }
  return log;
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
];

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'kernel/utils.json',
    describes: 'packages/core/main: docInfoEquals, docInfoListEquals and createEventHook.',
    functions: {
      docInfoEquals,
      docInfoListEquals,
      docInfoListEqualsItself: (list: DocInfo[]) => docInfoListEquals(list, list),
      eventHookScript,
    },
    cases,
  },
];
