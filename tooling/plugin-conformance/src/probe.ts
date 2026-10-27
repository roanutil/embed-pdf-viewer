/**
 * The probe: a plugin whose every member is configured by the vector, so the
 * kernel can be exercised without any real plugin's behaviour mixed in.
 *
 * `crates/plugin-conformance/src/probe.rs` is the Rust twin, and `README.md`
 * states the config. Every observable thing a probe does is logged as a
 * callback, named `<id>.<what>`, so the trace shows the order the kernel ran
 * plugin code in.
 */
import {
  CancelledError,
  DocumentsToken,
  PluginError,
  toPageRef,
  type AnyPlugin,
  type PluginContext,
} from '@embedpdf/core';
import { PermissionDenied, type PageRef } from '@embedpdf/engine-core/runtime';

import { makeError, type ErrorSpec } from './fake-engine';
import { registerPlugin, type PluginEnv } from './scenario';

/** How a probe's teardown finishes. */
export interface ProbeStep {
  /** Stay pending until a `settle` step names this label. */
  readonly defer?: string;
  readonly throws?: ErrorSpec;
  /** Reject asynchronously instead of throwing. */
  readonly rejects?: ErrorSpec;
}

export interface ProbeConnect {
  readonly throws?: ErrorSpec;
  readonly watchActive?: boolean;
  readonly watchState?: boolean;
  readonly watchMergeOnce?: Readonly<Record<string, unknown>>;
  readonly listenerThrows?: ErrorSpec;
  readonly cleanup?: ProbeStep | true;
  readonly replaceState?: unknown;
  readonly listenDoc?: boolean;
  readonly listenActive?: boolean;
  readonly readPage?: number;
  readonly tryGet?: string;
}

export interface ProbeConfig {
  readonly id: string;
  readonly scope?: 'workspace' | 'document';
  readonly token?: string;
  readonly hint?: string;
  readonly requires?: readonly string[];
  readonly requiresHint?: Readonly<Record<string, string>>;
  readonly optional?: readonly string[];
  readonly state?: unknown;
  readonly createThrows?: ErrorSpec;
  readonly connect?: ProbeConnect | true;
  readonly sameAs?: string;
}

/** An error the probe raises itself, for the classes a table cannot make:
 *  `PermissionDenied` (matched by class, not name), the kernel's
 *  `CancelledError`, and a `PluginError` that must pass through unchanged. */
export type ProbeErrorSpec =
  | ErrorSpec
  | { readonly class: 'PermissionDenied'; readonly required: string; readonly context?: string }
  | { readonly class: 'CancelledError'; readonly what: string }
  | { readonly class: 'PluginError'; readonly code: string; readonly message: string };

function probeError(spec: ProbeErrorSpec): Error {
  const special = spec as {
    class?: string;
    required: string;
    context?: string;
    what: string;
    code: string;
    message: string;
  };
  if (special.class === 'PermissionDenied')
    return new PermissionDenied(special.required, special.context);
  if (special.class === 'CancelledError') return new CancelledError(special.what);
  if (special.class === 'PluginError')
    return new PluginError(special.code as never, 'probe', special.message);
  return makeError(spec as ErrorSpec);
}

type Defer = PluginEnv['defer'];

function finish(step: ProbeStep | true | undefined, defer: Defer): void | Promise<void> {
  if (step === undefined || step === true) return;
  if (step.throws) throw makeError(step.throws);
  if (step.rejects) return Promise.reject(makeError(step.rejects));
  if (step.defer !== undefined) {
    const label = step.defer;
    return new Promise<void>((resolve, reject) => defer(label, () => resolve(), reject));
  }
}

export function probePlugin(config: ProbeConfig, env: PluginEnv): AnyPlugin {
  return {
    id: config.id,
    scope: config.scope,
    token: config.token === undefined ? undefined : env.token(config.token, config.hint),
    requires: config.requires?.map((t) => env.token(t, config.requiresHint?.[t])),
    optional: config.optional?.map((t) => env.token(t)),
    state: config.state === undefined ? undefined : () => structuredClone(config.state),
    create: (ctx: PluginContext<unknown>) => createApi(config, env, ctx),
  };
}

const replaceState = (_state: unknown, value: unknown) => value;
const mergeState = (state: unknown, patch: Readonly<Record<string, unknown>>) => ({
  ...(state as object),
  ...patch,
});
const sameState = (state: unknown) => state;

/**
 * The api of a `create` probe. Every member reaches one PluginContext
 * member and reports what it did; promise-returning members hand the promise
 * back, so its settlement lands in the trace. `<label>` arguments name a
 * `settle` step that finishes the work the member started.
 */
function createApi(config: ProbeConfig, env: PluginEnv, ctx: PluginContext<unknown>) {
  const { log, defer } = env;
  const name = config.id;
  log(`${name}.create`, [ctx.documentId ?? null, ctx.instanceId]);
  if (config.createThrows) throw makeError(config.createThrows);
  const deferred = (label: string) =>
    new Promise<unknown>((resolve, reject) => defer(label, resolve, reject));
  const controllers = new Map<string, AbortController>();
  const signalOf = (label?: string) => {
    if (label === undefined) return undefined;
    let controller = controllers.get(label);
    if (!controller) {
      controller = new AbortController();
      controllers.set(label, controller);
    }
    return controller.signal;
  };
  const source = ctx.events.source<unknown>();
  const token = (tokenName: string) => env.token(tokenName);
  const service = (path: string) => {
    const members = path.split('.');
    const method = members.pop()!;
    let target: Record<string, unknown> = ctx.doc as unknown as Record<string, unknown>;
    for (const member of members) target = target[member] as Record<string, unknown>;
    return { target, method };
  };

  const api = {
    instanceId: () => ctx.instanceId,
    state: () => ctx.state.get(),
    replaceState: (value: unknown) => ctx.state.update(replaceState, value),
    mergeState: (patch: Readonly<Record<string, unknown>>) => ctx.state.update(mergeState, patch),
    sameState: () => ctx.state.update(sameState),
    /** `ctx.doc.<path>(...args)`: through the guarded handle. */
    docCall: (path: string, ...args: unknown[]) => {
      const { target, method } = service(path);
      return (target[method] as (...a: unknown[]) => unknown)(...args);
    },
    /** `ctx.doc.page(toPageRef(pon)).<path>(...args)`. */
    pageCall: (pon: number, path: string, ...args: unknown[]) => {
      const page = ctx.doc.page(toPageRef(pon)) as unknown as Record<
        string,
        Record<string, (...a: unknown[]) => unknown>
      >;
      const [serviceName, method] = path.split('.');
      return page[serviceName][method](...args);
    },
    /** A synchronous read through the guard: `ctx.doc.security.allows(cap)`. */
    allows: (capability: string) => ctx.doc.security.allows(capability as never),
    docId: () => ctx.doc.id,
    /** A synchronous member read through the guard, by path. A method at the
     *  end of the path is called with the remaining arguments. */
    docRead: (path: string, ...args: unknown[]) => {
      let value: unknown = ctx.doc;
      for (const member of path.split('.')) value = (value as Record<string, unknown>)[member];
      return typeof value === 'function' ? (value as (...a: unknown[]) => unknown)(...args) : value;
    },
    /** Subscribe a logging listener to this instance's event source,
     *  optionally tied to a named AbortController. */
    on: (label: string, signal?: string) =>
      source.on((event) => log(`${name}.event`, [label, event]), { signal: signalOf(signal) }),
    emit: (event: unknown) => source.emit(event),
    /** Subscribe a listener that throws, to show the source reports it. */
    onThrowing: (message: string) =>
      source.on(() => {
        throw new Error(message);
      }),
    abortSignal: (label: string, reason?: unknown) => {
      signalOf(label);
      controllers.get(label)!.abort(reason);
    },
    /** Resolve once `state[key] === value`. */
    waitFor: (key: string, value: unknown, signal?: string) =>
      ctx.waitFor(() => (ctx.state.get() as Record<string, unknown>)[key] === value, {
        signal: signalOf(signal),
      }),
    /** A predicate that throws once it is checked. */
    waitForThrowing: (message: string) =>
      ctx.waitFor(() => {
        throw new Error(message);
      }),
    /** Queue work on `serialQueue(key)`: logs when it starts, finishes with the label's settlement. */
    queue: (key: string, label: string) =>
      ctx.serialQueue(key)(() => {
        log(`${name}.queued`, [key, label]);
        return deferred(label);
      }),
    /** Run on `latest(key)`: logs the run id when it starts, waits for the
     *  label, then tries to commit (logged with whether it could). */
    latest: (key: string, label: string, signal?: string, throws?: ProbeErrorSpec) =>
      ctx.latest(key).run(
        async (run) => {
          log(`${name}.run`, [run.id, run.signal.aborted]);
          const value = await deferred(label);
          if (throws) throw probeError(throws);
          const committed = run.commit(() => log(`${name}.commit`, [run.id, value]));
          log(`${name}.ran`, [run.id, committed, run.superseded]);
          return value;
        },
        { signal: signalOf(signal) },
      ),
    latestCancel: (key: string, reason?: unknown) => ctx.latest(key).cancel(reason),
    latestRunning: (key: string) => ctx.latest(key).running,
    /** `acquire`: the resource is the label's value; its disposal is logged. */
    acquire: (label: string) =>
      ctx.acquire(
        (lifetime) => {
          log(`${name}.acquiring`, [lifetime.aborted]);
          return deferred(label);
        },
        (resource) => log(`${name}.dispose`, [resource]),
      ),
    assertPageRef: (pon: number) => ctx.assertPageRef(toPageRef(pon)),
    getPage: (pon: number) => ctx.getPage(toPageRef(pon)),
    get: (tokenName: string) => ctx.get(token(tokenName)) !== undefined,
    tryGet: (tokenName: string) => ctx.tryGet(token(tokenName)) !== null,
    forDocument: (tokenName: string, documentId: string) =>
      ctx.forDocument(token(tokenName), documentId) !== undefined,
    tryForDocument: (tokenName: string, documentId: string) =>
      ctx.tryForDocument(token(tokenName), documentId) !== null,
    cleanup: (label: string) => ctx.cleanup(() => log(`${name}.cleanup`, [label])),
    /** The named controller's `signal.reason`: what `abortSignal` aborted
     *  it with, the platform's AbortError when no reason was given. */
    signalReason: (label: string) => controllers.get(label)?.signal.reason,
    document: () => ctx.document(),
    documentId: () => ctx.documentId ?? null,
    handleId: () => ctx.doc.id,
    handleFor: (documentId?: string) => ctx.documentHandle(documentId)?.id ?? null,
    pageOf: (page: number | PageRef) => ctx.pageOf(page),
    notify: () => ctx.notify(),
  };

  const connect = config.connect;
  return {
    api,
    connect: connect
      ? () => {
          log(`${name}.connect`, [ctx.documentId ?? null]);
          if (connect === true) return;
          if (connect.throws) throw makeError(connect.throws);
          const listened = (what: string, args: unknown[]) => {
            log(`${name}.${what}`, args);
            if (connect.listenerThrows) throw makeError(connect.listenerThrows);
          };
          if (connect.watchState) {
            let relay = connect.watchMergeOnce;
            ctx.listen(ctx.state.onChange, ({ next, previous }) => {
              if (relay) {
                const patch = relay;
                relay = undefined;
                ctx.state.update(mergeState, patch);
              }
              listened('watchState', [ctx.documentId ?? null, next, previous]);
            });
          }
          if (connect.watchActive) {
            ctx.watch(
              () => ctx.get(DocumentsToken).getActiveId(),
              (next, previous) => listened('watchActive', [ctx.documentId ?? null, next, previous]),
            );
          }
          if (connect.cleanup) {
            ctx.cleanup(() => {
              log(`${name}.teardown`, [ctx.documentId ?? null]);
              return finish(connect.cleanup, defer);
            });
          }
          if ('replaceState' in connect) ctx.state.update(replaceState, connect.replaceState);
          if (connect.listenDoc)
            ctx.listen(ctx.doc.events, (event) => log(`${name}.docEvent`, [event.type]));
          if (connect.readPage !== undefined) {
            const page = ctx.getPage(toPageRef(connect.readPage));
            log(`${name}.page`, [page?.index ?? null, page?.size.width ?? null]);
          }
          if (connect.tryGet !== undefined) {
            log(`${name}.tryGet`, [connect.tryGet, ctx.tryGet(token(connect.tryGet)) !== null]);
          }
          if (connect.listenActive) {
            ctx.listen(ctx.get(DocumentsToken).onActiveChanged, (event) =>
              log(`${name}.activeChanged`, [event]),
            );
          }
        }
      : undefined,
  };
}

/** The probes built in each case, by id, for `sameAs`. */
const built = new WeakMap<PluginEnv, Map<string, AnyPlugin>>();

registerPlugin('probe', {
  factory: (config, env) => {
    const probe = config as ProbeConfig;
    let byId = built.get(env);
    if (!byId) {
      byId = new Map();
      built.set(env, byId);
    }
    if (probe.sameAs !== undefined) {
      const earlier = byId.get(probe.sameAs);
      if (!earlier) throw new Error(`[probe] no earlier probe "${probe.sameAs}"`);
      return earlier;
    }
    const plugin = probePlugin(probe, env);
    byId.set(probe.id, plugin);
    return plugin;
  },
});
