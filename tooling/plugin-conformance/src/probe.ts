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
  type Action,
  type AnyPlugin,
  type EffectContext,
  type PluginContext,
} from '@embedpdf/core';

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
    capability: token
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

registerPlugin('probe', { factory: (config, env) => probePlugin(config as ProbeConfig, env) });
