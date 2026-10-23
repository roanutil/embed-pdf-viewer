/**
 * `packages/plugin/actions`: the action engine. Replayed by
 * `crates/plugins/actions/tests/conformance.rs`.
 *
 * The capability is driven through its public and host lenses. Executors,
 * sinks, adapters, handlers, resolvers and listeners are `$fn` callbacks, so
 * every crossing of a registration seam is in the trace. JavaScript runs on
 * the scripted sandbox from `acrojs.ts`: the vectors pin what the plugin asks
 * the sandbox and what it does with each answer, not what a VM computes.
 *
 * Two host inputs replace ambient reads. The TypeScript logs observer
 * failures to `globalThis.console.error`; this module routes `[actions]`
 * messages into the trace as a `console.error` callback, and the Rust port
 * takes the same sink as `ActionsPluginConfig::observer_error`. The UTC offset
 * the TypeScript reads from `Date#getTimezoneOffset` is zero here (the
 * generator pins TZ=UTC), and the Rust port takes it as an input whose
 * default is zero.
 *
 * The actions registration preserves its link-consumer configuration.
 * Active actions cases use the current fixture: enabled scripting with no
 * factory override exposes the default realm. Default VM execution needs
 * a loader outside the microtask-only scenario runner.
 */
import { definePlugin, type AnyPlugin, type DocumentMeta } from '@embedpdf/core';
import type { Annotation, AnnotationRef, DocumentActionsSnapshot, DocumentEvent, FormSubmissionEntry } from '@embedpdf/engine-core/runtime';
import { square as scriptSquare, stamp as scriptStamp, link as scriptLink, widget as scriptWidget } from './annotation-current-fixtures';
import { combobox, placed }  from './form-current-fixtures';
import type { FormEffect } from '@embedpdf/engine-core/runtime';
import type { ScriptSurfaceResult, ScriptCommitSurface, ActionsHostCapability } from '../../../../packages/plugin/actions/src/host-contract';
import type { FormFieldRef, Identity } from '@embedpdf/engine-core/runtime';
import type { ScriptOutput, ScriptAnnotEffect, ScriptWorldInput } from '../../../../packages/core/acrojs/src/index';

import { actionsPlugin } from '../../../../packages/plugin/actions/src/actions.plugin';
import { createHoverPump, type HoverTarget } from '../../../../packages/plugin/actions/src/hover-pump';
import { ActionsToken, type DetachedScriptRealm } from '../../../../packages/plugin/actions/src/host-contract';
import { submitEntriesToUrlEncoded } from '../../../../packages/plugin/actions/src/submit-encoding';
import {
  eventOf,
  triggerOriginOf,
  type ActionsConfig as ActionsPluginConfig,
  type ActionTrigger,
  type ActionTriggerResult,
  type ActionSource,
  type ActionContext,
  type PdfAnnotationEventKind,
  type DocumentTriggerEvent,
} from '../../../../packages/plugin/actions/src/contract';
import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import { onCaseEnd, registerPlugin, type PluginEnv, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';
import * as current from './actions-current-fixtures';
import { updatedFact } from './annotation-live-fixtures';
import type { PageStateReport } from '../../../../packages/plugin/actions/src/host-contract';
import { scriptedCallback, scriptedSandboxFactory, type AnswerSpec, type SandboxSpec } from './acrojs';

/** Why some cases record no state; see `ScenarioCase` in `../scenario`. */
const UNSNAPSHOTTED_ACTIONS_DISPATCHER = {
  snapshot: 'none',
  reason: "Pins what the dispatcher answers: read trees, refusals, and which installed sink or resolver is current, through call outcomes, adapter callbacks and engine calls. Its dispatched actions are compared; the state they build is not.",
} as const;
const UNSNAPSHOTTED_ACTIONS_HOVER = {
  snapshot: 'none',
  reason: "The hover pump keeps its target in a closure. These cases compare submitted transitions, callback order and the actual actions dispatcher outcomes.",
} as const;
const UNSNAPSHOTTED_ACTIONS_SCRIPT = {
  snapshot: 'none',
  reason: "Pins what reaches the script sandbox and the UI adapter (inputs, budgets, realm rebuilds, surfaced requests), through the engine log and the adapter callbacks. Its dispatched actions are compared; the state they build is not.",
} as const;
const UNSNAPSHOTTED_ACTIONS_SUBMIT = {
  snapshot: 'none',
  reason: "Pins the handoff between an installed submit handler and the home, through the handler callbacks and the engine calls. Its dispatched actions are compared; the state they build is not.",
} as const;
const UNSNAPSHOTTED_ACTIONS_TRIGGERS = {
  snapshot: 'none',
  reason: "Pins when lifecycle trees are read and which trigger families fire, through the engine reads and the dispatch outcomes. Its dispatched actions are compared; the state they build is not.",
} as const;

// ── the observer-error host input ─────────────────────────────────────────────

let observerEnv: PluginEnv | null = null;
onCaseEnd(() => {
  observerEnv = null;
});
{
  const original = console.error.bind(console);
  console.error = (...args: unknown[]) => {
    if (observerEnv && typeof args[0] === 'string' && args[0].startsWith('[actions]')) {
      observerEnv.log('console.error', args);
      return;
    }
    original(...args);
  };
}

// ── the factories ─────────────────────────────────────────────────────────────

type Scripted = AnswerSpec & { readonly $fn: string };

interface HarnessJavaScript {
  readonly enabled: boolean;
  /** The scripted sandbox; `null` is a host with no sandbox at all. */
  readonly sandbox?: SandboxSpec | null;
  readonly identity?: Record<string, unknown> | Scripted;
  readonly fileName?: Scripted;
  readonly now?: Scripted;
  readonly utcOffsetMinutes?: Scripted;
  readonly randomSeed?: Scripted;
  readonly budget?: NonNullable<ActionsPluginConfig['javascript']>['budget'];
  readonly maxScriptNodesPerDispatch?: number;
}

type HarnessConfig = Omit<ActionsPluginConfig, 'javascript'> & { readonly javascript?: HarnessJavaScript };

function actionsConfig(config: HarnessConfig | undefined, env: PluginEnv): ActionsPluginConfig | undefined {
  observerEnv = env;
  if (config === undefined) return undefined;
  const { javascript, ...rest } = config;
  if (!javascript) return rest;
  const fn = <T>(spec: Scripted | undefined) =>
    spec === undefined ? undefined : (scriptedCallback(spec.$fn, spec, env) as () => T);
  if (javascript.enabled && javascript.sandbox === undefined) {
    throw new Error('[actions suite] javascript.enabled needs a scripted sandbox: the QuickJS default does not port');
  }
  const identity = javascript.identity;
  return {
    ...rest,
    javascript: {
      enabled: javascript.enabled && javascript.sandbox !== null,
      ...(javascript.sandbox ? { sandboxFactory: scriptedSandboxFactory(javascript.sandbox, env) } : {}),
      ...(identity === undefined
        ? {}
        : { identity: '$fn' in identity ? fn<Record<string, string>>(identity as Scripted) : identity }),
      ...(javascript.fileName ? { fileName: fn<string>(javascript.fileName) } : {}),
      ...(javascript.now ? { now: fn<number>(javascript.now) } : {}),
      ...(javascript.utcOffsetMinutes ? { utcOffsetMinutes: fn<number>(javascript.utcOffsetMinutes) } : {}),
      ...(javascript.randomSeed ? { randomSeed: fn<number>(javascript.randomSeed) } : {}),
      ...(javascript.budget ? { budget: javascript.budget } : {}),
      ...(javascript.maxScriptNodesPerDispatch !== undefined
        ? { maxScriptNodesPerDispatch: javascript.maxScriptNodesPerDispatch }
        : {}),
    } as ActionsPluginConfig['javascript'],
  };
}

registerPlugin('actions', {
  factory: (config, env) => actionsPlugin(actionsConfig(config as HarnessConfig | undefined, env)),
  tokens: [ActionsToken],
});

/** `createHoverPump` behind a document-scoped capability. `dispatch` is the
 *  actions capability's own, or a scripted callback. */
interface HoverPumpConfig {
  readonly dispatch: 'actions' | Scripted;
}

function hoverPumpDriver(config: HoverPumpConfig, env: PluginEnv): AnyPlugin {
  const token = env.token('hover-pump');
  return definePlugin({
    id: 'hover-pump',
    token,
    scope: 'document',
    ...(config.dispatch === 'actions' ? { requires: [ActionsToken] } : {}),
    create: (ctx) => {
      const dispatch =
        config.dispatch === 'actions'
          ? (trigger: ActionTrigger) => ctx.get(ActionsToken).dispatch(trigger)
          : (scriptedCallback(config.dispatch.$fn, config.dispatch, env) as (
              trigger: ActionTrigger,
            ) => Promise<ActionTriggerResult>);
      const pump = createHoverPump(dispatch);
      ctx.cleanup(() => pump.reset());
      return {
        api: {
          hover: (target: HoverTarget | null) => pump.hover(target),
          reset: () => pump.reset(),
        },
      };
    },
  });
}

registerPlugin('hover-pump', {
  factory: (config, env) => hoverPumpDriver(config as HoverPumpConfig, env),
});

/** A sibling that mints a DETACHED realm for its own document from the
 *  actions plugin of `owner` (default `a`), as the stamp plugin does for an
 *  asset. `meta` replaces the realm's document metadata (`null` is none);
 *  `bootSources` is a scripted callback (default: no programs). `run`
 *  boots and runs one program in a single transaction over an empty world. */
interface DetachedRealmConfig {
  readonly owner?: string;
  readonly meta?: DocumentMeta | null;
  readonly bootSources?: Scripted;
}

const EMPTY_WORLD: ScriptWorldInput = { fields: [], event: { kind: 'widget-activate' } };

registerPlugin('detached-realm', {
  factory: (config, env) => {
    const { owner = 'a', meta, bootSources } = (config ?? {}) as DetachedRealmConfig;
    return definePlugin({
      id: 'detached-realm',
      token: env.token('detached-realm'),
      scope: 'document',
      optional: [ActionsToken],
      create: (ctx) => {
        let realm: DetachedScriptRealm | null = null;
        const documentRealm = ctx.tryGet(ActionsToken)?.scriptRealm;
        ctx.cleanup(() => realm?.dispose());
        return {
          api: {
            transaction: () => documentRealm?.transaction(async () => 'body-result'),
            transactionThrows: () =>
              documentRealm?.transaction(async () => {
                throw new Error('body threw');
              }),
            detachedTransaction: () => realm?.transaction(async () => 'body-result'),
            dispose: () => realm?.dispose(),
            mint: () => {
              const mintRealm = ctx.tryForDocument(ActionsToken, owner)?.createDetachedScriptRealm;
              if (!mintRealm) return null;
              realm?.dispose();
              realm = mintRealm({
                doc: ctx.doc!,
                document: () => (meta === undefined ? ctx.document() : meta),
                bootSources: bootSources
                  ? (scriptedCallback(bootSources.$fn, bootSources, env) as () => Promise<string[]>)
                  : async () => [],
              });
              return realm;
            },
            run: (program: string) =>
              realm!.transaction(async (txn) => ({
                boot: await txn.boot(EMPTY_WORLD),
                run: await txn.run(program, EMPTY_WORLD),
              })),
          },
        };
      },
    });
  },
});

// ── builders ──────────────────────────────────────────────────────────────────

const box = { left: 0, bottom: 0, right: 600, top: 800 };
const pr = (pon: number) => ({ kind: 'objectNumber', pageObjectNumber: pon });
const page = (pon: number, index: number, actions?: unknown) => ({
  index,
  ref: pr(pon),
  label: null,
  size: { width: 600, height: 800 },
  rotation: 0,
  userUnit: 1,
  boxes: { media: box, crop: box },
  ...(actions === undefined ? {} : { actions }),
});
const FORMS_EMPTY: ResponseSpec = { path: 'forms.list', value: { formKind: 'none', needsAppearances: false, fields: [], calculationOrder: [] } };
const ANNOTS_EMPTY: ResponseSpec = { path: 'page.annotations.list', value: { annotations: [] } };
const doc = (over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec => ({
  pages: [page(1, 0), page(2, 1)],
  security: { allows: '*' },
  ...over,
  responses: [...(over.responses ?? []), FORMS_EMPTY, ANNOTS_EMPTY],
});
const engineWith = (over: Partial<FakeDocumentSpec> = {}) => ({ documents: { a: doc(over) } });

type Node = Record<string, unknown>;
const tree = (root: Node | null, incomplete = false) => ({ root, incomplete, warningFlags: 0, warnings: incomplete ? ['incomplete'] : [] });
const js = (script: string, next: Node[] = []): Node => ({ type: 'javascript', subtype: 'JavaScript', script, next });
const goto = (pon: number, next: Node[] = []): Node => ({ type: 'goto', subtype: 'GoTo', destination: { kind: 'fit', page: pr(pon) }, next });
const uri = (value: string, next: Node[] = [], isMap = false): Node => ({ type: 'uri', subtype: 'URI', uri: value, isMap, next });
const named = (name: string, next: Node[] = []): Node => ({ type: 'named', subtype: 'Named', name, next });
const hide = (targets: unknown[], hidden = true, next: Node[] = []): Node => ({ type: 'hide', subtype: 'Hide', targets, hide: hidden, next });
const reset = (fields: unknown[] | null, exclude = false, next: Node[] = []): Node => ({ type: 'reset-form', subtype: 'ResetForm', fields, exclude, next });
const bare = (type: string, extra: Record<string, unknown> = {}, next: Node[] = []): Node => ({ type, subtype: type, ...extra, next });
const byName = (name: string) => ({ kind: 'name', name });
const byNumber = (objectNumber: number) => ({ kind: 'objectNumber', objectNumber });
const FLAGS = {
  raw: 0,
  exclude: false,
  includeNoValueFields: false,
  format: 'fdf',
  method: 'post',
  submitCoordinates: false,
  includeAppendSaves: false,
  includeAnnotations: false,
  canonicalFormat: false,
  exclNonUserAnnots: false,
  exclFKey: false,
  embedForm: false,
};
const payload = (over: Record<string, unknown> = {}) => ({ url: 'https://home.test/submit', fields: null, flags: FLAGS, ...over });
const submit = (p?: unknown, next: Node[] = []): Node => ({ type: 'submit-form', subtype: 'SubmitForm', ...(p === undefined ? {} : { payload: p }), next });

const aref = (pon: number, objectNumber: number) => ({ kind: 'objectNumber', page: pr(pon), annotObjectNumber: objectNumber });
const annot = (pon: number, objectNumber: number, actions?: unknown, over: Record<string, unknown> = {}) => ({
  ref: aref(pon, objectNumber),
  page: pr(pon),
  subtype: 'square',
  rect: { left: 10, bottom: 20, right: 30, top: 40 },
  flags: { hidden: false, print: true, readOnly: false, locked: false, noView: false, toggleNoView: false },
  ...(actions === undefined ? {} : { actions }),
  ...over,
});
const annotations = (pon: number, list: unknown[], extra: Partial<ResponseSpec> = {}): ResponseSpec => ({
  path: 'page.annotations.list',
  pon,
  value: { annotations: list },
  ...extra,
});
const formField = (name: string, fieldObjectNumber: number, widgets: number[] = [], over: Record<string, unknown> = {}) => ({
  ref: { kind: 'objectNumber', fieldObjectNumber },
  fieldObjectNumber,
  name,
  family: 'text',
  valueEntry: { kind: 'scalar', value: `${name}-value` },
  defaultValueEntry: { kind: 'none' },
  flags: { readOnly: false, required: false },
  widgets: widgets.map((annotObjectNumber) => ({ ref: aref(1, annotObjectNumber), annotObjectNumber, page: pr(1) })),
  ...over,
});
const forms = (fields: unknown[], extra: Partial<ResponseSpec> = {}): ResponseSpec => ({
  path: 'forms.list',
  value: { formKind: 'acroform', needsAppearances: false, fields, calculationOrder: [] },
  ...extra,
});
const catalog = (over: Record<string, unknown> = {}, extra: Partial<ResponseSpec> = {}): ResponseSpec => ({
  path: 'actions.read',
  value: { nameTreeScripts: [], openAction: null, openDestination: null, ...over },
  ...extra,
});

const USER = { origin: 'user', source: { kind: 'api' }, event: { scope: 'activate' } };
const HOVER = { origin: 'hover', source: { kind: 'api' }, event: { scope: 'annotation', name: 'cursorEnter' } };
const LIFECYCLE = { origin: 'lifecycle', source: { kind: 'api' }, event: { scope: 'document', name: 'open' } };

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const open = (id = 'a', options?: Record<string, unknown>): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id, bytes: { $bytes: '' } }, ...(options ? [options] : [])],
});
const close = (id = 'a'): Step => ({ op: 'call', target: 'documents', method: 'close', args: [id] });
const cap = (method: string, ...args: unknown[]): Step => ({ op: 'call', target: { token: 'actions', doc: 'a' }, method, args });
const settle = (label: string, value?: unknown): Step => ({ op: 'settle', label, value });
const fail = (label: string, message: string): Step => ({ op: 'settle', label, error: { message } });
const callRef = (ref: number, ...args: unknown[]): Step => ({ op: 'call', target: { ref }, args });
const fnSpec = (name: string, answer: AnswerSpec = {}) => ({ $fn: name, ...answer });
const EXECUTED = { returns: { status: 'executed' } };
const reg = (type: string, answer: AnswerSpec = EXECUTED, name = `exec.${type}`) => cap('registerExecutor', type, fnSpec(name, answer));
const adapter = (extra: Record<string, unknown> = {}) => ({ openUri: fnSpec('ui.openUri'), print: fnSpec('ui.print'), ...extra });
const listen = (hook = 'onDiagnostic', name = hook) => cap(hook, fnSpec(name));
const FORM_OK = (count = 1) => ({
  returns: { results: Array.from({ length: count }, (_, index) => ({ index, status: 'applied', fields: [], changedWidgets: [] })), changedWidgets: [], meta: null },
});
const ANNOT_OK = (numbers: number[]) => ({ returns: { results: numbers.map((annotObjectNumber) => ({ annotObjectNumber, status: 'applied' })) } });
const RESOLVED = { url: 'https://home.test/submit', method: 'post', format: 'fdf', flagsRaw: 0, entries: [{ name: 'plain', value: 'visible' }], origin: 'user', event: { scope: 'activate' } };
const resolverEcho = (entries: unknown[] = RESOLVED.entries, extra: Record<string, unknown> = {}) =>
  fnSpec('resolver', { resolves: { ...RESOLVED, entries, ...extra } });
const plugins = (config?: HarnessConfig) => [{ factory: 'actions', ...(config === undefined ? {} : { config }) }];
const OFF: HarnessConfig = { openSequence: 'off' };
const snap = ['actions::a'];
const boot = [start, open()];

// ── the dispatcher ────────────────────────────────────────────────────────────

const listenCurrent = (hook = 'onDiagnosticReported', name = hook) => cap(hook, fnSpec(name));

const dispatcherCases: ScenarioCase[] = [
  {
    name: 'incomplete-tree-is-refused',
    covers: 'Never execute an incomplete tree, not even its root; canExecute agrees.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [...boot, reg('javascript'), listenCurrent(), cap('execute', current.tree(current.js('boot()'), true), current.USER), cap('canExecute', current.tree(current.js('boot()'), true), current.USER)],
    snapshot: snap,
  },
  {
    name: 'rootless-tree-is-inert',
    covers: 'A tree with no root is inert and counted; canExecute is false.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [...boot, cap('execute', current.tree(null), current.USER), cap('canExecute', current.tree(null), current.USER)],
    snapshot: snap,
  },
  {
    name: 'walks-next-in-pdf-order',
    covers: 'Root then /Next depth-first, with the child-index path of every node.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [...boot, reg('javascript'), cap('execute', current.tree(current.js('a', [current.js('b', [current.js('c')]), current.js('d')])), current.USER)],
    snapshot: snap,
  },
  {
    name: 'executed-then-failed-is-partial',
    covers: 'An executed node beside a failed one folds to partial.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [...boot, reg('javascript', { seq: [EXECUTED, { returns: { status: 'failed', error: 'second broke' } }] }), cap('execute', current.tree(current.js('ok', [current.js('boom')])), current.USER)],
    snapshot: snap,
  },
  {
    name: 'defers-navigation-and-external-effects',
    covers: 'goto, then javascript, then uri in PDF order: document work runs inline and the navigation and URI thunks fire after it, in node order.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [...boot, reg('javascript'), reg('goto'), cap('setUiAdapter', adapter()), cap('execute', current.tree(current.goto(3, [current.js('x', [current.uri('https://a.test/', [], true)])])), current.USER)],
    snapshot: snap,
  },
  {
    name: 'a-document-failure-drops-deferred-work',
    covers: 'A failed document node skips later document nodes (javascript, reset-form, hide) and drops every deferred thunk; the result is partial.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [
      ...boot,
      reg('goto'),
      reg('javascript', { seq: [{ returns: { status: 'failed', error: 'exploded' } }, EXECUTED] }),
      reg('reset-form'),
      cap('setUiAdapter', adapter()),
      listenCurrent(),
      cap('execute', current.tree(current.goto(1, [current.js('boom', [current.js('after'), current.reset(null), current.hide([current.byNumber(7)]), current.uri('https://a.test/')])])), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'policy-matrix-defaults',
    covers:
      'The default rows: a lifecycle uri reports, a hover submit blocks, a lifecycle Print blocks, launch and the other fixed types are never executable, recognized types without an interpreter and unknown types are no-executor; canExecute mirrors the root decision.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [
      ...boot,
      cap('setUiAdapter', adapter()),
      listenCurrent(),
      cap('execute', current.tree(current.uri('https://a.test/')), current.LIFECYCLE),
      cap('canExecute', current.tree(current.uri('https://a.test/')), current.LIFECYCLE),
      cap('canExecute', current.tree(current.uri('https://a.test/')), current.USER),
      cap('execute', current.tree(current.submit(current.payload())), current.HOVER),
      cap('execute', current.tree(current.named('Print')), current.LIFECYCLE),
      cap('canExecute', current.tree(current.named('Print')), current.USER),
      cap(
        'execute',
        current.tree(
          current.bare('launch', 'x.exe', [
            current.bare('goto-remote', 'r.pdf'),
            current.bare('goto-embedded', 'e.pdf'),
            current.bare('sound'),
            current.bare('movie'),
            current.bare('import-data'),
            current.bare('rendition'),
            current.bare('thread'),
            current.bare('set-ocg-state'),
            current.bare('transition'),
            current.bare('goto-3d-view'),
            current.bare('unknown'),
          ]),
        ),
        current.USER,
      ),
      cap('canExecute', current.tree(current.bare('launch', 'x.exe')), current.USER),
      cap('execute', current.malformedTree('bogus'), current.USER),
      cap('canExecute', current.tree(current.bare('rendition')), current.USER),
      cap('canExecute', current.tree(current.goto(1)), current.HOVER),
    ],
    snapshot: snap,
  },
  {
    name: 'a-print-typed-node-uses-the-print-row',
    covers: 'A node whose type is literally "print" finds policy.print by key, and with no executor is no-executor.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [...boot, listenCurrent(), cap('execute', current.malformedTree('print'), current.USER), cap('canExecute', current.malformedTree('print'), current.USER)],
    snapshot: snap,
  },
  {
    name: 'policy-overrides',
    covers: 'config.policy rows replace the defaults: block and report refuse with their names, allow executes even for a lifecycle uri.',
    plugins: current.plugins({
      openSequence: 'off',
      policy: {
        uri: { user: 'block', hover: 'report', lifecycle: 'allow' },
        goto: { user: 'report', hover: 'allow', lifecycle: 'allow' },
      },
    }),
    engine: current.engineWith(),
    steps: [
      ...boot,
      cap('setUiAdapter', adapter()),
      reg('goto'),
      listenCurrent(),
      cap('execute', current.tree(current.uri('https://a.test/')), current.USER),
      cap('execute', current.tree(current.uri('https://a.test/')), current.HOVER),
      cap('execute', current.tree(current.uri('https://b.test/')), current.LIFECYCLE),
      cap('execute', current.tree(current.goto(1)), current.USER),
      cap('canExecute', current.tree(current.uri('https://b.test/')), current.LIFECYCLE),
    ],
    snapshot: snap,
  },
  {
    name: 'policy-rows-merge-and-update-live',
    covers:
      'config.policy merges row-wise over the defaults (a partial row keeps the other origins); getSettings returns the effective policy; updateSettings merges policy rows, and an empty settings patch preserves them for later dispatches.',
    plugins: current.plugins({ openSequence: 'off', policy: { uri: { hover: 'allow' } } }),
    engine: current.engineWith(),
    steps: [
      ...boot,
      cap('setUiAdapter', adapter()),
      reg('goto'),
      listenCurrent(),
      cap('getSettings'),
      cap('execute', current.tree(current.uri('https://a.test/')), current.HOVER),
      cap('updateSettings', { policy: { uri: { user: 'block' }, goto: { lifecycle: 'block' } } }),
      cap('getSettings'),
      cap('execute', current.tree(current.uri('https://a.test/')), current.USER),
      cap('execute', current.tree(current.goto(1)), current.LIFECYCLE),
      cap('canExecute', current.tree(current.goto(1)), current.LIFECYCLE),
      cap('updateSettings', {}),
      cap('getSettings'),
    ],
    snapshot: snap,
  },
  {
    name: 'execute-named',
    covers:
      'executeNamed runs one Named verb as a user-origin api action through the same queue; a partial context overrides the origin, source or event; Print goes through the adapter door.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [
      ...boot,
      reg('named'),
      listenCurrent(),
      listenCurrent('onExecuted'),
      cap('executeNamed', 'NextPage'),
      cap('executeNamed', 'FirstPage', { source: { kind: 'link' }, event: { scope: 'annotation', name: 'mouseUp' } }),
      cap('executeNamed', 'Print'),
      cap('setUiAdapter', adapter()),
      cap('executeNamed', 'Print'),
      cap('executeNamed', 'Print', { origin: 'hover' }),
    ],
    snapshot: snap,
  },
  {
    name: 'get-action-tree',
    covers:
      'getActionTree reads the raw tree behind each source kind: an annotation (activate by default, or the named /AA event; the /A-shadows-U rule is not applied), a field by object number (not the first numbered field) or by name, a page (open by default or close), and the catalog (OpenAction by default, or the Table-200 tree for a verb). Anything absent is null; the required actions service returns an empty catalog for the second document.',
    plugins: current.plugins(OFF),
    engine: {
      documents: {
        a: current.doc({
          pages: [current.page(1, 0, { open: current.tree(current.named('O-1')), close: current.tree(current.named('C-1')) }), current.page(2, 1)],
          responses: [
            current.annotations(1, [current.annot(1, 5, { activate: current.tree(current.named('A-5')), mouseUp: current.tree(current.named('U-5')), cursorEnter: current.tree(current.named('E-5')) })]),
            current.forms([current.formField('f', 100, [], { actions: { keystroke: current.tree(current.js('k()')) } }), current.formField('g', 101)]),
            current.catalog({ openAction: current.tree(current.named('OpenAction')), willSave: current.tree(current.named('WS')) }),
          ],
        }),
        b: current.doc(),
      },
    },
    steps: [
      start,
      open('a'),
      open('b'),
      current.readTree( { kind: 'annotation', annotation: current.aref(1, 5), page: current.pr(1) }),
      current.readTree( { kind: 'annotation', annotation: current.aref(1, 5), page: current.pr(1), event: 'cursorEnter' }),
      current.readTree( { kind: 'annotation', annotation: current.aref(1, 5), page: current.pr(1), event: 'mouseUp' }),
      current.readTree( { kind: 'annotation', annotation: current.aref(1, 5), page: current.pr(1), event: 'blur' }),
      current.readTree( { kind: 'annotation', annotation: current.aref(1, 9), page: current.pr(1) }),
      current.readTree( { kind: 'field', field: { kind: 'objectNumber', objectNumber: 100 }, event: 'keystroke' }),
      current.readTree( { kind: 'field', field: { kind: 'fqn', name: 'f' }, event: 'keystroke' }),
      current.readTree( { kind: 'field', field: { kind: 'fqn', name: 'g' }, event: 'keystroke' }),
      current.readTree( { kind: 'field', field: { kind: 'objectNumber', objectNumber: 999 }, event: 'format' }),
      current.readTree( { kind: 'field', field: { kind: 'objectNumber', objectNumber: 101 }, event: 'keystroke' }),
      current.readTree( { kind: 'page', page: current.pr(1) }),
      current.readTree( { kind: 'page', page: current.pr(1), event: 'close' }),
      current.readTree( { kind: 'page', page: current.pr(2) }),
      current.readTree( { kind: 'page', page: current.pr(7) }),
      current.readTree( { kind: 'document' }),
      current.readTree( { kind: 'document', event: 'will-save' }),
      current.readTree( { kind: 'document', event: 'did-print' }),
      current.readTree({ kind: 'document' }, 'b'),
    ],
    ...UNSNAPSHOTTED_ACTIONS_DISPATCHER,
  },
  {
    name: 'uri-without-an-adapter',
    covers: 'A user uri with no adapter installed is no-executor with a no-adapter diagnostic.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [...boot, listenCurrent(), cap('execute', current.tree(current.uri('https://a.test/')), current.USER)],
    snapshot: snap,
  },
  {
    name: 'named-print-goes-to-the-adapter',
    covers: 'The Named Print verb reaches the adapter with no context, never the named executor; other named verbs use the executor.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [...boot, reg('named'), cap('setUiAdapter', adapter()), cap('execute', current.tree(current.named('Print', [current.named('NextPage')])), current.USER)],
    snapshot: snap,
  },
  {
    name: 'print-refusals',
    covers: 'Print with no adapter is no-executor; print without doc.print authority is blocked before the adapter.',
    plugins: current.plugins(OFF),
    engine: { documents: { a: current.doc({ security: { allows: ['doc.render'] } }), b: current.doc() } },
    steps: [
      start,
      open('a'),
      open('b'),
      listenCurrent(),
      { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'onDiagnosticReported', args: [fnSpec('b.onDiagnostic')] },
      { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'execute', args: [current.tree(current.named('Print')), current.USER] },
      cap('setUiAdapter', adapter()),
      cap('execute', current.tree(current.named('Print')), current.USER),
    ],
    ...UNSNAPSHOTTED_ACTIONS_DISPATCHER,
  },
  {
    name: 'hide-routes-by-plane',
    covers:
      'A hide name resolves to its field (form setDisplay), a widget object number to its owning field, and a plain object number to an annotation flag patch; the form plane commits first.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({ responses: [current.forms([current.formField('note1', 100, [41, 42]), current.formField('other', 101, [43])])] }),
    steps: [
      ...boot,
      cap('registerFormCommitSink', fnSpec('sink.form', current.FORM_OK())),
      cap('registerAnnotCommitSink', fnSpec('sink.annot', current.ANNOT_OK([7]))),
      listenCurrent(),
      cap('execute', current.tree(current.hide([current.byName('note1'), current.byNumber(7), current.byNumber(43)], false)), current.USER),
      cap('execute', current.tree(current.hide([current.byNumber(42)], true)), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'hide-and-later-document-work',
    covers: 'An executed hide leaves later document nodes running; a failed hide skips them.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [
      ...boot,
      reg('reset-form'),
      cap('registerAnnotCommitSink', fnSpec('sink.annot', { seq: [current.ANNOT_OK([7]), { returns: { results: [{ annotObjectNumber: 7, status: 'failed', error: 'refused' }] } }] })),
      cap('execute', current.tree(current.hide([current.byNumber(7)], true, [current.reset(null)])), current.USER),
      cap('execute', current.tree(current.hide([current.byNumber(7)], true, [current.reset(null)])), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'hide-without-targets',
    covers: 'A hide with no targets reads nothing and executes.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [...boot, cap('execute', current.tree(current.hide([])), current.USER)],
    snapshot: snap,
  },
  {
    name: 'hide-reports-honestly',
    covers:
      'Missing sinks are no-executor, an unresolved name is a diagnostic (the node still executes), an annotation refusal fails the node only when every entry failed, and a failure without an error string executes.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({ responses: [current.forms([current.formField('note1', 100, [41])])] }),
    steps: [
      ...boot,
      listenCurrent(),
      cap('execute', current.tree(current.hide([current.byNumber(7)])), current.USER),
      cap('execute', current.tree(current.hide([current.byName('note1')])), current.USER),
      cap('execute', current.tree(current.hide([current.byName('ghost')])), current.USER),
      cap(
        'registerAnnotCommitSink',
        fnSpec('sink.annot', {
          seq: [
            { returns: { results: [{ annotObjectNumber: 7, status: 'failed', error: 'PermissionDenied: doc.annotate.modify' }] } },
            { returns: { results: [{ annotObjectNumber: 7, status: 'failed', error: 'first refused' }, { annotObjectNumber: 8, status: 'applied' }] } },
            { returns: { results: [{ annotObjectNumber: 7, status: 'failed' }, { annotObjectNumber: 8, status: 'failed', error: 'second' }] } },
            { returns: { results: [] } },
          ],
        }),
      ),
      cap('execute', current.tree(current.hide([current.byNumber(7)])), current.USER),
      cap('execute', current.tree(current.hide([current.byNumber(7), current.byNumber(8)])), current.USER),
      cap('execute', current.tree(current.hide([current.byNumber(7), current.byNumber(8)])), current.USER),
      cap('execute', current.tree(current.hide([current.byNumber(7)])), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'hide-form-failures',
    covers: 'A form sink failure (with an error message or without one) fails the node and skips the annotation plane.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({ responses: [current.forms([current.formField('note1', 100)])] }),
    steps: [
      ...boot,
      listenCurrent(),
      cap(
        'registerFormCommitSink',
        fnSpec('sink.form', {
          seq: [
            current.formFailure('form refused'),
            current.formFailure(),
            current.FORM_OK(),
          ],
        }),
      ),
      cap('registerAnnotCommitSink', fnSpec('sink.annot', current.ANNOT_OK([7]))),
      cap('execute', current.tree(current.hide([current.byName('note1'), current.byNumber(7)])), current.USER),
      cap('execute', current.tree(current.hide([current.byName('note1'), current.byNumber(7)])), current.USER),
      cap('execute', current.tree(current.hide([current.byName('note1')])), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'hide-sink-throws',
    covers: 'A sink that throws synchronously inside hide is not caught by the dispatcher: execute rejects and nothing is counted.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [...boot, cap('registerAnnotCommitSink', fnSpec('sink.annot', { throws: { message: 'sink broke' } })), cap('execute', current.tree(current.hide([current.byNumber(7)])), current.USER), cap('execute', current.tree(current.hide([])), current.USER)],
    snapshot: snap,
  },
  {
    name: 'hide-forms-read-rejects',
    covers: 'A forms.list rejection inside hide rejects execute.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({ responses: [{ path: 'forms.list', error: { name: 'EngineError', message: 'closed', code: 'DocClosed' } }] }),
    steps: [...boot, cap('execute', current.tree(current.hide([current.byName('x')])), current.USER)],
    snapshot: snap,
  },
  {
    name: 'document-executors',
    covers:
      'reset-form and javascript without an executor (javascript is inert, "scripting unavailable"); an executor that is inert, fails, throws, rejects, or answers later.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [
      ...boot,
      listenCurrent(),
      cap('execute', current.tree(current.reset(null)), current.USER),
      cap('execute', current.tree(current.js('orphan()')), current.USER),
      reg('reset-form', {
        seq: [
          { returns: { status: 'inert', reason: 'nothing to reset' } },
          { returns: { status: 'failed', error: 'reset refused' } },
          { throws: { message: 'reset threw' } },
          { rejects: { message: 'reset rejected' } },
          { defer: 'reset' },
          { resolves: { status: 'executed' } },
        ],
      }),
      cap('execute', current.tree(current.reset([current.byName('a')], true)), current.USER),
      cap('execute', current.tree(current.reset([])), current.USER),
      cap('execute', current.tree(current.reset(null)), current.USER),
      cap('execute', current.tree(current.reset(null)), current.USER),
      cap('execute', current.tree(current.reset(null)), current.USER),
      settle('reset', { status: 'executed' }),
      cap('execute', current.tree(current.reset(null)), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'navigation-executors',
    covers: 'Deferred goto/named: no executor, inert, throws, rejects, executed.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [
      ...boot,
      listenCurrent(),
      cap('execute', current.tree(current.goto(1, [current.named('NextPage')])), current.USER),
      reg('goto', {
        seq: [
          { returns: { status: 'inert', reason: 'no stage' } },
          { throws: { message: 'goto threw' } },
          { rejects: { message: 'goto rejected' } },
          EXECUTED,
        ],
      }),
      cap('execute', current.tree(current.goto(1)), current.USER),
      cap('execute', current.tree(current.goto(1)), current.USER),
      cap('execute', current.tree(current.goto(1)), current.USER),
      cap('execute', current.tree(current.goto(1)), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'executors-are-last-wins-and-identity-safe',
    covers: 'A duplicate registration replaces the executor with a duplicate-executor diagnostic; a stale disposer leaves the successor; the current disposer removes it.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [
      ...boot,
      listenCurrent(),
      reg('javascript', EXECUTED, 'first'),
      reg('javascript', EXECUTED, 'second'),
      callRef(1),
      cap('execute', current.tree(current.js('x')), current.USER),
      callRef(2),
      cap('execute', current.tree(current.js('x')), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'adapter-is-identity-safe',
    covers: 'A stale adapter disposer leaves the successor; the current one clears it; setUiAdapter(null) force-clears.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [
      ...boot,
      cap('setUiAdapter', { openUri: fnSpec('first.openUri'), print: fnSpec('first.print') }),
      cap('setUiAdapter', { openUri: fnSpec('second.openUri'), print: fnSpec('second.print') }),
      callRef(0),
      cap('execute', current.tree(current.uri('https://a.test/')), current.USER),
      callRef(1),
      cap('execute', current.tree(current.uri('https://a.test/')), current.USER),
      cap('setUiAdapter', adapter()),
      cap('setUiAdapter', null),
      cap('execute', current.tree(current.uri('https://a.test/')), current.USER),
      callRef(2),
      callRef(3),
    ],
    snapshot: snap,
  },
  {
    name: 'sinks-and-resolver-are-identity-safe',
    covers: 'Commit sinks, the submit resolver and the submit handler all install last-wins and clear only while current.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({ services: ['forms.submit'], responses: [{ path: 'forms.submit', value: current.receipt('s') }] }),
    steps: [
      ...boot,
      listenCurrent(),
      cap('registerAnnotCommitSink', fnSpec('annot.first', current.ANNOT_OK([7]))),
      cap('registerAnnotCommitSink', fnSpec('annot.second', current.ANNOT_OK([7]))),
      callRef(1),
      cap('execute', current.tree(current.hide([current.byNumber(7)])), current.USER),
      callRef(2),
      cap('execute', current.tree(current.hide([current.byNumber(7)])), current.USER),
      cap('registerFormCommitSink', fnSpec('form.first', current.FORM_OK())),
      cap('registerFormCommitSink', fnSpec('form.second', current.FORM_OK())),
      callRef(3),
      callRef(4),
      cap('registerSubmitResolver', current.resolverEcho()),
      cap('registerSubmitResolver', current.resolverEcho()),
      callRef(5),
      cap('setSubmitHandler', fnSpec('handler.first')),
      cap('setSubmitHandler', fnSpec('handler.second')),
      callRef(7),
      cap('execute', current.tree(current.submit(current.payload())), current.USER),
      callRef(8),
      cap('execute', current.tree(current.submit(current.payload())), current.USER),
      callRef(6),
      cap('execute', current.tree(current.submit(current.payload())), current.USER),
    ],
    ...UNSNAPSHOTTED_ACTIONS_DISPATCHER,
  },
  {
    name: 'dispatches-serialize',
    covers: 'One queue: a slow execute holds the next until it settles; both dispatch results settle in submission order.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [...boot, reg('javascript', { seq: [{ defer: 'slow' }, EXECUTED] }), cap('execute', current.tree(current.js('slow')), current.USER), cap('execute', current.tree(current.js('fast')), current.USER), settle('slow', { status: 'executed' })],
    snapshot: snap,
  },
  {
    name: 'hooks-deliver-and-isolate',
    covers:
      'onExecuted carries source, tree and result; a throwing observer is reported to the kernel error sink and its siblings still run; an unsubscribed listener hears nothing.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [
      ...boot,
      cap('onExecuted', fnSpec('onAction.bad', { throws: { message: 'observer broke' } })),
      cap('onExecuted', fnSpec('onAction')),
      cap('onDiagnosticReported', fnSpec('onDiagnostic.bad', { throws: { message: 'diag observer broke' } })),
      listenCurrent(),
      cap('execute', current.tree(current.js('orphan()')), current.USER),
      callRef(1),
      callRef(3),
      cap('execute', current.tree(current.js('orphan()')), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'a-user-execute-arms-the-open-sequence',
    covers: 'Under auto, a user-origin execute runs the open sequence ahead of itself; a hover execute does not arm it.',
    plugins: current.plugins(),
    engine: current.engineWith({ responses: [current.catalog({ openAction: current.tree(current.named('OpenAction')) })] }),
    steps: [...boot, reg('named'), cap('execute', current.tree(current.named('Hover')), current.HOVER), cap('execute', current.tree(current.named('Click')), current.USER), cap('execute', current.tree(current.named('Again')), current.USER)],
    snapshot: snap,
  },
];

// ── the submit chain ──────────────────────────────────────────────────────────

const submitEngine = (over: Partial<FakeDocumentSpec> = {}) => current.engineWith(over);
const submitCases: ScenarioCase[] = [
  {
    name: 'payload-less-submit-is-inert',
    covers: 'An older-runtime SubmitForm with no payload is recognized-inert with its own diagnostic.',
    plugins: current.plugins(OFF),
    engine: submitEngine(),
    steps: [...boot, listenCurrent(), cap('execute', current.tree(current.submit()), current.USER)],
    snapshot: snap,
  },
  {
    name: 'no-resolver',
    covers: 'Without a dataset resolver every submit blocks with no-submit-resolver.',
    plugins: current.plugins(OFF),
    engine: submitEngine(),
    steps: [...boot, listenCurrent(), cap('execute', current.tree(current.submit(current.payload())), current.USER)],
    snapshot: snap,
  },
  {
    name: 'resolver-failures',
    covers: 'A resolver that throws or rejects fails the node with the dataset-resolution diagnostic.',
    plugins: current.plugins(OFF),
    engine: submitEngine(),
    steps: [
      ...boot,
      listenCurrent(),
      cap('registerSubmitResolver', fnSpec('resolver', { seq: [{ throws: { message: 'no fields' } }, { rejects: { message: 'field plane gone' } }] })),
      cap('execute', current.tree(current.submit(current.payload())), current.USER),
      cap('execute', current.tree(current.submit(current.payload())), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'no-sink',
    covers: 'With a resolver but no handler and no submit-capable home, the submit blocks with no-submit-sink.',
    plugins: current.plugins(OFF),
    engine: submitEngine(),
    steps: [...boot, listenCurrent(), cap('registerSubmitResolver', current.resolverEcho()), cap('execute', current.tree(current.submit(current.payload({ fields: [current.byName('a'), current.byNumber(9)], charSet: 'utf-8' }))), current.USER)],
    snapshot: snap,
  },
  {
    name: 'the-document-home',
    covers: 'The home is awaited with the request shape it expects (charSet only when present, the clock for ISO sentAt); a refusal fails the node.',
    plugins: current.plugins(OFF),
    engine: submitEngine({
      services: ['forms.submit'],
      responses: [
        { path: 'forms.submit', times: 1, value: current.receipt('s-1') },
        { path: 'forms.submit', error: { name: 'PermissionDenied', message: 'permission denied: doc.forms.submit', required: 'doc.forms.submit' } },
      ],
    }),
    steps: [
      ...boot,
      listenCurrent(),
      cap('registerSubmitResolver', fnSpec('resolver', { seq: [{ resolves: current.RESOLVED }, { resolves: { ...RESOLVED, charSet: 'latin1' } }] })),
      { op: 'advance', ms: 1500 },
      cap('execute', current.tree(current.submit(current.payload())), current.USER),
      cap('execute', current.tree(current.submit(current.payload())), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'the-home-uses-the-configured-clock',
    covers: 'ISO sentAt comes from javascript.now when configured, even with scripting disabled.',
    plugins: current.plugins({ openSequence: 'off', javascript: { enabled: false, now: fnSpec('now', { returns: 1234 }) } }),
    engine: submitEngine({ services: ['forms.submit'], responses: [{ path: 'forms.submit', value: current.receipt('s') }] }),
    steps: [...boot, cap('registerSubmitResolver', current.resolverEcho()), cap('execute', current.tree(current.submit(current.payload())), current.USER)],
    snapshot: snap,
  },
  {
    name: 'a-handler-beats-the-home',
    covers: 'An installed handler receives the request and a home delegate (or null without a home); synchronous acceptance executes and the home never auto-fires.',
    plugins: current.plugins(OFF),
    engine: { documents: { a: current.doc({ services: ['forms.submit'] }), b: current.doc() } },
    steps: [
      start,
      open('a'),
      open('b'),
      cap('registerSubmitResolver', current.resolverEcho([], { charSet: 'utf-8' })),
      cap('setSubmitHandler', fnSpec('handler', { returns: { not: 'a promise' } })),
      cap('execute', current.tree(current.submit(current.payload())), current.USER),
      { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'registerSubmitResolver', args: [current.resolverEcho()] },
      { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'setSubmitHandler', args: [fnSpec('handler.b')] },
      { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'execute', args: [current.tree(current.submit(current.payload())), current.USER] },
    ],
    ...UNSNAPSHOTTED_ACTIONS_SUBMIT,
  },
  {
    name: 'handler-contract',
    covers: 'A synchronous throw fails the node; a returned promise is detached: resolving says nothing, rejecting emits a diagnostic later and never rewrites the node.',
    plugins: current.plugins(OFF),
    engine: submitEngine(),
    steps: [
      ...boot,
      listenCurrent(),
      cap('registerSubmitResolver', current.resolverEcho()),
      cap('setSubmitHandler', fnSpec('handler', { seq: [{ throws: { message: 'sync refuse' } }, { resolves: 'ok' }, { defer: 'network' }] })),
      cap('execute', current.tree(current.submit(current.payload())), current.USER),
      cap('execute', current.tree(current.submit(current.payload())), current.USER),
      cap('execute', current.tree(current.submit(current.payload())), current.USER),
      fail('network', 'late network error'),
    ],
    snapshot: snap,
  },
  {
    name: 'submit-policy',
    covers: 'A lifecycle submit is blocked by default; a policy row of allow or adapter admits it, and report or block refuse.',
    plugins: current.plugins({ openSequence: 'off', policy: { 'submit-form': { user: 'report', hover: 'allow', lifecycle: 'adapter' } } }),
    engine: submitEngine(),
    steps: [
      ...boot,
      listenCurrent(),
      cap('registerSubmitResolver', current.resolverEcho()),
      cap('setSubmitHandler', fnSpec('handler')),
      cap('execute', current.tree(current.submit(current.payload())), current.USER),
      cap('execute', current.tree(current.submit(current.payload())), current.HOVER),
      cap('execute', current.tree(current.submit(current.payload())), current.LIFECYCLE),
    ],
    snapshot: snap,
  },
  {
    name: 'submit-is-deferred-behind-document-work',
    covers: 'A submit before a failing document node never fires.',
    plugins: current.plugins(OFF),
    engine: submitEngine(),
    steps: [...boot, cap('registerSubmitResolver', current.resolverEcho()), cap('setSubmitHandler', fnSpec('handler')), reg('javascript', { returns: { status: 'failed', error: 'x' } }), cap('execute', current.tree(current.submit(current.payload(), [current.js('boom')])), current.USER)],
    snapshot: snap,
  },
];

// ── triggers ──────────────────────────────────────────────────────────────────

const activate = (pon: number, ref: unknown, source?: unknown) => ({ scope: 'activate', ref, page: pr(pon), ...(source === undefined ? {} : { source }) });
const annotationEvent = (event: string, pon: number, ref: unknown, source?: unknown) => ({ scope: 'annotation', event, ref, page: pr(pon), ...(source === undefined ? {} : { source }) });
const pageEvent = (event: string, pon: number) => ({ scope: 'page', event, page: pr(pon) });
const docEvent = (event: string) => ({ scope: 'document', event });
const dispatch = (trigger: unknown) => cap('dispatch', trigger);
const nmRef = (pon: number, nm: string) => ({ kind: 'nm', page: pr(pon), nm });
const indexRef = (pon: number, index: number, revision: unknown = 'r1') => ({ kind: 'index', page: pr(pon), index, revision });

/** C3 inputs are typed before recording; legacy builders above remain for C4/link. */
const baseRef = (page: number, baseIndex: number): AnnotationRef => ({
  kind: 'baseIndex',
  page: current.pr(page),
  baseIndex,
});
const lifecycleAnnot = (
  page: number,
  number: number,
  actions?: Annotation['actions'],
  over: { ref?: AnnotationRef } = {},
): Annotation => ({ ...current.annot(page, number, actions), ...over });
const activateCurrent = (
  page: number,
  ref: AnnotationRef,
  source?: ActionSource,
): ActionTrigger => ({
  scope: 'activate',
  page: current.pr(page),
  ref,
  ...(source ? { source } : {}),
});
const annotationCurrent = (
  event: PdfAnnotationEventKind,
  page: number,
  ref: AnnotationRef,
  source?: ActionSource,
): ActionTrigger => ({
  scope: 'annotation',
  event,
  page: current.pr(page),
  ref,
  ...(source ? { source } : {}),
});
const fanPage5 = [
  lifecycleAnnot(5, 11, {
    pageOpen: current.tree(current.named('PO-11')),
    pageClose: current.tree(current.named('PC-11')),
    pageVisible: current.tree(current.named('PV-11')),
    pageInvisible: current.tree(current.named('PI-11')),
  }),
  lifecycleAnnot(5, 12, { pageOpen: current.tree(current.named('PO-12')) }),
  lifecycleAnnot(5, 13),
  lifecycleAnnot(5, 14, {
    pageOpen: current.tree(null),
    cursorEnter: current.tree(current.named('E-14')),
  }),
];
const fanEngine = current.engineWith({
  pages: [
    current.page(5, 0, {
      open: current.tree(current.named('pageO')),
      close: current.tree(current.named('pageC')),
    }),
    current.page(6, 1, { open: current.tree(null) }),
  ],
  responses: [current.annotations(5, fanPage5)],
});

const triggerCases: ScenarioCase[] = [
  {
    name: 'activate-resolves-the-annotation-tree',
    covers:
      'An activate trigger resolves current objectNumber and stable baseIndex refs on their own pages; mismatched kinds, pages and absent refs find nothing. Historical NM/revision-index spellings are archived.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({
      responses: [
        current.annotations(2, [
          lifecycleAnnot(
            2,
            0,
            { activate: current.tree(current.named('click-numberless')) },
            { ref: { kind: 'baseIndex', page: current.pr(2), baseIndex: 0 } },
          ),
        ]),
        current.annotations(1, [
          lifecycleAnnot(1, 9, { activate: current.tree(current.named('click-9')) }),
          lifecycleAnnot(
            1,
            10,
            { activate: current.tree(current.named('click-nm')) },
            { ref: baseRef(1, 2) },
          ),
          lifecycleAnnot(
            1,
            11,
            { activate: current.tree(current.named('click-index')) },
            { ref: baseRef(1, 3) },
          ),
        ]),
      ],
    }),
    steps: [
      ...boot,
      reg('named'),
      dispatch(activateCurrent(1, current.aref(1, 9))),
      dispatch(activateCurrent(1, current.aref(99, 9))),
      dispatch(activateCurrent(1, baseRef(1, 2))),
      dispatch(activateCurrent(1, baseRef(2, 2))),
      dispatch(activateCurrent(1, baseRef(1, 99))),
      dispatch(activateCurrent(1, baseRef(1, 3))),
      dispatch(activateCurrent(1, baseRef(2, 3))),
      dispatch(activateCurrent(1, baseRef(1, 4))),
      dispatch(activateCurrent(1, current.aref(1, 10))),
      dispatch(activateCurrent(1, current.aref(1, 404))),
      dispatch(activateCurrent(2, { kind: 'baseIndex', page: current.pr(2), baseIndex: 0 })),
    ],
    snapshot: snap,
  },
  {
    name: 'annotation-events-resolve-their-aa-tree',
    covers:
      'Each of the six /AA pointer events resolves its own tree; absence is silently inert; the hover pair is hover-origin and the rest are user-origin.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({
      responses: [
        current.annotations(4, [
          lifecycleAnnot(4, 7, {
            cursorEnter: current.tree(current.named('E')),
            cursorExit: current.tree(current.named('X')),
            mouseDown: current.tree(current.named('D')),
            mouseUp: current.tree(current.named('U')),
            focus: current.tree(current.named('Fo')),
            blur: current.tree(current.named('Bl')),
          }),
          lifecycleAnnot(4, 8),
        ]),
      ],
      pages: [current.page(4, 0)],
    }),
    steps: [
      ...boot,
      reg('named'),
      listenCurrent('onExecuted'),
      ...(
        [
          'cursorEnter',
          'cursorExit',
          'mouseDown',
          'mouseUp',
          'focus',
          'blur',
        ] satisfies PdfAnnotationEventKind[]
      ).map((event) => dispatch(annotationCurrent(event, 4, current.aref(4, 7)))),
      dispatch(annotationCurrent('cursorEnter', 4, current.aref(4, 8))),
    ],
    snapshot: snap,
  },
  {
    name: 'a-source-hint-is-provenance-only',
    covers:
      'A first-party source hint becomes the step source, and the origin is still derived from the trigger.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({
      responses: [
        current.annotations(1, [
          lifecycleAnnot(1, 7, { cursorEnter: current.tree(current.named('e')) }),
        ]),
      ],
    }),
    steps: [
      ...boot,
      reg('named'),
      listenCurrent('onExecuted'),
      dispatch(
        annotationCurrent('cursorEnter', 1, current.aref(1, 7), {
          kind: 'link',
          annotation: current.aref(1, 7),
          page: current.pr(1),
        }),
      ),
    ],
    snapshot: snap,
  },
  {
    name: 'a-precedes-aa-u',
    covers:
      '/A shadows the /AA U tree: a mouseUp on an annotation with /A is inert; without /A its U tree runs.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({
      responses: [
        current.annotations(1, [
          lifecycleAnnot(1, 1, {
            activate: current.tree(current.named('A-1')),
            mouseUp: current.tree(current.named('U-1')),
          }),
          lifecycleAnnot(1, 2, { mouseUp: current.tree(current.named('U-2')) }),
        ]),
      ],
    }),
    steps: [
      ...boot,
      reg('named'),
      dispatch(annotationCurrent('mouseUp', 1, current.aref(1, 1))),
      dispatch(annotationCurrent('mouseUp', 1, current.aref(1, 2))),
    ],
    snapshot: snap,
  },
  {
    name: 'an-incomplete-trigger-tree-is-refused',
    covers:
      'A rootless but incomplete tree still becomes a step, and the step is refused; the trigger folds to refused.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({
      responses: [
        current.annotations(1, [lifecycleAnnot(1, 1, { activate: current.tree(null, true) })]),
      ],
    }),
    steps: [...boot, listenCurrent(), dispatch(activateCurrent(1, current.aref(1, 1)))],
    snapshot: snap,
  },
  {
    name: 'resolution-never-rejects',
    covers:
      'A failed annotation read and an unknown page both come back refused with trigger-failed; dispatch never rejects.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({
      responses: [
        {
          path: 'page.annotations.list',
          pon: 1,
          error: { name: 'EngineError', message: 'read exploded', code: 'Io' },
        },
      ],
    }),
    steps: [
      ...boot,
      listenCurrent(),
      dispatch(annotationCurrent('cursorEnter', 1, current.aref(1, 1))),
      dispatch(activateCurrent(77, current.aref(77, 1))),
      dispatch(current.pageEvent('open', 77)),
    ],
    snapshot: snap,
  },
  {
    name: 'submission-order-is-execution-order',
    covers: 'Two dispatches run in call order even when the first read is slower.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({
      pages: [
        current.page(1, 0, { close: current.tree(current.named('closeA')) }),
        current.page(2, 1, { open: current.tree(current.named('openB')) }),
      ],
      responses: [current.annotations(1, [], { defer: 'slow-list' })],
    }),
    steps: [
      ...boot,
      reg('named'),
      dispatch(current.pageEvent('close', 1)),
      dispatch(current.pageEvent('open', 2)),
      settle('slow-list', { annotations: [] }),
    ],
    snapshot: snap,
  },
  {
    name: 'page-fan-out-order',
    covers:
      'open runs page /O then the /PO set; close runs the /PC set then /C; visible and invisible fan only their sets; rootless trees and tree-less annotations are no steps.',
    plugins: current.plugins(OFF),
    engine: fanEngine,
    steps: [
      ...boot,
      reg('named'),
      listenCurrent('onExecuted'),
      dispatch(current.pageEvent('open', 5)),
      dispatch(current.pageEvent('close', 5)),
      dispatch(current.pageEvent('visible', 5)),
      dispatch(current.pageEvent('invisible', 5)),
      dispatch(current.pageEvent('open', 6)),
    ],
    snapshot: snap,
  },
  {
    name: 'a-failed-step-never-skips-its-siblings',
    covers:
      'A /PC step that fails is partial; the page /C still runs, and the trigger folds to partial.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({
      pages: [current.page(6, 0, { close: current.tree(current.named('pageC')) })],
      responses: [
        current.annotations(6, [
          lifecycleAnnot(6, 21, { pageClose: current.tree(current.js('boom')) }),
        ]),
      ],
    }),
    steps: [
      ...boot,
      reg('named'),
      reg('javascript', { returns: { status: 'failed', error: 'boom' } }),
      dispatch(current.pageEvent('close', 6)),
    ],
    snapshot: snap,
  },
  {
    name: 'single-tree-lifecycle-annotations',
    covers:
      'An annotation bearing only one lifecycle tree (close, visible or invisible) is still in the lifecycle set.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({
      pages: [current.page(6, 0)],
      responses: [
        current.annotations(6, [
          lifecycleAnnot(6, 31, { pageClose: current.tree(current.named('PC-31')) }),
          lifecycleAnnot(6, 32, { pageVisible: current.tree(current.named('PV-32')) }),
          lifecycleAnnot(6, 33, { pageInvisible: current.tree(current.named('PI-33')) }),
        ]),
      ],
    }),
    steps: [
      ...boot,
      reg('named'),
      dispatch(current.pageEvent('close', 6)),
      dispatch(current.pageEvent('visible', 6)),
      dispatch(current.pageEvent('invisible', 6)),
    ],
    snapshot: snap,
  },
  {
    name: 'an-all-inert-fan-out-is-inert',
    covers: 'Steps that all come back inert fold to inert.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({
      pages: [current.page(6, 0, { open: current.tree(current.js('orphan()')) })],
      responses: [
        current.annotations(6, [
          lifecycleAnnot(6, 34, { pageOpen: current.tree(current.named('Print')) }),
        ]),
      ],
    }),
    steps: [...boot, dispatch(current.pageEvent('open', 6))],
    snapshot: snap,
  },
  {
    name: 'deferred-navigation-flushes-per-step',
    covers: 'A page /O goto fires inside its own step, before the /PO step runs.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({
      pages: [current.page(7, 0, { open: current.tree(current.goto(2)) })],
      responses: [
        current.annotations(7, [
          lifecycleAnnot(7, 31, { pageOpen: current.tree(current.named('PO')) }),
        ]),
      ],
    }),
    steps: [...boot, reg('named'), reg('goto'), dispatch(current.pageEvent('open', 7))],
    snapshot: snap,
  },
  {
    name: 'lifecycle-cache-and-invalidation',
    covers:
      'Lifecycle trees are read once per page; confirmed annotations.updated for that page and stream.desynced clear the cache, other events do not; close unsubscribes.',
    plugins: current.plugins(OFF),
    engine: fanEngine,
    steps: [
      ...boot,
      reg('named'),
      dispatch(current.pageEvent('visible', 5)),
      dispatch(current.pageEvent('invisible', 5)),
      {
        op: 'emit',
        doc: 'a',
        event: {
          type: 'metadata.updated',
          origin: { kind: 'remote', sessionId: 'peer', sub: null, ts: 0, serverId: null },
          metadata: {
            title: 'Changed',
            author: null,
            subject: null,
            keywords: null,
            producer: null,
            creator: null,
            createdAt: null,
            modifiedAt: null,
            trapped: 'unknown',
          },
          meta: { affectedPages: [], cacheDelta: null, opId: 'remote-metadata', undoable: true },
        } satisfies DocumentEvent,
      },
      dispatch(current.pageEvent('visible', 5)),
      { op: 'emit', doc: 'a', event: updatedFact(current.annot(5, 11)) },
      dispatch(current.pageEvent('visible', 5)),
      {
        op: 'emit',
        doc: 'a',
        event: {
          type: 'stream.desynced',
          reason: 'backlog-overflow',
          ts: 0,
        } satisfies DocumentEvent,
      },
      dispatch(current.pageEvent('visible', 5)),
      close(),
      open(),
      reg('named'),
      dispatch(current.pageEvent('visible', 5)),
      close(),
    ],
    ...UNSNAPSHOTTED_ACTIONS_TRIGGERS,
  },
  {
    name: 'trigger-gates',
    covers:
      'Disabled families are inert with trigger-disabled and canDispatch false; activate is never gated.',
    plugins: current.plugins({
      openSequence: 'off',
      triggers: { page: false, annotation: false, document: false },
    }),
    engine: current.engineWith({
      pages: [current.page(1, 0, { open: current.tree(current.named('O')) })],
    }),
    steps: [
      ...boot,
      reg('named'),
      listenCurrent(),
      dispatch(current.pageEvent('open', 1)),
      dispatch(annotationCurrent('cursorEnter', 1, current.aref(1, 1))),
      dispatch(current.docEvent('open')),
      cap('canDispatch', current.pageEvent('open', 1)),
      cap('canDispatch', annotationCurrent('cursorEnter', 1, current.aref(1, 1))),
      cap('canDispatch', current.docEvent('will-save')),
      cap('canDispatch', activateCurrent(1, current.aref(1, 1))),
      dispatch(activateCurrent(1, current.aref(1, 1))),
    ],
    snapshot: snap,
  },
  {
    name: 'gates-default-open',
    covers: 'A triggers object with no families set leaves every family enabled.',
    plugins: current.plugins({ openSequence: 'off', triggers: {} }),
    engine: current.engineWith(),
    steps: [
      ...boot,
      cap('canDispatch', current.pageEvent('open', 1)),
      cap('canDispatch', annotationCurrent('focus', 1, current.aref(1, 1))),
      cap('canDispatch', current.docEvent('open')),
    ],
    ...UNSNAPSHOTTED_ACTIONS_TRIGGERS,
  },
];

// ── the open sequence, the barrier and the coordinator ─────────────────────────

const reportCurrent = (
  currentPon: number | null,
  visiblePons: number[],
  cause: PageStateReport['cause'] = 'user',
  placed = true,
) =>
  cap('reportPageState', {
    currentPage: currentPon === null ? null : current.pr(currentPon),
    visiblePages: visiblePons.map(current.pr),
    placed,
    cause,
  } satisfies PageStateReport);
// A valid inert document event drains queued work without a fabricated missing-page failure.
const drainTriggerCurrent = dispatch(current.docEvent('did-save'));
const openDocs = {
  openAction: current.tree(current.named('OpenAction')),
  openDestination: { kind: 'fit', page: current.pr(3) },
} satisfies Partial<DocumentActionsSnapshot>;
const threePages = [
  current.page(1, 0, { open: current.tree(current.named('O-1')) }),
  current.page(3, 1, { open: current.tree(current.named('O-3')) }),
];

const sequenceCases: ScenarioCase[] = [
  {
    name: 'auto-fires-on-adapter-install',
    covers:
      'Pre-open reports are buffered and coalesced; an adapter install runs the openDestination goto, then OpenAction, then the page open the latest report asks for; another adapter never replays it.',
    plugins: current.plugins(),
    engine: current.engineWith({
      services: ['actions'],
      pages: threePages,
      responses: [current.catalog(openDocs)],
    }),
    steps: [
      ...boot,
      reg('named'),
      reg('goto'),
      reportCurrent(1, [1], 'programmatic'),
      reportCurrent(3, [3], 'programmatic'),
      cap('setUiAdapter', adapter()),
      drainTriggerCurrent,
      cap('setUiAdapter', adapter()),
      drainTriggerCurrent,
    ],
    snapshot: snap,
  },
  {
    name: 'auto-fires-ahead-of-the-first-user-dispatch',
    covers: 'The first user-origin dispatch arms the sequence before taking its own queue slot.',
    plugins: current.plugins(),
    engine: current.engineWith({
      services: ['actions'],
      pages: [current.page(2, 0)],
      responses: [
        current.catalog({ openAction: current.tree(current.named('OpenAction')) }),
        current.annotations(2, [
          lifecycleAnnot(2, 9, { activate: current.tree(current.named('click')) }),
        ]),
      ],
    }),
    steps: [...boot, reg('named'), dispatch(activateCurrent(2, current.aref(2, 9)))],
    snapshot: snap,
  },
  {
    name: 'open-sequence-completed-event',
    covers:
      'onOpenSequenceCompleted fires once with the folded sequence result after the document steps; a throwing listener is reported to the host sink and never stops its siblings.',
    plugins: current.plugins(),
    engine: current.engineWith({
      services: ['actions'],
      pages: threePages,
      responses: [current.catalog(openDocs)],
    }),
    steps: [
      ...boot,
      reg('goto'),
      reg('named'),
      cap(
        'onOpenSequenceCompleted',
        fnSpec('completed.bad', { throws: { message: 'listener broke' } }),
      ),
      listenCurrent('onOpenSequenceCompleted', 'completed'),
      cap('setUiAdapter', adapter()),
      drainTriggerCurrent,
      cap('setUiAdapter', adapter()),
      drainTriggerCurrent,
    ],
    snapshot: snap,
  },
  {
    name: 'auto-a-submit-handler-does-not-arm',
    covers:
      'Installing a submit handler is not the latch release: nothing runs until an adapter arrives.',
    plugins: current.plugins(),
    engine: current.engineWith({
      services: ['actions'],
      responses: [current.catalog({ openAction: current.tree(current.named('OpenAction')) })],
    }),
    steps: [
      ...boot,
      reg('named'),
      cap('setSubmitHandler', fnSpec('handler')),
      cap('setUiAdapter', null),
      cap('setUiAdapter', adapter()),
    ],
    snapshot: snap,
  },
  {
    name: 'headless-falls-back-to-the-first-page',
    covers:
      'headless fires at bring-up; with no stage report the initial page open is the first page.',
    plugins: current.plugins({ openSequence: 'headless' }),
    engine: current.engineWith({
      services: ['actions'],
      pages: [current.page(8, 0, { open: current.tree(current.named('O-8')) })],
      responses: [current.catalog({}, { defer: 'fallback-catalog' })],
    }),
    steps: [
      ...boot,
      reg('named'),
      settle('fallback-catalog', {
        nameTreeScripts: [],
        openAction: null,
        openDestination: null,
      } satisfies DocumentActionsSnapshot),
      drainTriggerCurrent,
    ],
    snapshot: snap,
  },
  {
    name: 'headless-with-a-report',
    covers: 'headless with a buffered report opens the reported page instead of the first.',
    plugins: current.plugins({ openSequence: 'headless' }),
    engine: current.engineWith({
      services: ['actions'],
      pages: threePages,
      responses: [current.catalog({}, { defer: 'catalog' })],
    }),
    steps: [
      ...boot,
      reg('named'),
      reportCurrent(3, [1, 3]),
      settle('catalog', { nameTreeScripts: [], openAction: null, openDestination: null }),
      drainTriggerCurrent,
    ],
    snapshot: snap,
  },
  {
    name: 'headless-with-no-pages',
    covers: 'headless on a document with no pages releases the barrier and opens nothing.',
    plugins: current.plugins({ openSequence: 'headless' }),
    engine: current.engineWith({ pages: [] }),
    steps: [...boot, reg('named'), drainTriggerCurrent],
    snapshot: snap,
  },
  {
    name: 'off-releases-the-barrier',
    covers: 'off never runs the sequence but a report emits straight away.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({
      services: ['actions'],
      pages: [current.page(1, 0, { open: current.tree(current.named('O-1')) })],
      responses: [current.catalog(openDocs)],
    }),
    steps: [...boot, reg('named'), reg('goto'), reportCurrent(1, [1])],
    snapshot: snap,
  },
  {
    name: 'a-replayed-open-trigger',
    covers:
      'A document-open trigger after the sequence fired reports open-sequence-replayed; before it fired, it runs the sequence.',
    plugins: current.plugins(),
    engine: current.engineWith({
      services: ['actions'],
      responses: [current.catalog({ openAction: current.tree(null, true) })],
    }),
    steps: [
      ...boot,
      listenCurrent(),
      dispatch(current.docEvent('open')),
      dispatch(current.docEvent('open')),
    ],
    snapshot: snap,
  },
  {
    name: 'open-sequence-read-fails',
    covers:
      'A failed catalog read inside the sequence is a trigger-failed diagnostic; the barrier still releases and a later report emits.',
    plugins: current.plugins({ openSequence: 'headless' }),
    engine: current.engineWith({
      services: ['actions'],
      pages: threePages,
      responses: [
        { path: 'actions.get', times: 1, defer: 'failed-open-catalog' },
        current.catalog(),
      ],
    }),
    steps: [
      ...boot,
      reg('named'),
      listenCurrent(),
      fail('failed-open-catalog', 'catalog gone'),
      drainTriggerCurrent,
      reportCurrent(3, [3]),
    ],
    snapshot: snap,
  },
  {
    name: 'no-actions-service',
    covers:
      'The current required actions service returns an empty catalog: no lifecycle trees run; a second open is a replay.',
    plugins: current.plugins({ openSequence: 'headless' }),
    engine: current.engineWith({ pages: [current.page(1, 0)] }),
    steps: [...boot, dispatch(current.docEvent('will-save')), dispatch(current.docEvent('open'))],
    snapshot: snap,
  },
  {
    name: 'coordinator-diffs',
    covers:
      'Unplaced reports are ignored; a placed report emits close(old), invisible, visible, open(new); a report with no change emits nothing; a current page of null closes without opening.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({
      pages: [
        current.page(1, 0, {
          close: current.tree(current.named('C-1')),
          open: current.tree(current.named('O-1')),
        }),
        current.page(2, 1, {
          open: current.tree(current.named('O-2')),
          close: current.tree(current.named('C-2')),
        }),
      ],
      responses: [
        current.annotations(1, [
          lifecycleAnnot(1, 41, {
            pageInvisible: current.tree(current.named('PI-1')),
            pageVisible: current.tree(current.named('PV-1')),
          }),
        ]),
        current.annotations(2, [
          lifecycleAnnot(2, 42, {
            pageVisible: current.tree(current.named('PV-2')),
            pageInvisible: current.tree(current.named('PI-2')),
          }),
        ]),
      ],
    }),
    steps: [
      ...boot,
      reg('named'),
      reportCurrent(1, [1], 'user', false),
      reportCurrent(1, [1]),
      reportCurrent(2, [2]),
      reportCurrent(2, [2]),
      reportCurrent(2, [1, 2]),
      reportCurrent(null, []),
    ],
    snapshot: snap,
  },
  {
    name: 'cascade-budget',
    covers:
      'Consecutive programmatic rounds past the cap are suppressed with a cascade-budget diagnostic (state still tracked); a user-caused report resets the count.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({
      pages: [
        current.page(1, 0, { open: current.tree(current.named('O-1')) }),
        current.page(2, 1, { open: current.tree(current.named('O-2')) }),
      ],
    }),
    steps: [
      ...boot,
      reg('named'),
      listenCurrent(),
      reportCurrent(1, []),
      ...Array.from({ length: 10 }, (_, round) =>
        reportCurrent(round % 2 === 0 ? 2 : 1, [], 'programmatic'),
      ),
      reportCurrent(2, [], 'user'),
      reportCurrent(1, [], 'programmatic'),
    ],
    snapshot: snap,
  },
  {
    name: 'cascade-budget-resets-on-user-activity',
    covers:
      'A user-origin dispatch resets the cascade count, and so does a user report while the barrier is closed.',
    plugins: current.plugins(),
    engine: current.engineWith({
      pages: [
        current.page(1, 0, { open: current.tree(current.named('O-1')) }),
        current.page(2, 1, { open: current.tree(current.named('O-2')) }),
      ],
      responses: [
        current.annotations(1, [
          lifecycleAnnot(1, 5, { focus: current.tree(current.named('focus')) }),
        ]),
      ],
    }),
    steps: [
      ...boot,
      reg('named'),
      listenCurrent(),
      reportCurrent(1, [], 'user'),
      cap('setUiAdapter', adapter()),
      ...Array.from({ length: 8 }, (_, round) =>
        reportCurrent(round % 2 === 0 ? 2 : 1, [], 'programmatic'),
      ),
      dispatch(annotationCurrent('focus', 1, current.aref(1, 5))),
      ...Array.from({ length: 2 }, (_, round) =>
        reportCurrent(round % 2 === 0 ? 2 : 1, [], 'programmatic'),
      ),
    ],
    snapshot: snap,
  },
];

// ── document lifecycle events ─────────────────────────────────────────────────

const allTrees = {
  willSave: current.tree(current.js('ws')),
  didSave: current.tree(current.js('ds')),
  willPrint: current.tree(current.js('wp')),
  didPrint: current.tree(current.js('dp')),
  willClose: current.tree(current.js('wc')),
};
const docEngine = (over: Partial<DocumentActionsSnapshot> = {}, extra: ResponseSpec[] = []) =>
  current.engineWith({ services: ['actions'], responses: [...extra, current.catalog(over)] });
const verb = (verbName: string, answer: AnswerSpec = { returns: 'done' }) =>
  cap('runDocumentVerb', verbName, fnSpec(`op.${verbName}`, answer));

const docEventCases: ScenarioCase[] = [
  {
    name: 'the-five-catalog-trees',
    covers:
      'Each verb-shaped document event resolves its Table-200 tree through dispatch; a missing tree is inert.',
    plugins: current.plugins(OFF),
    engine: docEngine(allTrees),
    steps: [
      ...boot,
      reg('javascript'),
      ...(
        [
          'will-save',
          'did-save',
          'will-print',
          'did-print',
          'will-close',
        ] satisfies DocumentTriggerEvent[]
      ).map((e) => dispatch(current.docEvent(e))),
    ],
    snapshot: snap,
  },
  {
    name: 'missing-trees-are-inert',
    covers: 'A catalog without the tree, or with a rootless one, is inert.',
    plugins: current.plugins(OFF),
    engine: docEngine({ willSave: current.tree(null) }),
    steps: [
      ...boot,
      dispatch(current.docEvent('will-save')),
      dispatch(current.docEvent('did-save')),
    ],
    snapshot: snap,
  },
  {
    name: 'open-runs-before-the-first-document-event',
    covers:
      'Under auto with the latch still armed, a first will-save runs the OpenAction inline first; the sequence then counts as fired.',
    plugins: current.plugins(),
    engine: docEngine({
      openAction: current.tree(current.js('open')),
      willSave: current.tree(current.js('ws')),
    }),
    steps: [
      ...boot,
      reg('javascript'),
      listenCurrent(),
      dispatch(current.docEvent('will-save')),
      dispatch(current.docEvent('open')),
      dispatch(current.docEvent('will-save')),
    ],
    snapshot: snap,
  },
  {
    name: 'a-rejected-catalog-read-is-evicted',
    covers:
      'A rejected catalog read degrades to trigger-failed and is evicted, so the next event reads again.',
    plugins: current.plugins(OFF),
    engine: current.engineWith({
      services: ['actions'],
      responses: [
        { path: 'actions.get', times: 1, error: { message: 'transient read failure' } },
        current.catalog({ willSave: current.tree(current.js('ws')) }),
      ],
    }),
    steps: [
      ...boot,
      reg('javascript'),
      listenCurrent(),
      dispatch(current.docEvent('will-save')),
      dispatch(current.docEvent('will-save')),
      dispatch(current.docEvent('did-save')),
    ],
    snapshot: snap,
  },
  {
    name: 'document-family-disabled',
    covers:
      'triggers.document false: runDocumentVerb skips both trees and still runs the operation; dispatch of a document event is trigger-disabled.',
    plugins: current.plugins({ openSequence: 'off', triggers: { document: false } }),
    engine: docEngine(allTrees),
    steps: [
      ...boot,
      reg('javascript'),
      verb('save', { returns: 'ok' }),
      dispatch(current.docEvent('will-save')),
      cap('prepareClose'),
    ],
    snapshot: snap,
  },
  {
    name: 'prepare-close',
    covers: 'prepareClose runs the will-close tree through the dispatch door.',
    plugins: current.plugins(OFF),
    engine: docEngine({ willClose: current.tree(current.js('wc')) }),
    steps: [...boot, reg('javascript'), cap('prepareClose')],
    snapshot: snap,
  },
  {
    name: 'run-document-verb-serializes',
    covers:
      'Two concurrent save verbs never interleave: the second waits for the first operation and its did-save.',
    plugins: current.plugins(OFF),
    engine: docEngine({
      willSave: current.tree(current.js('ws')),
      didSave: current.tree(current.js('ds')),
    }),
    steps: [
      ...boot,
      reg('javascript'),
      verb('save', { defer: 'op1' }),
      verb('save', { resolves: 'bytes-2' }),
      settle('op1', 'bytes-1'),
    ],
    snapshot: snap,
  },
  {
    name: 'run-document-verb-failure-laws',
    covers:
      'A failing will-save never cancels the operation; an operation that throws or rejects skips did-save and rethrows.',
    plugins: current.plugins(OFF),
    engine: docEngine({
      willSave: current.tree(current.js('ws')),
      didSave: current.tree(current.js('ds')),
    }),
    steps: [
      ...boot,
      reg('javascript', { seq: [{ returns: { status: 'failed', error: 'ws broke' } }, EXECUTED] }),
      listenCurrent(),
      verb('save', { returns: 42 }),
      verb('save', { throws: { message: 'save failed' } }),
      verb('save', { rejects: { message: 'save rejected' } }),
    ],
    snapshot: snap,
  },
  {
    name: 'run-document-verb-print-holds-the-latch',
    covers:
      'The print verb holds the print latch for its whole body: a Named Print inside will-print is suppressed as reentrant, and the latch is released after.',
    plugins: current.plugins({
      openSequence: 'off',
      policy: { print: { user: 'adapter', hover: 'block', lifecycle: 'adapter' } },
    }),
    engine: docEngine({
      willPrint: current.tree(current.js('wp', [current.named('Print')])),
      didPrint: current.tree(current.js('dp')),
    }),
    steps: [
      ...boot,
      reg('javascript'),
      listenCurrent(),
      cap('setUiAdapter', adapter()),
      verb('print', { returns: 'printed' }),
      verb('print', { throws: { message: 'printer on fire' } }),
      cap('execute', current.tree(current.named('Print')), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'print-verb-runs-wp-adapter-dp',
    covers:
      'A Named Print runs will-print, the adapter exactly once, then did-print; a Print nested in will-print is suppressed with reentrant-print.',
    plugins: current.plugins({
      openSequence: 'off',
      policy: { print: { user: 'adapter', hover: 'block', lifecycle: 'adapter' } },
    }),
    engine: docEngine({
      willPrint: current.tree(current.js('wp', [current.named('Print')])),
      didPrint: current.tree(current.js('dp')),
    }),
    steps: [
      ...boot,
      reg('javascript'),
      listenCurrent(),
      cap('setUiAdapter', adapter()),
      cap('execute', current.tree(current.named('Print')), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'an-adapter-throw-skips-did-print',
    covers:
      'An adapter print that throws rejects execute and skips did-print; the latch resets, so a later print works.',
    plugins: current.plugins(OFF),
    engine: docEngine({
      willPrint: current.tree(current.js('wp')),
      didPrint: current.tree(current.js('dp')),
    }),
    steps: [
      ...boot,
      reg('javascript'),
      cap('setUiAdapter', {
        openUri: fnSpec('ui.openUri'),
        print: fnSpec('ui.print', { throws: { message: 'dialog exploded' } }),
      }),
      cap('execute', current.tree(current.named('Print')), current.USER),
      cap('setUiAdapter', adapter()),
      cap('execute', current.tree(current.named('Print')), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'adapter-uninstalled-mid-print',
    covers:
      'An adapter cleared while will-print runs leaves the print no-executor, without did-print.',
    plugins: current.plugins(OFF),
    engine: docEngine({
      willPrint: current.tree(current.js('wp')),
      didPrint: current.tree(current.js('dp')),
    }),
    steps: [
      ...boot,
      reg('javascript', { defer: 'wp' }),
      cap('setUiAdapter', adapter()),
      cap('execute', current.tree(current.named('Print')), current.USER),
      callRef(1),
      settle('wp', { status: 'executed' }),
    ],
    snapshot: snap,
  },
  {
    name: 'an-open-uri-throw-rejects-execute',
    covers: 'An adapter openUri that throws rejects execute; the counter does not move.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [
      ...boot,
      cap('setUiAdapter', {
        openUri: fnSpec('ui.openUri', { throws: { message: 'popup blocked' } }),
        print: fnSpec('ui.print'),
      }),
      cap('execute', current.tree(current.uri('https://a.test/')), current.USER),
    ],
    snapshot: snap,
  },
];

// ── scripting ─────────────────────────────────────────────────────────────────

const realmCall = (method: string, document = 'b'): Step => ({
  op: 'call',
  target: { token: 'detached-realm', doc: document },
  method,
  args: [],
});
const JS_OFF = (over: Partial<current.CurrentJavaScript> = {}): current.CurrentConfig => ({
  openSequence: 'off',
  javascript: { enabled: true, sandbox: {}, ...over },
});
const scriptAnnots: Annotation[] = [
  scriptSquare(6, {
    nm: 'tip',
    contents: 'hello',
    author: 'Ada',
    subject: 'Note',
    color: '#ff0033',
    interiorColor: null,
    opacity: 0.5,
    strokeWidth: 2,
    borderStyle: 'dashed',
    dashArray: [3, 1],
    hidden: true,
    print: false,
    readOnly: true,
    locked: true,
    noView: false,
    toggleNoView: true,
  }),
  scriptLink(7),
  scriptWidget(8, 'text'),
  scriptStamp(9),
];
const scriptFields = [
  current.formField('name', 100, [41]),
  combobox(101, 'choice', '1', {
    widgets: [placed(42)],
    options: [{ label: 'One', value: '1', selected: true }],
    valueEntry: { kind: 'array', values: ['1'] },
  }),
  current.formField('empty', 102, [43], { valueEntry: { kind: 'none' } }),
];
const scriptEngine = (
  over: Partial<FakeDocumentSpec> = {},
  catalogOver: Partial<DocumentActionsSnapshot> = {},
) =>
  current.engineWith({
    services: ['actions', 'forms.submit'],
    security: {
      allows: '*',
      identity: { userId: 'u-1', displayName: 'Ada', groupId: 'g-1' } satisfies Identity,
    },
    ...over,
    responses: [
      ...(over.responses ?? []),
      current.forms(scriptFields),
      current.annotations(1, scriptAnnots),
      current.catalog({
        nameTreeScripts: [
          {
            name: 'lib',
            action: current.tree(current.js('function lib() {}', [current.js('var x = 1;')])),
          },
        ],
        ...catalogOver,
      }),
      { path: 'forms.submit', value: current.receipt('s') },
    ],
  });
const widgetCtx = (
  field: FormFieldRef,
  event: ActionContext['event'] = { scope: 'activate' },
): ActionContext => ({
  origin: 'user',
  source: { kind: 'widget', field, annotation: current.aref(1, 41), page: current.pr(1) },
  event,
});
const out = (over: Partial<ScriptOutput>) => ({ output: over });
const sinks = [
  cap('registerFormCommitSink', fnSpec('sink.form', current.FORM_OK())),
  cap('registerAnnotCommitSink', fnSpec('sink.annot', current.ANNOT_OK([6]))),
];
const formEffect: FormEffect = {
  kind: 'setValue',
  ref: { kind: 'objectNumber', objectNumber: 100 },
  value: { value: 'x' },
};
const annotEffect = (ref: AnnotationRef = current.aref(1, 6)): ScriptAnnotEffect => ({
  ref,
  patch: { strokeColor: ['RGB', 0.1, 0.2, 0.3], contents: 'Annot:Mouse Enter' },
});

const scriptCases: ScenarioCase[] = [
  {
    name: 'a-script-node-is-one-transaction',
    covers:
      'The javascript executor prefetches the page world (fields, script-addressable annots with their styles and defaults), boots the name tree once, runs the node with Field x Mouse Up and the target value, and commits form effects then annotation effects.',
    plugins: current.plugins(
      JS_OFF({
        identity: { email: 'ada@x.test' },
        fileName: fnSpec('fileName', { returns: 'report.pdf' }),
      }),
    ),
    engine: scriptEngine(),
    steps: [
      start,
      open('a', { name: 'Named.pdf' }),
      ...sinks,
      listen('onDiagnosticReported'),
      listen('onScriptDiagnosticReported'),
      cap(
        'execute',
        current.tree(current.js('event.value')),
        widgetCtx({ kind: 'objectNumber', objectNumber: 100 }),
      ),
    ],
    snapshot: snap,
  },
  {
    name: 'script-effects-commit',
    covers:
      'Form effects reach the form sink and annotation effects the annotation sink (object-number refs with their page, other refs as -1 without one).',
    plugins: current.plugins(
      JS_OFF({
        sandbox: {
          builds: [
            {
              run: [
                out({
                  formEffects: [formEffect],
                  annotEffects: [
                    annotEffect(),
                    annotEffect({ kind: 'baseIndex', page: current.pr(1), baseIndex: 0 }),
                  ],
                }),
              ],
            },
          ],
        },
      }),
    ),
    engine: scriptEngine({}, { nameTreeScripts: [] }),
    steps: [
      ...boot,
      ...sinks,
      listen('onDiagnosticReported'),
      cap('execute', current.tree(current.js('commit()')), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'script-commit-failures',
    covers:
      'No form sink drops form effects with no-commit-sink; a form failure skips the annotation effects; no annotation sink drops them; an annotation failure fails the node.',
    plugins: current.plugins(
      JS_OFF({
        sandbox: {
          builds: [{ run: [out({ formEffects: [formEffect], annotEffects: [annotEffect()] })] }],
        },
      }),
    ),
    engine: scriptEngine({}, { nameTreeScripts: [] }),
    steps: [
      ...boot,
      listen('onDiagnosticReported'),
      cap('execute', current.tree(current.js('a()')), current.USER),
      cap(
        'registerFormCommitSink',
        fnSpec('sink.form', {
          seq: [current.formFailure('value refused'), current.formFailure(), current.FORM_OK()],
        }),
      ),
      cap('execute', current.tree(current.js('b()')), current.USER),
      cap('execute', current.tree(current.js('c()')), current.USER),
      cap('execute', current.tree(current.js('d()')), current.USER),
      cap(
        'registerAnnotCommitSink',
        fnSpec('sink.annot', {
          seq: [
            { returns: { results: [{ annotObjectNumber: 6, status: 'failed', error: 'locked' }] } },
            { returns: { results: [{ annotObjectNumber: 6, status: 'failed' }] } },
          ],
        }),
      ),
      cap('execute', current.tree(current.js('e()')), current.USER),
      cap('execute', current.tree(current.js('f()')), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'boot-effects-and-a-refetched-world',
    covers:
      'A boot that stages effects commits them in the first transaction, surfaces its UI effects as phase boot, and the run sees a refetched world; later nodes do not boot again.',
    plugins: current.plugins(
      JS_OFF({
        sandbox: {
          builds: [
            {
              boot: [
                out({
                  formEffects: [formEffect],
                  uiEffects: [{ kind: 'alert', message: 'booted', icon: 3 }],
                  diagnostics: [{ code: 'unsupported-api', message: 'app.beep' }],
                  error: { kind: 'exception', message: 'boot threw', stack: 'at boot' },
                }),
              ],
            },
          ],
        },
      }),
    ),
    engine: scriptEngine(),
    steps: [
      ...boot,
      ...sinks,
      cap('setUiAdapter', adapter({ alert: fnSpec('ui.alert') })),
      listen('onScriptDiagnosticReported'),
      listen('onScriptFailed'),
      cap('execute', current.tree(current.js('a()', [current.js('b()')])), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'boot-without-effects',
    covers: 'A boot output with nothing to commit does not refetch the world.',
    plugins: current.plugins(
      JS_OFF({
        sandbox: {
          builds: [
            { boot: [out({ diagnostics: [{ code: 'script-error', message: 'boot warned' }] })] },
          ],
        },
      }),
    ),
    engine: scriptEngine(),
    steps: [
      ...boot,
      listen('onScriptDiagnosticReported'),
      cap('execute', current.tree(current.js('a()')), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'run-errors-and-diagnostics',
    covers:
      'A run error fails the node with its message, after its diagnostics and error reach the script hooks; its effects are not committed.',
    plugins: current.plugins(
      JS_OFF({
        sandbox: {
          builds: [
            {
              run: [
                out({
                  formEffects: [formEffect],
                  diagnostics: [{ code: 'blocked-network', message: 'fetch refused' }],
                  error: { kind: 'budget', message: 'interrupted: time budget' },
                }),
                out({ error: { kind: 'invalid-output', message: 'output too large' } }),
              ],
            },
          ],
        },
      }),
    ),
    engine: scriptEngine({}, { nameTreeScripts: [] }),
    steps: [
      ...boot,
      ...sinks,
      listen('onDiagnosticReported'),
      listen('onScriptDiagnosticReported'),
      listen('onScriptFailed'),
      cap('execute', current.tree(current.js('a()')), current.USER),
      cap('execute', current.tree(current.js('b()')), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'external-effects-run-after-the-transaction',
    covers:
      'print and submitForm UI effects from boot and run are held back and fired after the transaction (print through will-print/adapter/did-print, submit through the sink chain); alert and gotoPage surface inline.',
    plugins: current.plugins(
      JS_OFF({
        sandbox: {
          builds: [
            {
              boot: [out({ uiEffects: [{ kind: 'print' }] })],
              run: [
                out({
                  uiEffects: [
                    { kind: 'gotoPage', page: 2 },
                    {
                      kind: 'submitForm',
                      url: 'https://s.test/',
                      fieldNames: ['name', 'choice'],
                      includeEmpty: true,
                      format: 'xfdf',
                      method: 'get',
                    },
                    { kind: 'alert', message: 'hi', icon: 1, title: 'T' },
                    { kind: 'print' },
                  ],
                }),
              ],
            },
          ],
        },
      }),
    ),
    engine: scriptEngine(),
    steps: [
      ...boot,
      cap('setUiAdapter', adapter({ alert: fnSpec('ui.alert'), gotoPage: fnSpec('ui.gotoPage') })),
      cap('registerSubmitResolver', current.resolverEcho()),
      cap('setSubmitHandler', fnSpec('handler')),
      cap('execute', current.tree(current.js('a()')), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'script-print-obeys-the-print-policy',
    covers:
      "A script's print() is gated by the print policy row for the dispatch origin: under the defaults a lifecycle script's print is blocked before the adapter, and a user script's still prints.",
    plugins: current.plugins(
      JS_OFF({ sandbox: { builds: [{ run: [out({ uiEffects: [{ kind: 'print' }] })] }] } }),
    ),
    engine: scriptEngine({}, { nameTreeScripts: [] }),
    steps: [
      ...boot,
      listen('onDiagnosticReported'),
      cap('setUiAdapter', adapter()),
      cap('execute', current.tree(current.js('this.print()')), current.LIFECYCLE),
      cap('execute', current.tree(current.js('this.print()')), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'script-node-budget',
    covers:
      'maxScriptNodesPerDispatch caps JS nodes per dispatch: the rest are inert with the budget reason, and the next dispatch starts a fresh count.',
    plugins: current.plugins(JS_OFF({ maxScriptNodesPerDispatch: 2 })),
    engine: scriptEngine({}, { nameTreeScripts: [] }),
    steps: [
      ...boot,
      listen('onDiagnosticReported'),
      cap(
        'execute',
        current.tree(current.js('1', [current.js('2', [current.js('3', [current.js('4')])])])),
        current.USER,
      ),
      cap('execute', current.tree(current.js('5')), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'script-event-provenance',
    covers:
      'event.type and event.name from the trigger provenance: Field, Link, Annot, Page, Doc, and every annotation, page and document event name; a widget target by object number or name, with a value only for a scalar.',
    plugins: current.plugins(JS_OFF()),
    engine: scriptEngine({}, { nameTreeScripts: [] }),
    steps: [
      ...boot,
      ...(
        [
          widgetCtx({ kind: 'objectNumber', objectNumber: 101 }),
          widgetCtx({ kind: 'fqn', name: 'name' }, { scope: 'annotation', name: 'focus' }),
          widgetCtx({ kind: 'fqn', name: 'ghost' }),
          widgetCtx({ kind: 'objectNumber', objectNumber: 102 }),
          widgetCtx({ kind: 'fqn', name: 'empty' }),
          {
            origin: 'hover',
            source: { kind: 'link', annotation: current.aref(1, 7), page: current.pr(1) },
            event: { scope: 'annotation', name: 'cursorEnter' },
          },
          {
            origin: 'hover',
            source: { kind: 'link', annotation: current.aref(2, 7), page: current.pr(2) },
            event: { scope: 'annotation', name: 'cursorEnter' },
          },
          {
            origin: 'user',
            source: { kind: 'annotation', annotation: current.aref(2, 6), page: current.pr(2) },
            event: { scope: 'annotation', name: 'mouseDown' },
          },
          {
            origin: 'hover',
            source: { kind: 'annotation', annotation: current.aref(1, 6), page: current.pr(1) },
            event: { scope: 'annotation', name: 'cursorExit' },
          },
          {
            origin: 'user',
            source: { kind: 'annotation', annotation: current.aref(1, 6), page: current.pr(1) },
            event: { scope: 'annotation', name: 'mouseDown' },
          },
          {
            origin: 'user',
            source: { kind: 'annotation', annotation: current.aref(1, 6), page: current.pr(1) },
            event: { scope: 'annotation', name: 'mouseUp' },
          },
          {
            origin: 'user',
            source: { kind: 'annotation', annotation: current.aref(1, 6), page: current.pr(1) },
            event: { scope: 'annotation', name: 'blur' },
          },
          {
            origin: 'lifecycle',
            source: { kind: 'page', page: current.pr(2) },
            event: { scope: 'page', name: 'open' },
          },
          {
            origin: 'lifecycle',
            source: { kind: 'page', page: current.pr(2) },
            event: { scope: 'page', name: 'close' },
          },
          {
            origin: 'lifecycle',
            source: { kind: 'page', page: current.pr(2) },
            event: { scope: 'page', name: 'visible' },
          },
          {
            origin: 'lifecycle',
            source: { kind: 'page', page: current.pr(2) },
            event: { scope: 'page', name: 'invisible' },
          },
          {
            origin: 'lifecycle',
            source: { kind: 'document' },
            event: { scope: 'document', name: 'open' },
          },
          {
            origin: 'lifecycle',
            source: { kind: 'document' },
            event: { scope: 'document', name: 'will-save' },
          },
          {
            origin: 'lifecycle',
            source: { kind: 'document' },
            event: { scope: 'document', name: 'did-save' },
          },
          {
            origin: 'lifecycle',
            source: { kind: 'document' },
            event: { scope: 'document', name: 'will-print' },
          },
          {
            origin: 'lifecycle',
            source: { kind: 'document' },
            event: { scope: 'document', name: 'did-print' },
          },
          {
            origin: 'lifecycle',
            source: { kind: 'document' },
            event: { scope: 'document', name: 'will-close' },
          },
          { origin: 'user', source: { kind: 'link' }, event: { scope: 'activate' } },
          { origin: 'user', source: { kind: 'api' }, event: { scope: 'activate' } },
        ] satisfies ActionContext[]
      ).map((ctx) => cap('execute', current.tree(current.js('probe()')), ctx)),
    ],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'no-page-to-anchor',
    covers: 'A document with no pages has nothing to anchor the world on: the node is inert.',
    plugins: current.plugins(JS_OFF()),
    engine: scriptEngine({ pages: [] }),
    steps: [
      ...boot,
      listen('onDiagnosticReported'),
      cap('execute', current.tree(current.js('a()')), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'an-unknown-page-fails-the-node',
    covers:
      'A source page that is not in the document makes the world read throw; the transaction rejects and the node fails.',
    plugins: current.plugins(JS_OFF()),
    engine: scriptEngine(),
    steps: [
      ...boot,
      listen('onDiagnosticReported'),
      cap('execute', current.tree(current.js('a()')), {
        origin: 'lifecycle',
        source: { kind: 'page', page: current.pr(42) },
        event: { scope: 'page', name: 'open' },
      }),
    ],
    snapshot: snap,
  },
  {
    name: 'environment-inputs',
    covers:
      'Configured now, UTC offset, seed, file name and a function identity reach every input; the defaults are the clock, zero, the FNV seed and the open name.',
    plugins: current.plugins(
      JS_OFF({
        now: fnSpec('now', { returns: 1720000000000 }),
        utcOffsetMinutes: fnSpec('utcOffsetMinutes', { returns: 180 }),
        randomSeed: fnSpec('randomSeed', { returns: 7 }),
        identity: fnSpec('identity', { returns: { name: 'Fn User', corporation: 'Fn Inc' } }),
      }),
    ),
    engine: scriptEngine(),
    steps: [...boot, cap('execute', current.tree(current.js('a()')), current.USER)],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'environment-defaults',
    covers:
      'With nothing configured: Date.now() from the clock, a zero offset, seedFrom(documentId, sequence), the name the document was opened with, and the engine identity.',
    plugins: current.plugins(JS_OFF()),
    engine: scriptEngine(),
    steps: [
      start,
      open('a', { name: 'Opened.pdf' }),
      { op: 'advance', ms: 2500 },
      cap('execute', current.tree(current.js('a()', [current.js('b()')])), current.USER),
    ],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'environment-file-name-fallback',
    covers: 'A document opened without a name is document.pdf to scripts.',
    plugins: current.plugins(JS_OFF()),
    engine: scriptEngine(),
    steps: [...boot, cap('execute', current.tree(current.js('a()')), current.USER)],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'a-fault-rebuilds-the-realm',
    covers: 'A sandbox that faults during a run is rebuilt and booted again for the next node.',
    plugins: current.plugins(JS_OFF({ sandbox: { builds: [{ run: [{ dispose: true }] }, {}] } })),
    engine: scriptEngine(),
    steps: [
      ...boot,
      cap('execute', current.tree(current.js('a()')), current.USER),
      cap('execute', current.tree(current.js('b()')), current.USER),
    ],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'a-throwing-sandbox-fails-the-node',
    covers: 'A sandbox run that throws rejects the transaction; the node fails with the message.',
    plugins: current.plugins(
      JS_OFF({ sandbox: { builds: [{ run: [{ throws: { message: 'vm exploded' } }] }] } }),
    ),
    engine: scriptEngine({}, { nameTreeScripts: [] }),
    steps: [
      ...boot,
      listen('onDiagnosticReported'),
      cap('execute', current.tree(current.js('a()')), current.USER),
    ],
    snapshot: snap,
  },
  {
    name: 'budget-reaches-the-sandbox',
    covers: 'A configured budget is the host default for every boot and run.',
    plugins: current.plugins(
      JS_OFF({
        budget: {
          maxExecutionMs: 5,
          maxMemoryBytes: 1024,
          maxStackBytes: 256,
          maxEffects: 2,
          maxOutputBytes: 64,
        },
      }),
    ),
    engine: scriptEngine(),
    steps: [...boot, cap('execute', current.tree(current.js('a()')), current.USER)],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'no-sandbox-host',
    covers:
      'Enabled scripting without a factory override exposes the default realm and actual settings. Close disposes it and refuses a new transaction; default-VM execution requires a host loader outside this runner.',
    plugins: [
      ...current.plugins({ openSequence: 'off', javascript: { enabled: true, sandbox: null } }),
      { factory: 'detached-realm' },
    ],
    engine: scriptEngine(),
    steps: [
      ...boot,
      cap('isScriptingEnabled'),
      cap('getSettings'),
      {
        op: 'call',
        target: 'kernel',
        method: 'capability',
        args: [{ $token: 'detached-realm' }, 'a'],
      },
      close(),
      callRef(0),
    ],
    snapshot: snap,
  },
  {
    name: 'script-transaction-port',
    covers:
      'With scripting on, the host lens has scriptRealm.transaction: a body shares the realm queue with the executor and resolves with the body result.',
    plugins: [
      ...current.plugins(JS_OFF({ sandbox: { builds: [{ run: [{}] }] } })),
      { factory: 'detached-realm' },
    ],
    engine: scriptEngine(),
    steps: [
      ...boot,
      { op: 'call', target: 'kernel', method: 'capability', args: [{ $token: 'actions' }, 'a'] },
      cap('execute', current.tree(current.js('a()')), current.USER),
      realmCall('transaction', 'a'),
      realmCall('transactionThrows', 'a'),
    ],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'closing-the-document-disposes-the-realm',
    covers: 'Closing the document disposes the script realm and releases the event subscription.',
    plugins: current.plugins(JS_OFF()),
    engine: scriptEngine(),
    steps: [...boot, cap('execute', current.tree(current.js('a()')), current.USER), close()],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'script-boot-without-a-catalog',
    covers:
      'Both required actions catalogs return an empty name-tree script list, so neither document boots a program.',
    plugins: current.plugins(JS_OFF()),
    engine: {
      documents: {
        a: current.doc({ responses: [current.forms(scriptFields)] }),
        b: current.doc({
          services: ['actions'],
          responses: [current.forms(scriptFields), current.catalog()],
        }),
      },
    },
    steps: [
      start,
      open('a'),
      open('b'),
      cap('execute', current.tree(current.js('a()')), current.USER),
      {
        op: 'call',
        target: { token: 'actions', doc: 'b' },
        method: 'execute',
        args: [current.tree(current.js('b()')), current.USER],
      },
    ],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'detached-script-realm',
    covers:
      "createDetachedScriptRealm mints a realm for another document under this document's environment: a fresh sandbox from the same factory, the target's id and metadata, the owner's identity, a seed from the target id, the target's boot sources, and the configured budget. The caller owns it: the transaction port queues bodies, and dispose ends the realm.",
    plugins: [
      {
        factory: 'actions-current',
        config: JS_OFF({
          sandbox: {
            builds: [
              {},
              { run: [{ output: { uiEffects: [{ kind: 'alert', message: 'hi', icon: 0 }] } }] },
            ],
          },
          budget: {
            maxExecutionMs: 9,
            maxMemoryBytes: 1024,
            maxStackBytes: 256,
            maxEffects: 2,
            maxOutputBytes: 64,
          },
        }),
      },
      {
        factory: 'detached-realm',
        config: { bootSources: fnSpec('asset.boot', { resolves: ['var asset = 1;'] }) },
      },
    ],
    engine: { documents: { a: scriptEngine().documents.a, b: current.doc() } },
    steps: [
      start,
      open('a'),
      open('b', { name: 'Asset.pdf' }),
      cap('isScriptingEnabled'),
      cap('execute', current.tree(current.js('own()')), current.USER),
      { op: 'call', target: { token: 'detached-realm', doc: 'b' }, method: 'mint', args: [] },
      {
        op: 'call',
        target: { token: 'detached-realm', doc: 'b' },
        method: 'run',
        args: ['stamp()'],
      },
      realmCall('detachedTransaction'),
      realmCall('dispose'),
      {
        op: 'call',
        target: { token: 'detached-realm', doc: 'b' },
        method: 'run',
        args: ['again()'],
      },
    ],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'detached-script-realm-without-metadata',
    covers:
      'A target with no document metadata runs as document.pdf with zero pages; with scripting off there is nothing to mint.',
    plugins: [
      { factory: 'actions-current', config: JS_OFF() },
      { factory: 'detached-realm', config: { meta: null } },
    ],
    engine: { documents: { a: scriptEngine().documents.a, b: current.doc(), c: current.doc() } },
    steps: [
      start,
      open('a'),
      open('b'),
      { op: 'call', target: { token: 'detached-realm', doc: 'b' }, method: 'mint', args: [] },
      {
        op: 'call',
        target: { token: 'detached-realm', doc: 'b' },
        method: 'run',
        args: ['stamp()'],
      },
      close('b'),
      callRef(0, fnSpec('after-close', { resolves: 'unreachable' })),
    ],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'no-detached-realm-without-scripting',
    covers:
      'With scripting off the host lens has no createDetachedScriptRealm, so a sibling mints nothing.',
    plugins: [
      {
        factory: 'actions-current',
        config: { openSequence: 'off', javascript: { enabled: false } },
      },
      { factory: 'detached-realm' },
    ],
    engine: { documents: { a: current.doc(), b: current.doc() } },
    steps: [
      start,
      open('a'),
      open('b'),
      { op: 'call', target: { token: 'detached-realm', doc: 'b' }, method: 'mint', args: [] },
    ],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'scripting-disabled',
    covers: 'javascript.enabled false: no realm, javascript nodes inert.',
    plugins: current.plugins({ openSequence: 'off', javascript: { enabled: false } }),
    engine: scriptEngine(),
    steps: [
      ...boot,
      cap('execute', current.tree(current.js('a()')), current.USER),
      cap('isScriptingEnabled'),
    ],
    snapshot: snap,
  },
];

// ── the surface door ──────────────────────────────────────────────────────────

const surfaceResult = (result: Partial<ScriptSurfaceResult>): ScriptSurfaceResult => ({
  uiEffects: [],
  diagnostics: [],
  origin: 'user',
  phase: 'user',
  realm: 'document',
  ...result,
});
const surface = (result: Partial<ScriptSurfaceResult>) =>
  cap('surfaceScriptResult', surfaceResult(result));
const surfaceCommit = (
  result: ScriptCommitSurface,
  options: Parameters<ActionsHostCapability['surfaceScriptCommit']>[1],
): Step => cap('surfaceScriptCommit', result, options);
const surfaceCases: ScenarioCase[] = [
  {
    name: 'surface-ui-effects',
    covers:
      'alert (with and without a title), gotoPage and print reach the adapter with the origin and phase; an adapter without alert or gotoPage ignores them; diagnostics and the error reach the script hooks.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [
      ...boot,
      listen('onScriptDiagnosticReported'),
      listen('onScriptFailed'),
      cap('setUiAdapter', adapter({ alert: fnSpec('ui.alert'), gotoPage: fnSpec('ui.gotoPage') })),
      surface({
        uiEffects: [
          { kind: 'alert', message: 'm1', icon: 0 },
          { kind: 'alert', message: 'm2', icon: 2, title: 'T' },
          { kind: 'gotoPage', page: 3 },
          { kind: 'print' },
        ],
        diagnostics: [{ code: 'unsupported-api', message: 'x' }],
        error: { kind: 'exception', message: 'boom' },
        origin: 'hover',
        phase: 'boot',
      }),
      cap('setUiAdapter', adapter()),
      surface({
        uiEffects: [
          { kind: 'alert', message: 'm3', icon: 1 },
          { kind: 'gotoPage', page: 1 },
        ],
      }),
    ],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'surface-refusals',
    covers:
      'No adapter is a no-adapter diagnostic; a print without doc.print is withheld with ui-effect-suppressed; a print during a document print event is suppressed as reentrant.',
    plugins: current.plugins(OFF),
    engine: {
      documents: {
        a: current.doc({
          services: ['actions'],
          security: { allows: ['doc.render'] },
          responses: [current.catalog()],
        }),
        b: current.doc({ services: ['actions'], responses: [current.catalog()] }),
      },
    },
    steps: [
      start,
      open('a'),
      open('b'),
      listen('onDiagnosticReported'),
      listen('onScriptDiagnosticReported'),
      surface({ uiEffects: [{ kind: 'alert', message: 'm', icon: 0 }, { kind: 'print' }] }),
      {
        op: 'call',
        target: { token: 'actions', doc: 'b' },
        method: 'onScriptDiagnosticReported',
        args: [fnSpec('b.onScriptDiagnosticReported')],
      },
      {
        op: 'call',
        target: { token: 'actions', doc: 'b' },
        method: 'setUiAdapter',
        args: [adapter()],
      },
      {
        op: 'call',
        target: { token: 'actions', doc: 'b' },
        method: 'runDocumentVerb',
        args: ['print', fnSpec('op.print', { defer: 'printing' })],
      },
      {
        op: 'call',
        target: { token: 'actions', doc: 'b' },
        method: 'surfaceScriptResult',
        args: [surfaceResult({ uiEffects: [{ kind: 'print' }] })],
      },
      settle('printing', 'done'),
      {
        op: 'call',
        target: { token: 'actions', doc: 'b' },
        method: 'surfaceScriptResult',
        args: [surfaceResult({ uiEffects: [{ kind: 'print' }] })],
      },
    ],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'surface-submit',
    covers:
      'A scripted submitForm from the form pipeline is detached through the same submit door (api source, activate event), with format fdf and method post by default and include-mode names.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [
      ...boot,
      listen('onDiagnosticReported'),
      cap('registerSubmitResolver', current.resolverEcho()),
      cap('setSubmitHandler', fnSpec('handler')),
      surface({
        uiEffects: [
          { kind: 'submitForm', url: null, fieldNames: null, includeEmpty: false },
          {
            kind: 'submitForm',
            url: 'https://x.test',
            fieldNames: ['a'],
            includeEmpty: true,
            format: 'html',
            method: 'get',
          },
        ],
        origin: 'lifecycle',
      }),
      surface({
        uiEffects: [{ kind: 'submitForm', url: null, fieldNames: null, includeEmpty: false }],
        origin: 'user',
      }),
    ],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'surface-a-detached-realm',
    covers:
      'A detached realm has no document surface: its print, gotoPage and submitForm requests are suppressed with ui-effect-suppressed, and only alerts reach the adapter.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [
      ...boot,
      listen('onDiagnosticReported'),
      listen('onScriptDiagnosticReported'),
      cap('registerSubmitResolver', current.resolverEcho()),
      cap('setSubmitHandler', fnSpec('handler')),
      cap('setUiAdapter', adapter({ alert: fnSpec('ui.alert'), gotoPage: fnSpec('ui.gotoPage') })),
      surface({
        uiEffects: [
          { kind: 'print' },
          { kind: 'gotoPage', page: 2 },
          { kind: 'submitForm', url: null, fieldNames: null, includeEmpty: false },
          { kind: 'alert', message: 'from the asset', icon: 3 },
        ],
        realm: 'detached',
      }),
      surface({ uiEffects: [{ kind: 'gotoPage', page: 2 }], realm: 'document' }),
    ],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'surface-script-commit',
    covers:
      'surfaceScriptCommit splits a K/V/C/F commit into its boot-phase effects (only when there are any) and its user-phase effects, which carry the diagnostics and the error even with no user effects; the realm kind rides both.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [
      ...boot,
      listen('onScriptDiagnosticReported'),
      listen('onScriptFailed'),
      cap('setUiAdapter', adapter({ alert: fnSpec('ui.alert'), gotoPage: fnSpec('ui.gotoPage') })),
      surfaceCommit(
        {
          uiEffects: [
            { kind: 'alert', message: 'boot', icon: 0, phase: 'boot' },
            { kind: 'gotoPage', page: 1, phase: 'user' },
            { kind: 'alert', message: 'user', icon: 1, phase: 'user' },
          ],
          diagnostics: [{ code: 'unsupported-api', message: 'd' }],
          error: { kind: 'exception', message: 'e' },
        },
        { origin: 'user', realm: 'document' },
      ),
      surfaceCommit({ uiEffects: [], diagnostics: [] }, { origin: 'hover', realm: 'document' }),
      surfaceCommit(
        {
          uiEffects: [],
          diagnostics: [{ code: 'unsupported-api', message: 'no effects' }],
          error: { kind: 'exception', message: 'no effects' },
        },
        { origin: 'user', realm: 'document' },
      ),
      surfaceCommit(
        {
          uiEffects: [
            { kind: 'print', phase: 'boot' },
            { kind: 'alert', message: 'asset', icon: 0, phase: 'user' },
          ],
          diagnostics: [],
        },
        { origin: 'user', realm: 'detached' },
      ),
    ],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
  {
    name: 'script-hooks-isolate-observers',
    covers:
      'Throwing script diagnostic and failure observers are reported to the host sink without blocking sibling listeners.',
    plugins: current.plugins(OFF),
    engine: current.engineWith(),
    steps: [
      ...boot,
      cap('onScriptDiagnosticReported', fnSpec('bad.diag', { throws: { message: 'diag broke' } })),
      cap('onScriptFailed', fnSpec('bad.error', { throws: { message: 'error broke' } })),
      listen('onScriptDiagnosticReported', 'good.diag'),
      listen('onScriptFailed', 'good.error'),
      surface({
        diagnostics: [{ code: 'script-error', message: 'd' }],
        error: { kind: 'exception', message: 'e' },
      }),
    ],
    ...UNSNAPSHOTTED_ACTIONS_SCRIPT,
  },
];

// ── the hover pump ────────────────────────────────────────────────────────────

const pumpPlugins = (dispatchSpec: 'actions' | Scripted) =>
  dispatchSpec === 'actions'
    ? [
        { factory: 'actions-current', config: OFF },
        { factory: 'hover-pump', config: { dispatch: 'actions' } },
      ]
    : [{ factory: 'hover-pump', config: { dispatch: dispatchSpec } }];
const hover = (target: HoverTarget | null) =>
  ({
    op: 'call',
    target: { token: 'hover-pump', doc: 'a' },
    method: 'hover',
    args: [target],
  }) as Step;
const pumpReset: Step = {
  op: 'call',
  target: { token: 'hover-pump', doc: 'a' },
  method: 'reset',
  args: [],
};
const hoverTarget = (
  objectNumber: number,
  events?: HoverTarget['events'],
  ref?: AnnotationRef,
  pon = 1,
): HoverTarget => ({
  ref: ref ?? current.aref(pon, objectNumber),
  page: current.pr(pon),
  ...(events === undefined ? {} : { events }),
});
const scriptedDispatch = (labels: string[]) =>
  fnSpec('dispatch', { seq: labels.map((label) => ({ defer: label })) });
const done: ActionTriggerResult = { status: 'executed', steps: [], diagnostics: [] };

const rawHover = (): Step => ({
  op: 'call',
  target: { token: 'hover-pump', doc: 'a' },
  method: 'hover',
  args: [{ ref: { kind: 'mystery' }, page: current.pr(1) }],
});
const hoverCases: ScenarioCase[] = [
  {
    name: 'pairs-are-submitted-back-to-back',
    covers:
      'Enter(A); then A to B while idle submits Exit(A) and Enter(B) together, synchronously.',
    plugins: pumpPlugins(scriptedDispatch(['d1', 'd2', 'd3'])),
    engine: current.engineWith(),
    steps: [
      ...boot,
      hover(hoverTarget(1)),
      settle('d1', done),
      hover(hoverTarget(2)),
      settle('d2', done),
      settle('d3', done),
    ],
    ...UNSNAPSHOTTED_ACTIONS_HOVER,
  },
  {
    name: 'a-slow-transition-is-never-split',
    covers:
      'While a transition is in flight nothing else is submitted; after it settles the pump delivers the latest target.',
    plugins: pumpPlugins(scriptedDispatch(['d1', 'd2', 'd3'])),
    engine: current.engineWith(),
    steps: [
      ...boot,
      hover(hoverTarget(1)),
      hover(hoverTarget(2)),
      settle('d1', done),
      settle('d2', done),
      settle('d3', done),
    ],
    ...UNSNAPSHOTTED_ACTIONS_HOVER,
  },
  {
    name: 'intermediate-targets-are-skipped',
    covers: 'A to B to C to nothing to D while A is in flight delivers Exit(A) then Enter(D).',
    plugins: pumpPlugins(scriptedDispatch(['d1', 'd2', 'd3'])),
    engine: current.engineWith(),
    steps: [
      ...boot,
      hover(hoverTarget(1)),
      hover(hoverTarget(2)),
      hover(hoverTarget(3)),
      hover(null),
      hover(hoverTarget(4)),
      settle('d1', done),
      settle('d2', done),
      settle('d3', done),
    ],
    ...UNSNAPSHOTTED_ACTIONS_HOVER,
  },
  {
    name: 'same-target-is-free-and-exit-happens-once',
    covers:
      'Hovering the same target submits nothing; leaving to nothing exits once, and nothing again stays silent.',
    plugins: pumpPlugins(scriptedDispatch(['d1', 'd2'])),
    engine: current.engineWith(),
    steps: [
      ...boot,
      hover(hoverTarget(1)),
      settle('d1', done),
      hover(hoverTarget(1)),
      hover(null),
      settle('d2', done),
      hover(null),
    ],
    ...UNSNAPSHOTTED_ACTIONS_HOVER,
  },
  {
    name: 'event-flags-skip-tree-less-halves',
    covers:
      'events.enter false and events.exit false skip their half; a transition with nothing to submit settles on the next turn.',
    plugins: pumpPlugins(scriptedDispatch(['d1', 'd2'])),
    engine: current.engineWith(),
    steps: [
      ...boot,
      hover(hoverTarget(1, { enter: false })),
      hover(hoverTarget(2, { exit: false })),
      settle('d1', done),
      settle('d2', done),
      hover(null),
      hover(hoverTarget(3, { enter: false, exit: false })),
    ],
    ...UNSNAPSHOTTED_ACTIONS_HOVER,
  },
  {
    name: 'reset-forgets-without-dispatching',
    covers: 'reset drops the delivered target, so the next hover enters without an exit.',
    plugins: pumpPlugins(scriptedDispatch(['d1', 'd2'])),
    engine: current.engineWith(),
    steps: [
      ...boot,
      hover(hoverTarget(1)),
      settle('d1', done),
      pumpReset,
      hover(hoverTarget(2)),
      settle('d2', done),
    ],
    ...UNSNAPSHOTTED_ACTIONS_HOVER,
  },
  {
    name: 'target-identity',
    covers:
      'Targets compare by page and current annotationKey: object numbers and stable base indices; repeated positions stay silent and different pages produce a new target.',
    plugins: pumpPlugins(fnSpec('dispatch', { resolves: done })),
    engine: current.engineWith(),
    steps: [
      ...boot,
      hover(hoverTarget(1, undefined, { kind: 'baseIndex', page: current.pr(1), baseIndex: 0 })),
      hover(hoverTarget(1, undefined, { kind: 'baseIndex', page: current.pr(1), baseIndex: 0 })),
      hover(hoverTarget(1, undefined, { kind: 'baseIndex', page: current.pr(1), baseIndex: 1 })),
      hover(hoverTarget(1, undefined, { kind: 'baseIndex', page: current.pr(1), baseIndex: 2 })),
      hover(hoverTarget(1, undefined, { kind: 'baseIndex', page: current.pr(1), baseIndex: 2 })),
      hover(hoverTarget(1, undefined, { kind: 'baseIndex', page: current.pr(1), baseIndex: 2 })),
      hover(hoverTarget(1, undefined, { kind: 'baseIndex', page: current.pr(1), baseIndex: 3 })),
      hover(hoverTarget(1, undefined, current.aref(1, 1))),
      hover(hoverTarget(1, undefined, current.aref(2, 1), 2)),
      hover(hoverTarget(1, undefined, current.aref(2, 1), 2)),
      hover(hoverTarget(1, undefined, current.aref(2, 2), 2)),
    ],
    ...UNSNAPSHOTTED_ACTIONS_HOVER,
  },
  {
    name: 'a-rejected-dispatch-still-releases',
    covers: 'allSettled: a rejected dispatch releases the pump like a resolved one.',
    plugins: pumpPlugins(
      fnSpec('dispatch', { seq: [{ rejects: { message: 'no' } }, { resolves: done }] }),
    ),
    engine: current.engineWith(),
    steps: [...boot, hover(hoverTarget(1)), hover(hoverTarget(2))],
    ...UNSNAPSHOTTED_ACTIONS_HOVER,
  },
  {
    name: 'an-unknown-ref-kind-has-no-key-and-matches-itself',
    covers:
      'An intentionally malformed ref has no annotationKey; two identical unknown keys compare equal, so repeated input does not dispatch again.',
    plugins: pumpPlugins(fnSpec('dispatch', { resolves: done })),
    engine: current.engineWith(),
    steps: [...boot, rawHover(), rawHover()],
    ...UNSNAPSHOTTED_ACTIONS_HOVER,
  },
  {
    name: 'drives-the-real-dispatcher',
    covers:
      'Over the actions capability: the pump forwards the source hint, and the /E and /X trees run as hover-origin steps.',
    plugins: pumpPlugins('actions'),
    engine: current.engineWith({
      responses: [
        current.annotations(1, [
          current.annot(1, 5, {
            cursorEnter: current.tree(current.named('show')),
            cursorExit: current.tree(current.named('hide')),
          }),
          current.annot(1, 6, { cursorEnter: current.tree(current.named('show-6')) }),
        ]),
      ],
    }),
    steps: [
      ...boot,
      reg('named'),
      listen('onExecuted'),
      hover({
        ...hoverTarget(5),
        source: { kind: 'link', annotation: current.aref(1, 5), page: current.pr(1) },
      }),
      hover(hoverTarget(6)),
      hover(null),
    ],
    snapshot: snap,
  },
];

// ── pure helpers ──────────────────────────────────────────────────────────────

const triggers: ActionTrigger[] = [
  current.activate(1),
  ...(['cursorEnter', 'cursorExit', 'mouseDown', 'mouseUp', 'focus', 'blur'] as const).map((event) => current.annotationEvent(event, 1)),
  ...(['open', 'close', 'visible', 'invisible'] as const).map((event) => current.pageEvent(event, 1)),
  ...(['open', 'will-save', 'did-save', 'will-print', 'did-print', 'will-close'] as const).map(current.docEvent),
];
const functionCases: FunctionCase[] = [
  ...triggers.map((trigger, i) => ({ name: `event-of-${i}`, fn: 'eventOf', args: [trigger] })),
  ...triggers.map((trigger, i) => ({ name: `origin-of-${i}`, fn: 'triggerOriginOf', args: [trigger] })),
  { name: 'url-encode-empty', fn: 'submitEntriesToUrlEncoded', args: [[]] },
  {
    name: 'url-encode-mixed',
    covers: 'A name-only entry, a scalar, a multi-select repeating its name, and an empty multi-select; UTF-8 percent encoding of everything outside the unreserved set.',
    fn: 'submitEntriesToUrlEncoded',
    args: [
      [
        { name: 'flag', value: null },
        { name: 'full name', value: 'Ada Lovelace & co=1' },
        { name: 'pick[]', value: ['a/b', 'ç', '😀'] },
        { name: 'none', value: [] },
        { name: "safe-_.!~*'()", value: 'ok' },
      ] satisfies FormSubmissionEntry[],
    ],
  },
];

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'plugins/actions.json',
    describes: 'packages/plugin/actions: eventOf, originOf and submitEntriesToUrlEncoded.',
    functions: { eventOf, triggerOriginOf, submitEntriesToUrlEncoded },
    cases: functionCases,
  },
  {
    kind: 'scenario',
    file: 'plugins/actions-dispatcher.json',
    describes: 'packages/plugin/actions: execute, policy, the tree walk, deferred effects, hide, executors, registration and hooks.',
    cases: dispatcherCases,
  },
  { kind: 'scenario', file: 'plugins/actions-submit.json', describes: 'packages/plugin/actions: the submit intent, resolver and sink chain.', cases: submitCases },
  { kind: 'scenario', file: 'plugins/actions-triggers.json', describes: 'packages/plugin/actions: trigger resolution, page fan-out and the lifecycle cache.', cases: triggerCases },
  {
    kind: 'scenario',
    file: 'plugins/actions-lifecycle.json',
    describes: 'packages/plugin/actions: the open sequence, the page-state coordinator, document events, verbs and print.',
    cases: [...sequenceCases, ...docEventCases],
  },
  {
    kind: 'scenario',
    file: 'plugins/actions-script.json',
    describes: 'packages/plugin/actions: the javascript executor over a scripted sandbox, and the script surface door.',
    cases: [...scriptCases, ...surfaceCases],
  },
  { kind: 'scenario', file: 'plugins/actions-hover.json', describes: 'packages/plugin/actions createHoverPump, scripted and over the real dispatcher.', cases: hoverCases },
];
