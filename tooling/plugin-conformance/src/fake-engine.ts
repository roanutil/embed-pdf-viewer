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
  createTextLayout,
  type PageGeometrySnapshot,
  PermissionDenied,
  passwordPromptFromState,
  checkCapability,
  checkCollab,
  checkFieldAction,
  checkSetGroup,
  collabTargetOf,
  fieldRightsOf,
  type AnnotationOwner,
  type Identity,
  decodePdfBits,
  expandRawScope,
  type DocCapability,
  type DocumentSecurityState,
  type DocumentUnlockResult,
  LOCAL_ENGINE_BRAND,
  type PageRef,
  type PageLayout,
  type DocumentEvent,
  type DocumentHandle,
  type LocalDocumentHandle,
  type LocalPageHandle,
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
  /** Confirmed events published in order before this successful call settles. */
  readonly events?: readonly DocumentEvent[];
  readonly error?: ErrorSpec;
  readonly defer?: string;
}

export interface SecuritySpec {
  readonly allows?: readonly string[] | '*';
  readonly allowsAnnotation?: Partial<
    Record<'create' | 'update' | 'delete' | 'set-group', boolean>
  >;
  readonly allowsField?: Partial<Record<'fill' | 'sign' | 'set-group', boolean>>;
  readonly scope?: readonly string[];
  /** Raw scope fixture: use the shipping resolver against the current security bits.
   * Explicit allows overrides still take precedence. */
  readonly rawScope?: readonly string[];
  readonly identity?: unknown;
  readonly state?: unknown;
  readonly passwordPrompt?: unknown;
}

export interface FakeDocumentSpec {
  /** The handle's id; defaults to the table key. */
  readonly id?: string;
  /** Local extensions exposed individually for targeted handle fixtures, and
   *  the optional shared member `forms.submit`. Shared services always exist. */
  readonly services?: readonly string[];
  /** Members intentionally absent from a malformed handle fixture. This does
   *  not make required members optional in the shipping engine contract. */
  readonly lacks?: readonly string[];
  /** Marks a local document and exposes its local-only services. */
  readonly local?: boolean;
  /** Explicit reserved numbers, consumed by objectNumbers.take(). */
  readonly objectNumbers?: readonly number[];
  readonly security?: SecuritySpec;
  /** The page registry (`PageLayout`s, each with its `ref`). `pages.list` answers
   *  from it unless the table says otherwise. Page service calls refuse a ref
   *  whose object number is not in the current registry. */
  readonly pages?: readonly unknown[];
  readonly namedPages?: readonly unknown[];
  readonly responses?: readonly ResponseSpec[];
}

export interface FakeEngineSpec {
  readonly documents?: Readonly<Record<string, FakeDocumentSpec>>;
  readonly canOpenFiles?: boolean;
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

type TableServices = {
  pages: LocalDocumentHandle['pages'];
  metadata: LocalDocumentHandle['metadata'];
  'metadata.custom': LocalDocumentHandle['metadata']['custom'];
  annotations: LocalDocumentHandle['annotations'];
  forms: LocalDocumentHandle['forms'];
  search: LocalDocumentHandle['search'];
  outline: LocalDocumentHandle['outline'];
  actions: LocalDocumentHandle['actions'];
  attachments: LocalDocumentHandle['attachments'];
  redaction: LocalDocumentHandle['redaction'];
  signatures: LocalDocumentHandle['signatures'];
  'page.annotations': LocalPageHandle['annotations'];
  'page.forms': LocalPageHandle['forms'];
  'page.text': LocalPageHandle['text'];
  'page.render': LocalPageHandle['render'];
  'page.measure': LocalPageHandle['measure'];
  pieceInfo: LocalDocumentHandle['pieceInfo'];
  'page.pieceInfo': LocalPageHandle['pieceInfo'];
  fonts: LocalDocumentHandle['fonts'];
};

/** Only shipping service members exist; table answers cannot revive a removed method. */
const SERVICE_MEMBERS = {
  pages: {
    list: true,
    reorder: true,
    rotate: true,
    delete: true,
    setName: true,
    removeName: true,
    flatten: true,
    extract: true,
    insert: true,
    insertBlank: true,
  },
  metadata: { get: true, update: true, custom: true },
  'metadata.custom': { get: true, update: true },
  annotations: { list: true, export: true, import: true },
  forms: {
    list: true,
    get: true,
    setValue: true,
    reset: true,
    submit: true,
    export: true,
    import: true,
    importValues: true,
    create: true,
    update: true,
    setSignatureAppearance: true,
    delete: true,
    addWidget: true,
    removeWidget: true,
    deleteWidget: true,
    updateWidget: true,
    reorderWidgets: true,
    reorderCalculations: true,
    repair: true,
  },
  search: { query: true },
  outline: { get: true, create: true, update: true, move: true, delete: true },
  actions: { get: true },
  attachments: { list: true, download: true, create: true, delete: true },
  redaction: { apply: true },
  signatures: {
    list: true,
    getContents: true,
    getDigest: true,
    downloadRevision: true,
    analyze: true,
    prepare: true,
    complete: true,
    cancel: true,
  },
  'page.annotations': {
    list: true,
    renderAppearances: true,
    downloadResource: true,
    create: true,
    update: true,
    delete: true,
    reorder: true,
    flatten: true,
    exportAppearance: true,
    renderAppearancesRaw: true,
  },
  'page.forms': { renderAppearances: true, renderAppearancesRaw: true },
  'page.text': { get: true, slice: true, layout: true },
  'page.render': { image: true, raw: true },
  'page.measure': { listViewports: true, setScale: true },
  pieceInfo: { get: true, update: true, list: true, delete: true },
  'page.pieceInfo': { get: true, update: true, list: true, delete: true },
  fonts: { setEmbeddingPolicy: true, setTypographicFeatures: true },
} satisfies { [Path in keyof TableServices]: { [Member in keyof TableServices[Path]]-?: true } };

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
  report: (error: unknown) => void = console.error,
): FakeEngine {
  const log: EngineLogEntry[] = [];
  const unmatched: string[] = [];
  const pending: Pending[] = [];
  const handles = new Map<
    string,
    { listeners: Set<(event: DocumentEvent) => void>; publish(event: DocumentEvent): void }
  >();
  const consumed = new Map<ResponseSpec, number>();

  function match(
    table: readonly ResponseSpec[] | undefined,
    path: string,
    pon: number | undefined,
    args: unknown[],
  ) {
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
    validate?: () => void,
    publish?: (event: DocumentEvent) => void,
  ): AbortablePromise<unknown> {
    const args = rawArgs.map((arg) => canonical(arg));
    const index = log.length;
    log.push(pon === undefined ? { doc, path, args } : { doc, path, pon, args });
    const executor = (resolve: (value: unknown) => void, reject: (error: unknown) => void) => {
      try {
        validate?.();
      } catch (error) {
        reject(error);
        return;
      }
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
        unmatched.push(
          `${doc ?? 'engine'}: ${path}${pon === undefined ? '' : `@${pon}`} ${JSON.stringify(args)}`,
        );
        reject(makeError({ name: 'FakeEngineUnmatched', message: `no response for ${path}` }));
        return;
      }
      const succeed = (value: unknown) => {
        try {
          const result = map(value);
          for (const event of entry.events ?? []) {
            if (!publish)
              throw new Error(
                '[fake-engine] an engine-level response cannot publish document events',
              );
            publish(decode(event) as DocumentEvent);
          }
          resolve(result);
        } catch (error) {
          reject(error);
        }
      };
      if (entry.defer !== undefined) {
        pending.push({ label: entry.defer, engine: true, resolve: succeed, reject });
        return;
      }
      if (entry.error) reject(makeEngineError(entry.error));
      else succeed(decode(entry.value));
    };
    // A plain-promise engine is what the kernel's race exists for: a call
    // nobody can abort, whose late result still has to be released.
    if (spec.plainPromises) return new Promise<unknown>(executor) as AbortablePromise<unknown>;
    return new AbortablePromise<unknown>((resolve, reject, _progress, signal) => {
      signal.addEventListener('abort', () => {
        log.push({
          abort: index,
          reason: canonical((signal.reason as { reason?: unknown })?.reason),
        });
      });
      executor(resolve, reject);
    });
  }

  function service(
    doc: string,
    table: readonly ResponseSpec[] | undefined,
    prefix: keyof TableServices,
    pon?: number,
    absent: ReadonlySet<string> = new Set(),
    validate?: () => void,
    publish?: (event: DocumentEvent) => void,
  ): unknown {
    const custom =
      prefix === 'metadata'
        ? service(doc, table, 'metadata.custom', undefined, undefined, undefined, publish)
        : undefined;
    return new Proxy(
      {},
      {
        get: (_target, member) => {
          if (member === 'custom' && custom) return custom;
          if (
            typeof member !== 'string' ||
            !Object.hasOwn(SERVICE_MEMBERS[prefix], member) ||
            absent.has(member)
          )
            return undefined;
          return (...args: unknown[]) =>
            call(
              doc,
              table,
              `${prefix}.${member}`,
              pon,
              args,
              undefined,
              prefix === 'page.text' && member === 'layout'
                ? (value) => {
                    if (!value || typeof value !== 'object' || !('$textLayout' in value))
                      throw new Error(
                        '[fake-engine] page.text.layout needs a $textLayout geometry fixture',
                      );
                    return createTextLayout(
                      (value as { $textLayout: PageGeometrySnapshot }).$textLayout,
                    );
                  }
                : undefined,
              validate,
              publish,
            );
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
    // Required methods can be removed explicitly to exercise malformed handles.
    for (const member of lacks) if (member.startsWith('forms.')) formsAbsent.add(member.slice(6));
    const pageAnnotationsAbsent = new Set<string>(document.local ? [] : ['renderAppearancesRaw']);
    for (const member of lacks)
      if (member.startsWith('page.annotations.'))
        pageAnnotationsAbsent.add(member.slice('page.annotations.'.length));
    const table = document.responses;
    const security = document.security ?? {};
    const listeners = new Set<(event: DocumentEvent) => void>();
    let pages = decode(document.pages ?? []) as PageLayout[];
    let namedPages = decode(document.namedPages ?? []);
    let lastServerId: number | null = null;
    const publish = (event: DocumentEvent) => {
      if ('layout' in event) {
        pages = event.layout.pages ?? [];
        namedPages = event.layout.namedPages ?? [];
      }
      if ('origin' in event && event.origin.serverId !== null) {
        lastServerId = Math.max(lastServerId ?? 0, event.origin.serverId);
      }
      for (const listener of [...listeners]) {
        try {
          listener(event);
        } catch (error) {
          report(error);
        }
      }
    };
    handles.set(id, { listeners, publish });
    // Every method owns this open's event stream, including calls retained across a reopen.
    const callForHandle: typeof call = (doc, table, path, pon, args, fallback, map, validate) =>
      call(doc, table, path, pon, args, fallback, map, validate, publish);
    const serviceForHandle: typeof service = (doc, table, prefix, pon, absent, validate) =>
      service(doc, table, prefix, pon, absent, validate, publish);
    const numbers = [...(document.objectNumbers ?? [])];
    let nextNumber = Math.max(0, ...pages.map((page) => page.ref.objectNumber), ...numbers) + 1;
    const lostListeners = new Set<Parameters<DocumentHandle['objectNumbers']['onLost']>[0]>();
    let currentSecurityState = decode(
      security.state ?? {
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
      },
    ) as DocumentSecurityState;
    let currentPasswordPrompt = decode(
      security.passwordPrompt ?? passwordPromptFromState(currentSecurityState),
    );
    const identity = decode(security.identity ?? {}) as Identity;
    const currentBits = () => decodePdfBits(currentSecurityState.permissions.bits);
    const allows = (capability: string) =>
      security.allows !== undefined
        ? security.allows === '*' || security.allows.includes(capability)
        : security.rawScope !== undefined &&
          checkCapability(capability as DocCapability, security.rawScope, currentBits());

    const handle: Record<string, unknown> = {
      id,
      security: {
        get state() {
          return currentSecurityState;
        },
        get passwordPrompt() {
          return currentPasswordPrompt;
        },
        get scope() {
          return security.rawScope === undefined
            ? decode(security.scope ?? [])
            : [...expandRawScope(security.rawScope, currentBits())].sort();
        },
        identity: decode(security.identity ?? null),
        allows,
        allowsAnnotation: (
          action: 'create' | 'update' | 'delete' | 'set-group',
          target?: AnnotationOwner | { groupId: string },
        ) => {
          const configured = security.allowsAnnotation?.[action];
          if (configured !== undefined) return configured;
          if (security.rawScope !== undefined) {
            if (action === 'set-group')
              return checkSetGroup(
                'annotations',
                (target as { groupId: string }).groupId,
                identity.groupId,
                security.rawScope,
              );
            if (
              action !== 'create' &&
              (target as AnnotationOwner | undefined)?.subtype === 'widget'
            )
              return allows('doc.forms.modify');
            return checkCollab(
              action,
              action === 'create' ? identity : collabTargetOf(target ?? {}),
              security.rawScope,
              identity,
              currentBits(),
            );
          }
          return action === 'set-group'
            ? false
            : allows(action === 'create' ? 'doc.annotate.create' : 'doc.annotate.modify');
        },
        allowsField: (
          action: 'fill' | 'sign' | 'set-group',
          target?: { groupId: string | null },
        ) => {
          const configured = security.allowsField?.[action];
          if (configured !== undefined) return configured;
          if (security.rawScope !== undefined)
            return action === 'set-group'
              ? checkSetGroup('fields', target?.groupId ?? '', identity.groupId, security.rawScope)
              : checkFieldAction(action, target?.groupId ?? null, security.rawScope, currentBits());
          return action === 'set-group'
            ? false
            : allows(action === 'fill' ? 'doc.forms.fill' : 'doc.sign');
        },
        fieldRights: () =>
          security.rawScope !== undefined
            ? fieldRightsOf(security.rawScope, currentBits(), null, identity.groupId)
            : {
                fill: { all: allows('doc.forms.fill') },
                sign: { all: allows('doc.sign') },
                setGroup: { all: false, groups: [] },
              },
        unlock: (...args: unknown[]) =>
          callForHandle(id, table, 'security.unlock', undefined, args, undefined, (value) => {
            const result = value as DocumentUnlockResult;
            currentSecurityState = result.security;
            currentPasswordPrompt = passwordPromptFromState(currentSecurityState);
            return result;
          }),
      },
      metadata: serviceForHandle(id, table, 'metadata'),
      annotations: serviceForHandle(id, table, 'annotations'),
      // `forms.submit` is optional in the contract (a submit-capable home):
      // present only when `services` lists it, so `typeof submit` can be false.
      forms: serviceForHandle(id, table, 'forms', undefined, formsAbsent),
      search: serviceForHandle(id, table, 'search'),
      outline: serviceForHandle(id, table, 'outline'),
      actions: serviceForHandle(id, table, 'actions'),
      attachments: serviceForHandle(id, table, 'attachments'),
      redaction: serviceForHandle(id, table, 'redaction'),
      signatures: serviceForHandle(id, table, 'signatures'),
      render: {
        getPolicy: () =>
          callForHandle(id, table, 'render.getPolicy', undefined, [], () => ({
            kind: 'continuous',
          })),
      },
      objectNumbers: {
        take: () => numbers.shift() ?? null,
        get held() {
          return numbers.length;
        },
        reserve: (count: number) =>
          callForHandle(
            id,
            table,
            'objectNumbers.reserve',
            undefined,
            [count],
            () => undefined,
            () => {
              while (numbers.length < count) numbers.push(nextNumber++);
            },
            () => {
              if (!Number.isInteger(count) || count < 1)
                throw makeError({
                  name: 'EngineError',
                  code: 'InvalidArg',
                  message: `cannot reserve ${count} object numbers`,
                });
            },
          ),
        onLost: (listener: Parameters<DocumentHandle['objectNumbers']['onLost']>[0]) => {
          lostListeners.add(listener);
          return () => {
            lostListeners.delete(listener);
          };
        },
      },
      apply: (...args: unknown[]) => callForHandle(id, table, 'apply', undefined, args),
      with: () => handle,
      setWorkingSet: () => {},
      pages: new Proxy(
        {},
        {
          get: (_target, member) => {
            if (
              typeof member !== 'string' ||
              !Object.hasOwn(SERVICE_MEMBERS.pages, member) ||
              lacks.has(`pages.${member}`)
            )
              return undefined;
            return (...args: unknown[]) =>
              callForHandle(
                id,
                table,
                `pages.${member}`,
                undefined,
                args,
                member === 'list'
                  ? () => ({
                      pageCount: pages.length,
                      pages: decode(pages),
                      namedPages: decode(namedPages),
                    })
                  : undefined,
              );
          },
        },
      ),
      events: {
        subscribe: (listener: (event: DocumentEvent) => void) => {
          listeners.add(listener);
          return () => void listeners.delete(listener);
        },
        on: (type: DocumentEvent['type'], listener: (event: DocumentEvent) => void) => {
          const filtered = (event: DocumentEvent) => {
            if (event.type === type) listener(event);
          };
          listeners.add(filtered);
          return () => {
            listeners.delete(filtered);
          };
        },
        lastServerId: () => lastServerId,
      },
      page: (ref: PageRef) => {
        const pon = ref.objectNumber;
        const validate = () => {
          if (!pages.some((page) => page.ref.objectNumber === pon)) {
            throw makeError({ name: 'EngineError', message: `no page ${pon}`, code: 'NotFound' });
          }
        };
        const pageHandle: Record<string | symbol, unknown> = { ref: { ...ref } };
        for (const name of ['annotations', 'forms', 'text', 'render', 'measure'] as const) {
          pageHandle[name] = serviceForHandle(
            id,
            table,
            `page.${name}`,
            pon,
            name === 'annotations'
              ? pageAnnotationsAbsent
              : !document.local && (name === 'render' || name === 'forms')
                ? new Set(['raw', 'renderAppearancesRaw'])
                : undefined,
            validate,
          );
        }
        if (document.local || services.has('page.pieceInfo'))
          pageHandle.pieceInfo = serviceForHandle(
            id,
            table,
            'page.pieceInfo',
            pon,
            undefined,
            validate,
          );
        if (document.local) pageHandle[LOCAL_ENGINE_BRAND] = true;
        return pageHandle;
      },
      download: (...args: unknown[]) => callForHandle(id, table, 'download', undefined, args),
      close: (...args: unknown[]) =>
        callForHandle(id, table, 'close', undefined, args, () => undefined),
    };
    for (const optional of ['pieceInfo', 'fonts'] as const) {
      if (document.local || services.has(optional))
        handle[optional] = serviceForHandle(id, table, optional);
    }
    if (document.local || services.has('version')) {
      handle.version = (...args: unknown[]) => callForHandle(id, table, 'version', undefined, args);
    }
    if (document.local || services.has('downloadLayer')) {
      handle.downloadLayer = (...args: unknown[]) =>
        callForHandle(id, table, 'downloadLayer', undefined, args);
    }
    if (document.local) {
      Object.defineProperty(handle, LOCAL_ENGINE_BRAND, { value: true });
      handle.downloadToFile = (...args: unknown[]) =>
        callForHandle(id, table, 'downloadToFile', undefined, args);
    }
    return handle as unknown as DocumentHandle;
  }

  const engine = {
    canOpenFiles: spec.canOpenFiles ?? true,
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
          if (typeof inputId === 'string' && spec.documents?.[inputId])
            return { $document: inputId };
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
      handle.publish(decode(event) as DocumentEvent);
    },
    settle(label, outcome) {
      const index = pending.findIndex((p) => p.label === label);
      if (index < 0) return false;
      const [entry] = pending.splice(index, 1);
      if (outcome.error)
        entry.reject(entry.engine ? makeEngineError(outcome.error) : makeError(outcome.error));
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
