/**
 * The fake engine: `engine-core`'s `Engine` / `DocumentHandle` contract,
 * answered from a table in the vector rather than from PDFium.
 *
 * The plugins' engine boundary is that contract and nothing else, so a table
 * is enough to drive them, and it keeps the plugin vectors independent of the
 * engine vectors: `tooling/engine-conformance` already pins what the engine
 * answers. What this pins is what the plugins ASK, in what order, and what they
 * do with the answer.
 *
 * `crates/plugin-conformance/src/fake_engine.rs` implements the same rules;
 * `README.md` states them. In short:
 *
 *   - Every asynchronous call is logged as `{doc, path, pon?, args}` and
 *     answered by the first table entry whose `path`, `pon` and `args` match.
 *     An entry with `times` is consumed; one without is sticky.
 *   - An entry answers with `value`, `error`, or `defer: label`. A deferred call
 *     stays pending until a `settle` step names its label (oldest first).
 *   - Calls return `AbortablePromise`; an abort is logged as `{abort: index}`.
 *   - Synchronous reads (`security.allows`, `security.passwordPrompt`, ...) are
 *     answered from `security` and are not logged: they are selectors, and how
 *     often a selector runs is not behaviour.
 *   - A call with no matching entry rejects with `FakeEngineUnmatched`. The
 *     generator refuses to write such a trace unless the case opts in, since it
 *     is almost always a case that forgot a response.
 */
import {
  AbortablePromise,
  PermissionDenied,
  type DocumentHandle,
  type Engine,
} from '@embedpdf/engine-core/runtime';

import { canonical, canonicalKey } from './canonical';

export interface ErrorSpec {
  readonly name?: string;
  readonly message: string;
  readonly [member: string]: unknown;
}

export interface ResponseSpec {
  readonly path: string;
  readonly pon?: number;
  /** Canonical arguments to match exactly; absent matches any. */
  readonly args?: readonly unknown[];
  readonly times?: number;
  readonly value?: unknown;
  readonly error?: ErrorSpec;
  readonly defer?: string;
}

export interface SecuritySpec {
  /** Capabilities `allows()` answers true for; `'*'` is every capability. */
  readonly allows?: readonly string[] | '*';
  readonly allowsAnnotationCreate?: boolean;
  /** `allowsAnnotationMutation(action, target)`; keyed `update` / `delete`. */
  readonly allowsAnnotationMutation?: { readonly update?: boolean; readonly delete?: boolean };
  readonly allowsAnnotationGroupAssignment?: boolean;
  readonly effectiveScope?: readonly string[];
  readonly identity?: unknown;
  readonly current?: unknown;
  /** `passwordPrompt`; `{state: 'required'}` parks an open as locked. */
  readonly passwordPrompt?: unknown;
}

export interface FakeDocumentSpec {
  /** The handle's id; defaults to the table key. */
  readonly id?: string;
  /** Optional services present on this handle: `render`, `actions`,
   *  `attachments`, `pieceInfo`, `redaction`, `downloadLayer`, `page.pieceInfo`,
   *  `page.measure`, `signatures`, `fonts`, `version`, and the optional member
   *  `forms.submit`. */
  readonly services?: readonly string[];
  /** Optional members present by default that this handle lacks:
   *  `forms.applyEffects`, `pages.flatten`, `pages.extract` and
   *  `page.annotations.downloadFile`, which the contract makes optional and
   *  every shipped engine implements. */
  readonly lacks?: readonly string[];
  readonly capabilities?: unknown;
  readonly security?: SecuritySpec;
  /** The page registry (`PageLayout`s, each with its `ref`). `pages.list` answers
   *  from it unless the table says otherwise, and `page(ref)` refuses a ref
   *  whose object number is not in it. */
  readonly pages?: readonly unknown[];
  readonly responses?: readonly ResponseSpec[];
}

export interface FakeEngineSpec {
  readonly documents?: Readonly<Record<string, FakeDocumentSpec>>;
  /** Table for engine-level calls: `open`, `destroy`. `open` answers
   *  `{"$document": key}` to hand out a handle. Absent: `open` hands out the
   *  document whose key is the input's `id`. */
  readonly responses?: readonly ResponseSpec[];
  /** Answer every call with a plain `Promise` that has no `abort`. */
  readonly plainPromises?: boolean;
  /** The document an `open` falls back to when no table entry answers it and
   *  no document is keyed by the input's `id`: a plugin that opens scratch
   *  documents under ids it mints (stamp's asset engine) gets this one. */
  readonly defaultDocument?: string;
}

export type EngineLogEntry =
  | { doc: string | null; path: string; pon?: number; args: unknown[] }
  | { abort: number; reason: unknown };

interface Pending {
  readonly label: string;
  /** An engine call's settlement, as opposed to a scripted callback's. */
  readonly engine?: boolean;
  readonly resolve: (value: unknown) => void;
  readonly reject: (error: unknown) => void;
}

export interface FakeEngine {
  readonly engine: Engine;
  readonly log: EngineLogEntry[];
  readonly unmatched: string[];
  emit(doc: string, event: unknown): void;
  /** Settle the oldest pending call (engine or callback) with this label. */
  settle(label: string, outcome: { value?: unknown; error?: ErrorSpec }): boolean;
  /** Register a pending settlement owned by someone else (a `$fn` callback). */
  defer(label: string, resolve: (value: unknown) => void, reject: (error: unknown) => void): void;
  /** Per-document observable engine-side state: live event subscribers. */
  snapshot(): Record<string, { subscribers: number }>;
}

export function makeError(spec: ErrorSpec): Error {
  const { name, message, ...rest } = spec;
  const error = new Error(message);
  error.name = name ?? 'Error';
  Object.assign(error, rest);
  return error;
}

/** An error the fake ENGINE rejects with. The kernel maps engine refusals by
 *  `instanceof PermissionDenied`, and a real engine throws that class, so a
 *  refusal the table names `PermissionDenied` is one, with the members it
 *  spells. Errors a vector hands in as arguments (`$error`) stay plain. */
export function makeEngineError(spec: ErrorSpec): Error {
  const error = makeError(spec);
  if (error.name === 'PermissionDenied') Object.setPrototypeOf(error, PermissionDenied.prototype);
  return error;
}

export function createFakeEngine(
  spec: FakeEngineSpec,
  decode: (value: unknown) => unknown,
): FakeEngine {
  const log: EngineLogEntry[] = [];
  const unmatched: string[] = [];
  const pending: Pending[] = [];
  const handles = new Map<string, { listeners: Set<(event: unknown) => void> }>();
  const consumed = new Map<ResponseSpec, number>();

  function match(table: readonly ResponseSpec[] | undefined, path: string, pon: number | undefined, args: unknown[]) {
    const key = canonicalKey(args);
    for (const entry of table ?? []) {
      if (entry.path !== path) continue;
      if (entry.pon !== undefined && entry.pon !== pon) continue;
      if (entry.args !== undefined && canonicalKey(entry.args) !== key) continue;
      if (entry.times !== undefined) {
        const used = consumed.get(entry) ?? 0;
        if (used >= entry.times) continue;
        consumed.set(entry, used + 1);
      }
      return entry;
    }
    return null;
  }

  function call(
    doc: string | null,
    table: readonly ResponseSpec[] | undefined,
    path: string,
    pon: number | undefined,
    rawArgs: unknown[],
    fallback?: () => unknown,
    map: (value: unknown) => unknown = (value) => value,
  ): AbortablePromise<unknown> {
    const args = rawArgs.map((arg) => canonical(arg));
    const index = log.length;
    log.push(pon === undefined ? { doc, path, args } : { doc, path, pon, args });
    const executor = (resolve: (value: unknown) => void, reject: (error: unknown) => void) => {
      const entry = match(table, path, pon, args);
      if (!entry) {
        if (fallback) {
          try {
            resolve(map(fallback()));
          } catch (error) {
            reject(error);
          }
          return;
        }
        unmatched.push(`${doc ?? 'engine'}: ${path}${pon === undefined ? '' : `@${pon}`} ${JSON.stringify(args)}`);
        reject(makeError({ name: 'FakeEngineUnmatched', message: `no response for ${path}` }));
        return;
      }
      if (entry.defer !== undefined) {
        pending.push({ label: entry.defer, engine: true, resolve: (value) => resolve(map(value)), reject });
        return;
      }
      if (entry.error) reject(makeEngineError(entry.error));
      else resolve(map(decode(entry.value)));
    };
    // A plain-promise engine is what the kernel's race exists for: a call
    // nobody can abort, whose late result still has to be released.
    if (spec.plainPromises) return new Promise<unknown>(executor) as AbortablePromise<unknown>;
    return new AbortablePromise<unknown>((resolve, reject, _progress, signal) => {
      signal.addEventListener('abort', () => {
        log.push({ abort: index, reason: canonical((signal.reason as { reason?: unknown })?.reason) });
      });
      executor(resolve, reject);
    });
  }

  function service(
    doc: string,
    table: readonly ResponseSpec[] | undefined,
    prefix: string,
    pon?: number,
    absent: ReadonlySet<string> = new Set(),
  ): unknown {
    return new Proxy(
      {},
      {
        get: (_target, member) => {
          if (typeof member !== 'string' || member === 'then' || absent.has(member)) return undefined;
          return (...args: unknown[]) => call(doc, table, `${prefix}.${member}`, pon, args);
        },
      },
    );
  }

  function openHandle(key: string): DocumentHandle {
    const document = spec.documents?.[key];
    if (!document) throw new Error(`[fake-engine] no document "${key}"`);
    const id = document.id ?? key;
    const services = new Set(document.services ?? []);
    const lacks = new Set(document.lacks ?? []);
    const formsAbsent = new Set<string>();
    if (!services.has('forms.submit')) formsAbsent.add('submit');
    if (lacks.has('forms.applyEffects')) formsAbsent.add('applyEffects');
    const pageAnnotationsAbsent = new Set<string>();
    if (lacks.has('page.annotations.downloadFile')) pageAnnotationsAbsent.add('downloadFile');
    const table = document.responses;
    const security = document.security ?? {};
    const listeners = new Set<(event: unknown) => void>();
    handles.set(id, { listeners });
    const pages = (document.pages ?? []) as { ref: { pageObjectNumber: number }; index: number }[];
    const allows = (capability: string) =>
      security.allows === '*' || (security.allows ?? []).includes(capability);

    const handle: Record<string, unknown> = {
      id,
      capabilities: decode(document.capabilities ?? {}),
      security: {
        current: decode(security.current ?? null),
        effectiveScope: security.effectiveScope ?? [],
        identity: decode(security.identity ?? null),
        passwordPrompt: decode(security.passwordPrompt ?? { state: 'none' }),
        allows,
        allowsAnnotationCreate: () => security.allowsAnnotationCreate ?? allows('doc.annotate.create'),
        allowsAnnotationMutation: (action: 'update' | 'delete') =>
          security.allowsAnnotationMutation?.[action] ?? allows('doc.annotate.modify'),
        allowsAnnotationGroupAssignment: () => security.allowsAnnotationGroupAssignment ?? false,
        unlock: (...args: unknown[]) => call(id, table, 'security.unlock', undefined, args),
      },
      metadata: service(id, table, 'metadata'),
      annotations: service(id, table, 'annotations'),
      // `forms.submit` is optional in the contract (a submit-capable home):
      // present only when `services` lists it, so `typeof submit` can be false.
      // `forms.applyEffects` is optional too, but present unless `lacks` names it.
      forms: service(id, table, 'forms', undefined, formsAbsent),
      search: service(id, table, 'search'),
      pages: new Proxy(
        {},
        {
          get: (_target, member) => {
            if (typeof member !== 'string' || member === 'then' || lacks.has(`pages.${member}`)) return undefined;
            return (...args: unknown[]) =>
              call(id, table, `pages.${member}`, undefined, args, member === 'list'
                ? () => ({ pageCount: pages.length, pages: decode(pages) })
                : undefined);
          },
        },
      ),
      events: {
        subscribe: (listener: (event: unknown) => void) => {
          listeners.add(listener);
          return () => void listeners.delete(listener);
        },
        lastServerId: () => null,
      },
      page: (ref: { pageObjectNumber: number }) => {
        const pon = ref.pageObjectNumber;
        const page = pages.find((p) => p.ref.pageObjectNumber === pon);
        if (!page) {
          throw makeError({ name: 'EngineError', message: `no page ${pon}`, code: 'NotFound' });
        }
        const pageHandle: Record<string, unknown> = {
          ref: { kind: 'objectNumber', pageObjectNumber: pon },
          pageIndex: page.index,
          annotations: service(id, table, 'page.annotations', pon, pageAnnotationsAbsent),
          text: service(id, table, 'page.text', pon),
          geometry: service(id, table, 'page.geometry', pon),
          render: service(id, table, 'page.render', pon),
        };
        if (services.has('page.pieceInfo')) pageHandle.pieceInfo = service(id, table, 'page.pieceInfo', pon);
        if (services.has('page.measure')) pageHandle.measure = service(id, table, 'page.measure', pon);
        return pageHandle;
      },
      download: (...args: unknown[]) => call(id, table, 'download', undefined, args),
      close: (...args: unknown[]) => call(id, table, 'close', undefined, args, () => undefined),
    };
    for (const optional of ['render', 'actions', 'attachments', 'pieceInfo', 'redaction', 'signatures', 'fonts']) {
      if (services.has(optional)) handle[optional] = service(id, table, optional);
    }
    if (services.has('version')) {
      handle.version = (...args: unknown[]) => call(id, table, 'version', undefined, args);
    }
    if (services.has('downloadLayer')) {
      handle.downloadLayer = (...args: unknown[]) => call(id, table, 'downloadLayer', undefined, args);
    }
    return handle as unknown as DocumentHandle;
  }

  const engine = {
    open: (input: unknown, options?: unknown) => {
      const args = options === undefined ? [input] : [input, options];
      const inputId = (input as { id?: unknown } | null)?.id;
      return call(
        null,
        spec.responses,
        'open',
        undefined,
        args,
        () => {
          if (typeof inputId === 'string' && spec.documents?.[inputId]) return { $document: inputId };
          if (spec.defaultDocument !== undefined) return { $document: spec.defaultDocument };
          throw makeError({ name: 'FakeEngineUnmatched', message: 'no document for open' });
        },
        (value) => {
          const key = (value as { $document?: unknown } | null)?.$document;
          return typeof key === 'string' ? openHandle(key) : value;
        },
      );
    },
    destroy: () => call(null, spec.responses, 'destroy', undefined, [], () => undefined),
  } as unknown as Engine;

  return {
    engine,
    log,
    unmatched,
    emit(doc, event) {
      const handle = handles.get(doc);
      if (!handle) throw new Error(`[fake-engine] emit to unopened document "${doc}"`);
      const live = decode(event);
      for (const listener of [...handle.listeners]) listener(live);
    },
    settle(label, outcome) {
      const index = pending.findIndex((p) => p.label === label);
      if (index < 0) return false;
      const [entry] = pending.splice(index, 1);
      if (outcome.error) entry.reject(entry.engine ? makeEngineError(outcome.error) : makeError(outcome.error));
      else entry.resolve(decode(outcome.value));
      return true;
    },
    defer(label, resolve, reject) {
      pending.push({ label, resolve, reject });
    },
    snapshot() {
      const out: Record<string, { subscribers: number }> = {};
      for (const [id, handle] of handles) out[id] = { subscribers: handle.listeners.size };
      return out;
    },
  };
}
