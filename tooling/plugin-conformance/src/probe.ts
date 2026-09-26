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
  type Action,
  type AnyPlugin,
  type ControllerContext,
  type EffectContext,
  type PluginContext,
} from '@embedpdf/core';
import { PermissionDenied } from '@embedpdf/engine-core/runtime';

import { makeError, type ErrorSpec } from './fake-engine';
import { registerPlugin, type PluginEnv } from './scenario';

/** How a probe's `init` or teardown finishes. */
export interface ProbeStep {
  /** Stay pending until a `settle` step names this label. */
  readonly defer?: string;
  readonly throws?: ErrorSpec;
  /** Reject asynchronously instead of throwing. */
  readonly rejects?: ErrorSpec;
}

export interface ProbeConfig {
  readonly id: string;
  readonly scope?: 'workspace' | 'document';
  /** Provide a capability under this token name. */
  readonly token?: string;
  readonly hint?: string;
  readonly requires?: readonly string[];
  /** Hints for required tokens this probe is the first to name. */
  readonly requiresHint?: Readonly<Record<string, string>>;
  readonly optional?: readonly string[];
  readonly initialState?: unknown;
  /** Per action type: `replace` (state becomes `action.state`), `merge`
   *  (`{...state, ...action.state}`) or `same` (return the state object: no
   *  change). Unlisted types return the state object. `null` gives the
   *  plugin no reducer at all. */
  readonly reduce?: Readonly<Record<string, 'replace' | 'merge' | 'same'>> | null;
  readonly init?: ProbeStep | true;
  readonly effects?: {
    readonly throws?: ErrorSpec;
    /** Log every action of these types. */
    readonly onAction?: readonly string[];
    /** Log changes of the kernel's active document id. */
    readonly watchActive?: boolean;
    /** Log changes to this probe's own state, compared by reference. */
    readonly watchState?: boolean;
    /** The first time the state watch fires, dispatch this action from
     *  inside the handler: a listener re-entering the store mid-pass. */
    readonly watchDispatchOnce?: Action & { readonly state?: unknown };
    /** Make the watch and action handlers throw after logging. */
    readonly listenerThrows?: ErrorSpec;
    /** Register a teardown that logs, and finishes as described. */
    readonly cleanup?: ProbeStep | true;
    /** Dispatch this action when the effect is set up. */
    readonly dispatch?: Action & { readonly state?: unknown };
  };
  /** Throw while building the capability. */
  readonly capabilityThrows?: ErrorSpec;
  /** Hand back the plugin object an earlier probe in this case built under
   *  this id, instead of a new one: the same definition installed twice. */
  readonly sameAs?: string;
  /** Build the capability through the v3 `create` hook instead of
   *  `capability`: the api is `createApi` below, over a ControllerContext. */
  readonly create?: {
    readonly throws?: ErrorSpec;
    /** Provide a `connect` half. It logs `<id>.connect` and then, as asked,
     *  throws, dispatches, or listens: `listenDoc` to the guarded handle's
     *  event stream (an object source), `listenActive` to the documents
     *  capability's `onActiveChanged` (a function source). */
    readonly connect?:
      | true
      | {
          readonly throws?: ErrorSpec;
          readonly dispatch?: Action & { readonly state?: unknown };
          readonly listenDoc?: boolean;
          readonly listenActive?: boolean;
        };
  };
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
  const special = spec as { class?: string; required: string; context?: string; what: string; code: string; message: string };
  if (special.class === 'PermissionDenied') return new PermissionDenied(special.required, special.context);
  if (special.class === 'CancelledError') return new CancelledError(special.what);
  if (special.class === 'PluginError') return new PluginError(special.code as never, 'probe', special.message);
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
  const { log, defer } = env;
  const probeToken = (tokenName: string, hint?: string) => env.token(tokenName, hint);
  const name = config.id;
  const where = (ctx: PluginContext<unknown>) => ctx.documentId ?? null;
  const token = config.token === undefined ? undefined : probeToken(config.token, config.hint);

  const plugin: AnyPlugin = {
    id: config.id,
    scope: config.scope,
    token,
    requires: config.requires?.map((t) => probeToken(t, config.requiresHint?.[t])),
    optional: config.optional?.map((t) => probeToken(t)),
    initialState: config.initialState === undefined ? undefined : () => structuredClone(config.initialState),
    reduce:
      config.reduce === null
        ? undefined
        : (state: unknown, action: Action & { state?: unknown }) => {
            const rule = config.reduce?.[action.type];
            if (rule === 'replace') return action.state;
            if (rule === 'merge') return { ...(state as object), ...(action.state as object) };
            return state;
          },
    create: config.create ? (ctx: ControllerContext<unknown>) => createApi(config, env, ctx) : undefined,
    capability: token && !config.create
      ? (ctx: PluginContext<unknown>) => {
          log(`${name}.capability`, [where(ctx)]);
          if (config.capabilityThrows) throw makeError(config.capabilityThrows);
          return {
            dispatch: (action: Action) => ctx.dispatch(action),
            state: () => ctx.getState(),
            document: () => ctx.document(),
            core: () => ctx.core(),
            documentId: () => ctx.documentId ?? null,
            handleId: () => ctx.doc?.id ?? null,
            handleFor: (documentId?: string) => ctx.documentHandle(documentId)?.id ?? null,
            get: (tokenName: string) => ctx.get(probeToken(tokenName)) !== undefined,
            tryGet: (tokenName: string) => ctx.tryGet(probeToken(tokenName)) !== null,
            forDocument: (tokenName: string, documentId: string) =>
              ctx.forDocument(probeToken(tokenName), documentId) !== undefined,
            tryForDocument: (tokenName: string, documentId: string) =>
              ctx.tryForDocument(probeToken(tokenName), documentId) !== null,
            cleanup: (label: string) => ctx.cleanup(() => log(`${name}.cleanup`, [label])),
          };
        }
      : undefined,
    init: config.init
      ? (ctx: PluginContext<unknown>) => {
          log(`${name}.init`, [where(ctx)]);
          return finish(config.init, defer);
        }
      : undefined,
    effects: config.effects
      ? (ctx: EffectContext<unknown>) => {
          const effects = config.effects!;
          log(`${name}.effects`, [where(ctx)]);
          if (effects.throws) throw makeError(effects.throws);
          const listened = (what: string, args: unknown[]) => {
            log(`${name}.${what}`, args);
            if (effects.listenerThrows) throw makeError(effects.listenerThrows);
          };
          for (const type of effects.onAction ?? []) {
            ctx.onAction(type, (action) => listened('onAction', [where(ctx), action]));
          }
          if (effects.watchActive) {
            ctx.watch(
              () => ctx.core().activeId,
              (next, previous) => listened('watchActive', [where(ctx), next, previous]),
            );
          }
          if (effects.watchState) {
            let relay = effects.watchDispatchOnce;
            ctx.watch(
              () => ctx.getState(),
              (next, previous) => {
                if (relay) {
                  const action = relay;
                  relay = undefined;
                  ctx.dispatch(action);
                }
                listened('watchState', [where(ctx), next, previous]);
              },
            );
          }
          if (effects.cleanup) {
            const step = effects.cleanup;
            ctx.cleanup(() => {
              log(`${name}.teardown`, [where(ctx)]);
              return finish(step, defer);
            });
          }
          if (effects.dispatch) ctx.dispatch(effects.dispatch);
        }
      : undefined,
  };
  return plugin;
}

/**
 * The api of a `create` probe. Every member reaches one ControllerContext
 * member and reports what it did; promise-returning members hand the promise
 * back, so its settlement lands in the trace. `<label>` arguments name a
 * `settle` step that finishes the work the member started.
 */
function createApi(config: ProbeConfig, env: PluginEnv, ctx: ControllerContext<unknown>) {
  const { log, defer } = env;
  const name = config.id;
  const create = config.create!;
  log(`${name}.create`, [ctx.documentId ?? null, ctx.instanceId]);
  if (create.throws) throw makeError(create.throws);
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
    state: () => ctx.getState(),
    dispatch: (action: Action) => ctx.dispatch(action),
    /** `ctx.doc.<path>(...args)`: through the guarded handle. */
    docCall: (path: string, ...args: unknown[]) => {
      const { target, method } = service(path);
      return (target[method] as (...a: unknown[]) => unknown)(...args);
    },
    /** `ctx.doc.page(toPageRef(pon)).<path>(...args)`. */
    pageCall: (pon: number, path: string, ...args: unknown[]) => {
      const page = ctx.doc.page(toPageRef(pon)) as unknown as Record<string, Record<string, (...a: unknown[]) => unknown>>;
      const [serviceName, method] = path.split('.');
      return page[serviceName][method](...args);
    },
    /** A synchronous read through the guard: `ctx.doc.security.allows(cap)`. */
    allows: (capability: string) => ctx.doc.security.allows(capability as never),
    docId: () => ctx.doc.id,
    /** A synchronous member read through the guard, by path. */
    docRead: (path: string) => {
      let value: unknown = ctx.doc;
      for (const member of path.split('.')) value = (value as Record<string, unknown>)[member];
      return value;
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
      ctx.waitFor(() => (ctx.getState() as Record<string, unknown>)[key] === value, { signal: signalOf(signal) }),
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
    forPage: (pon: number) => describeSpace(ctx.geometry.forPage(toPageRef(pon))),
    tryForPage: (pon: number) => {
      const space = ctx.geometry.tryForPage(toPageRef(pon));
      return space ? describeSpace(space) : null;
    },
    assertPageRef: (pon: number) => ctx.assertPageRef(toPageRef(pon)),
    getPage: (pon: number) => ctx.getPage(toPageRef(pon)),
    get: (tokenName: string) => ctx.get(token(tokenName)) !== undefined,
    tryGet: (tokenName: string) => ctx.tryGet(token(tokenName)) !== null,
    forDocument: (tokenName: string, documentId: string) => ctx.forDocument(token(tokenName), documentId) !== undefined,
    tryForDocument: (tokenName: string, documentId: string) =>
      ctx.tryForDocument(token(tokenName), documentId) !== null,
    cleanup: (label: string) => ctx.cleanup(() => log(`${name}.cleanup`, [label])),
  };

  const connect = create.connect;
  return {
    api,
    connect: connect
      ? () => {
          log(`${name}.connect`, [ctx.documentId ?? null]);
          if (connect === true) return;
          if (connect.throws) throw makeError(connect.throws);
          if (connect.dispatch) ctx.dispatch(connect.dispatch);
          if (connect.listenDoc) {
            ctx.listen(ctx.doc.events, (event) => log(`${name}.docEvent`, [(event as { type: string }).type]));
          }
          if (connect.listenActive) {
            ctx.listen(ctx.get(DocumentsToken).onActiveChanged, (event) => log(`${name}.activeChanged`, [event]));
          }
        }
      : undefined,
  };
}

/** A PageSpace as data: its box and one sample of each conversion. */
function describeSpace(space: ReturnType<ControllerContext<unknown>['geometry']['forPage']>) {
  const quad = { p1: { x: 1, y: 2 }, p2: { x: 3, y: 2 }, p3: { x: 1, y: 4 }, p4: { x: 3, y: 4 } };
  return {
    crop: space.crop,
    width: space.width,
    height: space.height,
    pdfToPage: space.pdfToPage({ x: 10, y: 20 }),
    pageToPdf: space.pageToPdf({ x: 10, y: 20 }),
    pdfRectToPage: space.pdfRectToPage({ left: 10, bottom: 20, right: 30, top: 60 }),
    pageRectToPdf: space.pageRectToPdf({ x: 10, y: 20, width: 20, height: 40 }),
    pdfQuadToPage: space.pdfQuadToPage(quad),
    pageQuadToPdf: space.pageQuadToPdf(quad),
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
