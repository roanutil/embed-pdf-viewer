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
 * `javascript.sandbox: null` is the configuration of a host that has no
 * sandbox: the TypeScript runs with scripting disabled, and the Rust port
 * runs with `enabled: true` and no factory, which must behave identically.
 */
import { definePlugin, type AnyPlugin, type DocumentMeta } from '@embedpdf/core';
import type { ScriptWorldInput } from '../../../../packages/core/acrojs/src/index';

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
} from '../../../../packages/plugin/actions/src/contract';
import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import { registerPlugin, type PluginEnv, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';
import { scriptedCallback, scriptedSandboxFactory, type AnswerSpec, type SandboxSpec } from './acrojs';

// ── the observer-error host input ─────────────────────────────────────────────

let observerEnv: PluginEnv | null = null;
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
    capability: (ctx) => {
      const dispatch =
        config.dispatch === 'actions'
          ? (trigger: ActionTrigger) => ctx.get(ActionsToken).dispatch(trigger)
          : (scriptedCallback(config.dispatch.$fn, config.dispatch, env) as (
              trigger: ActionTrigger,
            ) => Promise<ActionTriggerResult>);
      const pump = createHoverPump(dispatch);
      return { hover: (target: HoverTarget | null) => pump.hover(target), reset: () => pump.reset() };
    },
  });
}

registerPlugin('hover-pump', { factory: (config, env) => hoverPumpDriver(config as HoverPumpConfig, env) });

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
      capability: (ctx) => {
        let realm: DetachedScriptRealm | null = null;
        return {
          mint: () => {
            const mintRealm = ctx.tryForDocument(ActionsToken, owner)?.createDetachedScriptRealm;
            if (!mintRealm) return null;
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
            realm!.transaction(async (txn) => ({ boot: await txn.boot(EMPTY_WORLD), run: await txn.run(program, EMPTY_WORLD) })),
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

const dispatcherCases: ScenarioCase[] = [
  {
    name: 'incomplete-tree-is-refused',
    covers: 'Never execute an incomplete tree, not even its root; canExecute agrees.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [...boot, reg('javascript'), listen(), cap('execute', tree(js('boot()'), true), USER), cap('canExecute', tree(js('boot()'), true), USER)],
    snapshot: snap,
  },
  {
    name: 'rootless-tree-is-inert',
    covers: 'A tree with no root is inert and counted; canExecute is false.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [...boot, cap('execute', tree(null), USER), cap('canExecute', tree(null), USER)],
    snapshot: snap,
  },
  {
    name: 'walks-next-in-pdf-order',
    covers: 'Root then /Next depth-first, with the child-index path of every node.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [...boot, reg('javascript'), cap('execute', tree(js('a', [js('b', [js('c')]), js('d')])), USER)],
    snapshot: snap,
  },
  {
    name: 'executed-then-failed-is-partial',
    covers: 'An executed node beside a failed one folds to partial.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [...boot, reg('javascript', { seq: [EXECUTED, { returns: { status: 'failed', error: 'second broke' } }] }), cap('execute', tree(js('ok', [js('boom')])), USER)],
    snapshot: snap,
  },
  {
    name: 'defers-navigation-and-external-effects',
    covers: 'goto, then javascript, then uri in PDF order: document work runs inline and the navigation and URI thunks fire after it, in node order.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [...boot, reg('javascript'), reg('goto'), cap('setUiAdapter', adapter()), cap('execute', tree(goto(3, [js('x', [uri('https://a.test/', [], true)])])), USER)],
    snapshot: snap,
  },
  {
    name: 'a-document-failure-drops-deferred-work',
    covers: 'A failed document node skips later document nodes (javascript, reset-form, hide) and drops every deferred thunk; the result is partial.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [
      ...boot,
      reg('goto'),
      reg('javascript', { seq: [{ returns: { status: 'failed', error: 'exploded' } }, EXECUTED] }),
      reg('reset-form'),
      cap('setUiAdapter', adapter()),
      listen(),
      cap('execute', tree(goto(1, [js('boom', [js('after'), reset(null), hide([byNumber(7)]), uri('https://a.test/')])])), USER),
    ],
    snapshot: snap,
  },
  {
    name: 'policy-matrix-defaults',
    covers:
      'The default rows: a lifecycle uri reports, a hover submit blocks, a lifecycle Print blocks, launch and the other fixed types are never executable, recognized types without an interpreter and unknown types are no-executor; canExecute mirrors the root decision.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [
      ...boot,
      cap('setUiAdapter', adapter()),
      listen(),
      cap('execute', tree(uri('https://a.test/')), LIFECYCLE),
      cap('canExecute', tree(uri('https://a.test/')), LIFECYCLE),
      cap('canExecute', tree(uri('https://a.test/')), USER),
      cap('execute', tree(submit(payload())), HOVER),
      cap('execute', tree(named('Print')), LIFECYCLE),
      cap('canExecute', tree(named('Print')), USER),
      cap(
        'execute',
        tree(
          bare('launch', { filePath: 'x.exe' }, [
            bare('goto-remote', { filePath: 'r.pdf' }),
            bare('goto-embedded', { filePath: 'e.pdf' }),
            bare('sound'),
            bare('movie'),
            bare('import-data'),
            bare('rendition'),
            bare('thread'),
            bare('set-ocg-state'),
            bare('transition'),
            bare('goto-3d-view'),
            bare('unknown'),
            bare('bogus'),
          ]),
        ),
        USER,
      ),
      cap('canExecute', tree(bare('launch', { filePath: 'x.exe' })), USER),
      cap('canExecute', tree(bare('rendition')), USER),
      cap('canExecute', tree(goto(1)), HOVER),
    ],
    snapshot: snap,
  },
  {
    name: 'a-print-typed-node-uses-the-print-row',
    covers: 'A node whose type is literally "print" finds policy.print by key, and with no executor is no-executor.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [...boot, listen(), cap('execute', tree(bare('print')), USER), cap('canExecute', tree(bare('print')), USER)],
    snapshot: snap,
  },
  {
    name: 'policy-overrides',
    covers: 'config.policy rows replace the defaults: block and report refuse with their names, allow executes even for a lifecycle uri.',
    plugins: plugins({
      openSequence: 'off',
      policy: {
        uri: { user: 'block', hover: 'report', lifecycle: 'allow' },
        goto: { user: 'report', hover: 'allow', lifecycle: 'allow' },
      },
    }),
    engine: engineWith(),
    steps: [
      ...boot,
      cap('setUiAdapter', adapter()),
      reg('goto'),
      listen(),
      cap('execute', tree(uri('https://a.test/')), USER),
      cap('execute', tree(uri('https://a.test/')), HOVER),
      cap('execute', tree(uri('https://b.test/')), LIFECYCLE),
      cap('execute', tree(goto(1)), USER),
      cap('canExecute', tree(uri('https://b.test/')), LIFECYCLE),
    ],
    snapshot: snap,
  },
  {
    name: 'policy-rows-merge-and-update-live',
    covers:
      'config.policy merges row-wise over the defaults (a partial row keeps the other origins); getPolicy is the effective policy; updatePolicy merges rows over the current policy, skips an empty row, bumps policyRevision even for an empty patch, and later dispatches decide with it.',
    plugins: plugins({ openSequence: 'off', policy: { uri: { hover: 'allow' } } }),
    engine: engineWith(),
    steps: [
      ...boot,
      cap('setUiAdapter', adapter()),
      reg('goto'),
      listen(),
      cap('getPolicy'),
      cap('execute', tree(uri('https://a.test/')), HOVER),
      cap('updatePolicy', { uri: { user: 'block' }, goto: { lifecycle: 'block' }, hide: null }),
      cap('getPolicy'),
      cap('execute', tree(uri('https://a.test/')), USER),
      cap('execute', tree(goto(1)), LIFECYCLE),
      cap('canExecute', tree(goto(1)), LIFECYCLE),
      cap('updatePolicy', {}),
      cap('getPolicy'),
    ],
    snapshot: snap,
  },
  {
    name: 'execute-named',
    covers:
      'executeNamed runs one Named verb as a user-origin api action through the same queue; a partial context overrides the origin, source or event; Print goes through the adapter door.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [
      ...boot,
      reg('named'),
      listen(),
      listen('onExecuted'),
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
      'getActionTree reads the raw tree behind each source kind: an annotation (activate by default, or the named /AA event; the /A-shadows-U rule is not applied), a field by object number (not the first numbered field) or by name, a page (open by default, or close, visible or invisible), and the catalog (OpenAction by default, or the Table-200 tree for a verb). Anything absent is null, and a document without the actions service has no catalog trees.',
    plugins: plugins(OFF),
    engine: {
      documents: {
        a: doc({
          services: ['actions'],
          pages: [page(1, 0, { open: tree(named('O-1')), close: tree(named('C-1')), visible: tree(named('V-1')), invisible: tree(named('I-1')) }), page(2, 1)],
          responses: [
            annotations(1, [annot(1, 5, { activate: tree(named('A-5')), mouseUp: tree(named('U-5')), cursorEnter: tree(named('E-5')) })]),
            forms([formField('f', 100, [], { actions: { keystroke: tree(js('k()')) } }), formField('g', 101)]),
            catalog({ openAction: tree(named('OpenAction')), willSave: tree(named('WS')) }),
          ],
        }),
        b: doc(),
      },
    },
    steps: [
      start,
      open('a'),
      open('b'),
      cap('getActionTree', { kind: 'annotation', annotation: aref(1, 5), page: pr(1) }),
      cap('getActionTree', { kind: 'annotation', annotation: aref(1, 5), page: pr(1), event: 'cursorEnter' }),
      cap('getActionTree', { kind: 'annotation', annotation: aref(1, 5), page: pr(1), event: 'mouseUp' }),
      cap('getActionTree', { kind: 'annotation', annotation: aref(1, 5), page: pr(1), event: 'blur' }),
      cap('getActionTree', { kind: 'annotation', annotation: aref(1, 9), page: pr(1) }),
      cap('getActionTree', { kind: 'field', field: { kind: 'objectNumber', fieldObjectNumber: 100 }, event: 'keystroke' }),
      cap('getActionTree', { kind: 'field', field: { kind: 'fqn', name: 'f' }, event: 'keystroke' }),
      cap('getActionTree', { kind: 'field', field: { kind: 'fqn', name: 'g' }, event: 'keystroke' }),
      cap('getActionTree', { kind: 'field', field: { kind: 'objectNumber', fieldObjectNumber: 999 }, event: 'format' }),
      cap('getActionTree', { kind: 'field', field: { kind: 'objectNumber', fieldObjectNumber: 101 }, event: 'keystroke' }),
      cap('getActionTree', { kind: 'page', page: pr(1) }),
      cap('getActionTree', { kind: 'page', page: pr(1), event: 'close' }),
      cap('getActionTree', { kind: 'page', page: pr(1), event: 'visible' }),
      cap('getActionTree', { kind: 'page', page: pr(1), event: 'invisible' }),
      cap('getActionTree', { kind: 'page', page: pr(2) }),
      cap('getActionTree', { kind: 'page', page: pr(7) }),
      cap('getActionTree', { kind: 'document' }),
      cap('getActionTree', { kind: 'document', event: 'will-save' }),
      cap('getActionTree', { kind: 'document', event: 'did-print' }),
      { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'getActionTree', args: [{ kind: 'document' }] },
    ],
    snapshot: 'none',
  },
  {
    name: 'uri-without-an-adapter',
    covers: 'A user uri with no adapter installed is no-executor with a no-adapter diagnostic.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [...boot, listen(), cap('execute', tree(uri('https://a.test/')), USER)],
    snapshot: snap,
  },
  {
    name: 'named-print-goes-to-the-adapter',
    covers: 'The Named Print verb reaches the adapter with no context, never the named executor; other named verbs use the executor.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [...boot, reg('named'), cap('setUiAdapter', adapter()), cap('execute', tree(named('Print', [named('NextPage')])), USER)],
    snapshot: snap,
  },
  {
    name: 'print-refusals',
    covers: 'Print with no adapter is no-executor; print without doc.print authority is blocked before the adapter.',
    plugins: plugins(OFF),
    engine: { documents: { a: doc({ security: { allows: ['doc.render'] } }), b: doc() } },
    steps: [
      start,
      open('a'),
      open('b'),
      listen(),
      { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'onDiagnostic', args: [fnSpec('b.onDiagnostic')] },
      { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'execute', args: [tree(named('Print')), USER] },
      cap('setUiAdapter', adapter()),
      cap('execute', tree(named('Print')), USER),
    ],
    snapshot: 'none',
  },
  {
    name: 'hide-routes-by-plane',
    covers:
      'A hide name resolves to its field (form setDisplay), a widget object number to its owning field, and a plain object number to an annotation flag patch; the form plane commits first.',
    plugins: plugins(OFF),
    engine: engineWith({ responses: [forms([formField('note1', 100, [41, 42]), formField('other', 101, [43])])] }),
    steps: [
      ...boot,
      cap('registerFormCommitSink', fnSpec('sink.form', FORM_OK(2))),
      cap('registerAnnotCommitSink', fnSpec('sink.annot', ANNOT_OK([7]))),
      listen(),
      cap('execute', tree(hide([byName('note1'), byNumber(7), byNumber(43)], false)), USER),
      cap('execute', tree(hide([byNumber(42)], true)), USER),
    ],
    snapshot: snap,
  },
  {
    name: 'hide-and-later-document-work',
    covers: 'An executed hide leaves later document nodes running; a failed hide skips them.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [
      ...boot,
      reg('reset-form'),
      cap('registerAnnotCommitSink', fnSpec('sink.annot', { seq: [ANNOT_OK([7]), { returns: { results: [{ annotObjectNumber: 7, status: 'failed', error: 'refused' }] } }] })),
      cap('execute', tree(hide([byNumber(7)], true, [reset(null)])), USER),
      cap('execute', tree(hide([byNumber(7)], true, [reset(null)])), USER),
    ],
    snapshot: snap,
  },
  {
    name: 'hide-without-targets',
    covers: 'A hide with no targets reads nothing and executes.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [...boot, cap('execute', tree(hide([])), USER)],
    snapshot: snap,
  },
  {
    name: 'hide-reports-honestly',
    covers:
      'Missing sinks are no-executor, an unresolved name is a diagnostic (the node still executes), an annotation refusal fails the node only when every entry failed, and a failure without an error string executes.',
    plugins: plugins(OFF),
    engine: engineWith({ responses: [forms([formField('note1', 100, [41])])] }),
    steps: [
      ...boot,
      listen(),
      cap('execute', tree(hide([byNumber(7)])), USER),
      cap('execute', tree(hide([byName('note1')])), USER),
      cap('execute', tree(hide([byName('ghost')])), USER),
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
      cap('execute', tree(hide([byNumber(7)])), USER),
      cap('execute', tree(hide([byNumber(7), byNumber(8)])), USER),
      cap('execute', tree(hide([byNumber(7), byNumber(8)])), USER),
      cap('execute', tree(hide([byNumber(7)])), USER),
    ],
    snapshot: snap,
  },
  {
    name: 'hide-form-failures',
    covers: 'A form sink failure (with an error message, or a bare rejected status) fails the node and skips the annotation plane.',
    plugins: plugins(OFF),
    engine: engineWith({ responses: [forms([formField('note1', 100)])] }),
    steps: [
      ...boot,
      listen(),
      cap(
        'registerFormCommitSink',
        fnSpec('sink.form', {
          seq: [
            { returns: { results: [{ index: 0, status: 'failed', error: { message: 'form refused' } }], changedWidgets: [], meta: null } },
            { returns: { results: [{ index: 0, status: 'rejected' }], changedWidgets: [], meta: null } },
            FORM_OK(1),
          ],
        }),
      ),
      cap('registerAnnotCommitSink', fnSpec('sink.annot', ANNOT_OK([7]))),
      cap('execute', tree(hide([byName('note1'), byNumber(7)])), USER),
      cap('execute', tree(hide([byName('note1'), byNumber(7)])), USER),
      cap('execute', tree(hide([byName('note1')])), USER),
    ],
    snapshot: snap,
  },
  {
    name: 'hide-sink-throws',
    covers: 'A sink that throws synchronously inside hide is not caught by the dispatcher: execute rejects and nothing is counted.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [...boot, cap('registerAnnotCommitSink', fnSpec('sink.annot', { throws: { message: 'sink broke' } })), cap('execute', tree(hide([byNumber(7)])), USER), cap('execute', tree(hide([])), USER)],
    snapshot: snap,
  },
  {
    name: 'hide-forms-read-rejects',
    covers: 'A forms.list rejection inside hide rejects execute.',
    plugins: plugins(OFF),
    engine: engineWith({ responses: [{ path: 'forms.list', error: { name: 'EngineError', message: 'closed', code: 'DocClosed' } }] }),
    steps: [...boot, cap('execute', tree(hide([byName('x')])), USER)],
    snapshot: snap,
  },
  {
    name: 'document-executors',
    covers:
      'reset-form and javascript without an executor (javascript is inert, "scripting unavailable"); an executor that is inert, fails, throws, rejects, or answers later.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [
      ...boot,
      listen(),
      cap('execute', tree(reset(null)), USER),
      cap('execute', tree(js('orphan()')), USER),
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
      cap('execute', tree(reset([byName('a')], true)), USER),
      cap('execute', tree(reset([])), USER),
      cap('execute', tree(reset(null)), USER),
      cap('execute', tree(reset(null)), USER),
      cap('execute', tree(reset(null)), USER),
      settle('reset', { status: 'executed' }),
      cap('execute', tree(reset(null)), USER),
    ],
    snapshot: snap,
  },
  {
    name: 'navigation-executors',
    covers: 'Deferred goto/named: no executor, inert, throws, rejects, executed.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [
      ...boot,
      listen(),
      cap('execute', tree(goto(1, [named('NextPage')])), USER),
      reg('goto', {
        seq: [
          { returns: { status: 'inert', reason: 'no stage' } },
          { throws: { message: 'goto threw' } },
          { rejects: { message: 'goto rejected' } },
          EXECUTED,
        ],
      }),
      cap('execute', tree(goto(1)), USER),
      cap('execute', tree(goto(1)), USER),
      cap('execute', tree(goto(1)), USER),
      cap('execute', tree(goto(1)), USER),
    ],
    snapshot: snap,
  },
  {
    name: 'executors-are-last-wins-and-identity-safe',
    covers: 'A duplicate registration replaces the executor with a duplicate-executor diagnostic; a stale disposer leaves the successor; the current disposer removes it.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [
      ...boot,
      listen(),
      reg('javascript', EXECUTED, 'first'),
      reg('javascript', EXECUTED, 'second'),
      callRef(1),
      cap('execute', tree(js('x')), USER),
      callRef(2),
      cap('execute', tree(js('x')), USER),
    ],
    snapshot: snap,
  },
  {
    name: 'adapter-is-identity-safe',
    covers: 'A stale adapter disposer leaves the successor; the current one clears it; setUiAdapter(null) force-clears.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [
      ...boot,
      cap('setUiAdapter', { openUri: fnSpec('first.openUri'), print: fnSpec('first.print') }),
      cap('setUiAdapter', { openUri: fnSpec('second.openUri'), print: fnSpec('second.print') }),
      callRef(0),
      cap('execute', tree(uri('https://a.test/')), USER),
      callRef(1),
      cap('execute', tree(uri('https://a.test/')), USER),
      cap('setUiAdapter', adapter()),
      cap('setUiAdapter', null),
      cap('execute', tree(uri('https://a.test/')), USER),
      callRef(2),
      callRef(3),
    ],
    snapshot: snap,
  },
  {
    name: 'sinks-and-resolver-are-identity-safe',
    covers: 'Commit sinks, the submit resolver and the submit handler all install last-wins and clear only while current.',
    plugins: plugins(OFF),
    engine: engineWith({ services: ['forms.submit'], responses: [{ path: 'forms.submit', value: { submissionId: 's', receivedAt: 'now' } }] }),
    steps: [
      ...boot,
      listen(),
      cap('registerAnnotCommitSink', fnSpec('annot.first', ANNOT_OK([7]))),
      cap('registerAnnotCommitSink', fnSpec('annot.second', ANNOT_OK([7]))),
      callRef(1),
      cap('execute', tree(hide([byNumber(7)])), USER),
      callRef(2),
      cap('execute', tree(hide([byNumber(7)])), USER),
      cap('registerFormCommitSink', fnSpec('form.first', FORM_OK())),
      cap('registerFormCommitSink', fnSpec('form.second', FORM_OK())),
      callRef(3),
      callRef(4),
      cap('registerSubmitResolver', resolverEcho()),
      cap('registerSubmitResolver', resolverEcho()),
      callRef(5),
      cap('setSubmitHandler', fnSpec('handler.first')),
      cap('setSubmitHandler', fnSpec('handler.second')),
      callRef(7),
      cap('execute', tree(submit(payload())), USER),
      callRef(8),
      cap('execute', tree(submit(payload())), USER),
      callRef(6),
      cap('execute', tree(submit(payload())), USER),
    ],
    snapshot: 'none',
  },
  {
    name: 'dispatches-serialize',
    covers: 'One queue: a slow execute holds the next until it settles, and the counter advances once per dispatch.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [...boot, reg('javascript', { seq: [{ defer: 'slow' }, EXECUTED] }), cap('execute', tree(js('slow')), USER), cap('execute', tree(js('fast')), USER), settle('slow', { status: 'executed' })],
    snapshot: snap,
  },
  {
    name: 'hooks-deliver-and-isolate',
    covers:
      'onAction carries ctx, tree and result; a throwing observer is reported to the host sink and its siblings still run; an unsubscribed listener hears nothing.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [
      ...boot,
      cap('onExecuted', fnSpec('onAction.bad', { throws: { message: 'observer broke' } })),
      cap('onExecuted', fnSpec('onAction')),
      cap('onDiagnostic', fnSpec('onDiagnostic.bad', { throws: { message: 'diag observer broke' } })),
      listen(),
      cap('execute', tree(js('orphan()')), USER),
      callRef(1),
      callRef(3),
      cap('execute', tree(js('orphan()')), USER),
    ],
    snapshot: snap,
  },
  {
    name: 'a-user-execute-arms-the-open-sequence',
    covers: 'Under auto, a user-origin execute runs the open sequence ahead of itself; a hover execute does not arm it.',
    plugins: plugins(),
    engine: engineWith({ services: ['actions'], responses: [catalog({ openAction: tree(named('OpenAction')) })] }),
    steps: [...boot, reg('named'), cap('execute', tree(named('Hover')), HOVER), cap('execute', tree(named('Click')), USER), cap('execute', tree(named('Again')), USER)],
    snapshot: snap,
  },
];

// ── the submit chain ──────────────────────────────────────────────────────────

const submitEngine = (over: Partial<FakeDocumentSpec> = {}) => engineWith(over);
const submitCases: ScenarioCase[] = [
  {
    name: 'payload-less-submit-is-inert',
    covers: 'An older-runtime SubmitForm with no payload is recognized-inert with its own diagnostic.',
    plugins: plugins(OFF),
    engine: submitEngine(),
    steps: [...boot, listen(), cap('execute', tree(submit()), USER)],
    snapshot: snap,
  },
  {
    name: 'no-resolver',
    covers: 'Without a dataset resolver every submit blocks with no-submit-resolver.',
    plugins: plugins(OFF),
    engine: submitEngine(),
    steps: [...boot, listen(), cap('execute', tree(submit(payload())), USER)],
    snapshot: snap,
  },
  {
    name: 'resolver-failures',
    covers: 'A resolver that throws or rejects fails the node with the dataset-resolution diagnostic.',
    plugins: plugins(OFF),
    engine: submitEngine(),
    steps: [
      ...boot,
      listen(),
      cap('registerSubmitResolver', fnSpec('resolver', { seq: [{ throws: { message: 'no fields' } }, { rejects: { message: 'field plane gone' } }] })),
      cap('execute', tree(submit(payload())), USER),
      cap('execute', tree(submit(payload())), USER),
    ],
    snapshot: snap,
  },
  {
    name: 'no-sink',
    covers: 'With a resolver but no handler and no submit-capable home, the submit blocks with no-submit-sink.',
    plugins: plugins(OFF),
    engine: submitEngine(),
    steps: [...boot, listen(), cap('registerSubmitResolver', resolverEcho()), cap('execute', tree(submit(payload({ fields: [byName('a'), byNumber(9)], charSet: 'utf-8' }))), USER)],
    snapshot: snap,
  },
  {
    name: 'the-document-home',
    covers: 'The home is awaited with the request shape it expects (charSet only when present, the clock for clientTimeMs); a refusal fails the node.',
    plugins: plugins(OFF),
    engine: submitEngine({
      services: ['forms.submit'],
      responses: [
        { path: 'forms.submit', times: 1, value: { submissionId: 's-1', receivedAt: 'now' } },
        { path: 'forms.submit', error: { name: 'PermissionDenied', message: 'permission denied: doc.forms.submit', required: 'doc.forms.submit' } },
      ],
    }),
    steps: [
      ...boot,
      listen(),
      cap('registerSubmitResolver', fnSpec('resolver', { seq: [{ resolves: RESOLVED }, { resolves: { ...RESOLVED, charSet: 'latin1' } }] })),
      { op: 'advance', ms: 1500 },
      cap('execute', tree(submit(payload())), USER),
      cap('execute', tree(submit(payload())), USER),
    ],
    snapshot: snap,
  },
  {
    name: 'the-home-uses-the-configured-clock',
    covers: 'clientTimeMs comes from javascript.now when configured, even with scripting disabled.',
    plugins: plugins({ openSequence: 'off', javascript: { enabled: false, now: fnSpec('now', { returns: 1234 }) } }),
    engine: submitEngine({ services: ['forms.submit'], responses: [{ path: 'forms.submit', value: { submissionId: 's', receivedAt: 'now' } }] }),
    steps: [...boot, cap('registerSubmitResolver', resolverEcho()), cap('execute', tree(submit(payload())), USER)],
    snapshot: snap,
  },
  {
    name: 'a-handler-beats-the-home',
    covers: 'An installed handler receives the request and a home delegate (or null without a home); synchronous acceptance executes and the home never auto-fires.',
    plugins: plugins(OFF),
    engine: { documents: { a: doc({ services: ['forms.submit'] }), b: doc() } },
    steps: [
      start,
      open('a'),
      open('b'),
      cap('registerSubmitResolver', resolverEcho([], { charSet: 'utf-8' })),
      cap('setSubmitHandler', fnSpec('handler', { returns: { not: 'a promise' } })),
      cap('execute', tree(submit(payload())), USER),
      { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'registerSubmitResolver', args: [resolverEcho()] },
      { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'setSubmitHandler', args: [fnSpec('handler.b')] },
      { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'execute', args: [tree(submit(payload())), USER] },
    ],
    snapshot: 'none',
  },
  {
    name: 'handler-contract',
    covers: 'A synchronous throw fails the node; a returned promise is detached: resolving says nothing, rejecting emits a diagnostic later and never rewrites the node.',
    plugins: plugins(OFF),
    engine: submitEngine(),
    steps: [
      ...boot,
      listen(),
      cap('registerSubmitResolver', resolverEcho()),
      cap('setSubmitHandler', fnSpec('handler', { seq: [{ throws: { message: 'sync refuse' } }, { resolves: 'ok' }, { defer: 'network' }] })),
      cap('execute', tree(submit(payload())), USER),
      cap('execute', tree(submit(payload())), USER),
      cap('execute', tree(submit(payload())), USER),
      fail('network', 'late network error'),
    ],
    snapshot: snap,
  },
  {
    name: 'submit-policy',
    covers: 'A lifecycle submit is blocked by default; a policy row of allow or adapter admits it, and report or block refuse.',
    plugins: plugins({ openSequence: 'off', policy: { 'submit-form': { user: 'report', hover: 'allow', lifecycle: 'adapter' } } }),
    engine: submitEngine(),
    steps: [
      ...boot,
      listen(),
      cap('registerSubmitResolver', resolverEcho()),
      cap('setSubmitHandler', fnSpec('handler')),
      cap('execute', tree(submit(payload())), USER),
      cap('execute', tree(submit(payload())), HOVER),
      cap('execute', tree(submit(payload())), LIFECYCLE),
    ],
    snapshot: snap,
  },
  {
    name: 'submit-is-deferred-behind-document-work',
    covers: 'A submit before a failing document node never fires.',
    plugins: plugins(OFF),
    engine: submitEngine(),
    steps: [...boot, cap('registerSubmitResolver', resolverEcho()), cap('setSubmitHandler', fnSpec('handler')), reg('javascript', { returns: { status: 'failed', error: 'x' } }), cap('execute', tree(submit(payload(), [js('boom')])), USER)],
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

const fanPage5 = [
  annot(5, 11, { pageOpen: tree(named('PO-11')), pageClose: tree(named('PC-11')), pageVisible: tree(named('PV-11')), pageInvisible: tree(named('PI-11')) }),
  annot(5, 12, { pageOpen: tree(named('PO-12')) }),
  annot(5, 13),
  annot(5, 14, { pageOpen: tree(null), cursorEnter: tree(named('E-14')) }),
];
const fanEngine = engineWith({
  pages: [page(5, 0, { open: tree(named('pageO')), close: tree(named('pageC')) }), page(6, 1, { open: tree(null) })],
  responses: [annotations(5, fanPage5)],
});

const triggerCases: ScenarioCase[] = [
  {
    name: 'activate-resolves-the-annotation-tree',
    covers: 'An activate trigger finds its annotation by ref (object number, /NM on the same page, index on the same page) and runs /A; mismatched kinds, pages or indexes find nothing.',
    plugins: plugins(OFF),
    engine: engineWith({
      responses: [
        annotations(2, [annot(2, 0, { activate: tree(named('click-numberless')) }, { ref: { kind: 'objectNumber', page: pr(2) } })]),
        annotations(1, [
          annot(1, 9, { activate: tree(named('click-9')) }),
          annot(1, 10, { activate: tree(named('click-nm')) }, { ref: nmRef(1, 'tip') }),
          annot(1, 11, { activate: tree(named('click-index')) }, { ref: indexRef(1, 3) }),
        ]),
      ],
    }),
    steps: [
      ...boot,
      reg('named'),
      dispatch(activate(1, aref(1, 9))),
      dispatch(activate(1, aref(99, 9))),
      dispatch(activate(1, nmRef(1, 'tip'))),
      dispatch(activate(1, nmRef(2, 'tip'))),
      dispatch(activate(1, nmRef(1, 'other'))),
      dispatch(activate(1, indexRef(1, 3, 'other-revision'))),
      dispatch(activate(1, indexRef(2, 3))),
      dispatch(activate(1, indexRef(1, 4))),
      dispatch(activate(1, aref(1, 10))),
      dispatch(activate(1, { kind: 'objectNumber', page: pr(1), annotObjectNumber: 404 })),
      dispatch(activate(2, { kind: 'objectNumber', page: pr(2) })),
    ],
    snapshot: snap,
  },
  {
    name: 'annotation-events-resolve-their-aa-tree',
    covers: 'Each of the six /AA pointer events resolves its own tree; absence is silently inert; the hover pair is hover-origin and the rest are user-origin.',
    plugins: plugins(OFF),
    engine: engineWith({
      responses: [
        annotations(4, [
          annot(4, 7, {
            cursorEnter: tree(named('E')),
            cursorExit: tree(named('X')),
            mouseDown: tree(named('D')),
            mouseUp: tree(named('U')),
            focus: tree(named('Fo')),
            blur: tree(named('Bl')),
          }),
          annot(4, 8),
        ]),
      ],
      pages: [page(4, 0)],
    }),
    steps: [
      ...boot,
      reg('named'),
      listen('onExecuted'),
      ...['cursorEnter', 'cursorExit', 'mouseDown', 'mouseUp', 'focus', 'blur'].map((event) => dispatch(annotationEvent(event, 4, aref(4, 7)))),
      dispatch(annotationEvent('cursorEnter', 4, aref(4, 8))),
    ],
    snapshot: snap,
  },
  {
    name: 'a-source-hint-is-provenance-only',
    covers: 'A first-party source hint becomes the step source, and the origin is still derived from the trigger.',
    plugins: plugins(OFF),
    engine: engineWith({ responses: [annotations(1, [annot(1, 7, { cursorEnter: tree(named('e')) })])] }),
    steps: [...boot, reg('named'), listen('onExecuted'), dispatch(annotationEvent('cursorEnter', 1, aref(1, 7), { kind: 'link', annotation: aref(1, 7), page: pr(1) }))],
    snapshot: snap,
  },
  {
    name: 'a-precedes-aa-u',
    covers: '/A shadows the /AA U tree: a mouseUp on an annotation with /A is inert; without /A its U tree runs.',
    plugins: plugins(OFF),
    engine: engineWith({
      responses: [
        annotations(1, [annot(1, 1, { activate: tree(named('A-1')), mouseUp: tree(named('U-1')) }), annot(1, 2, { mouseUp: tree(named('U-2')) })]),
      ],
    }),
    steps: [...boot, reg('named'), dispatch(annotationEvent('mouseUp', 1, aref(1, 1))), dispatch(annotationEvent('mouseUp', 1, aref(1, 2)))],
    snapshot: snap,
  },
  {
    name: 'an-incomplete-trigger-tree-is-refused',
    covers: 'A rootless but incomplete tree still becomes a step, and the step is refused; the trigger folds to refused.',
    plugins: plugins(OFF),
    engine: engineWith({ responses: [annotations(1, [annot(1, 1, { activate: tree(null, true) })])] }),
    steps: [...boot, listen(), dispatch(activate(1, aref(1, 1)))],
    snapshot: snap,
  },
  {
    name: 'resolution-never-rejects',
    covers: 'A failed annotation read and an unknown page both come back refused with trigger-failed; dispatch never rejects.',
    plugins: plugins(OFF),
    engine: engineWith({ responses: [{ path: 'page.annotations.list', pon: 1, error: { name: 'EngineError', message: 'read exploded', code: 'Io' } }] }),
    steps: [...boot, listen(), dispatch(annotationEvent('cursorEnter', 1, aref(1, 1))), dispatch(activate(77, aref(77, 1))), dispatch(pageEvent('open', 77))],
    snapshot: snap,
  },
  {
    name: 'submission-order-is-execution-order',
    covers: 'Two dispatches run in call order even when the first read is slower.',
    plugins: plugins(OFF),
    engine: engineWith({
      pages: [page(1, 0, { close: tree(named('closeA')) }), page(2, 1, { open: tree(named('openB')) })],
      responses: [annotations(1, [], { defer: 'slow-list' })],
    }),
    steps: [...boot, reg('named'), dispatch(pageEvent('close', 1)), dispatch(pageEvent('open', 2)), settle('slow-list', { annotations: [] })],
    snapshot: snap,
  },
  {
    name: 'page-fan-out-order',
    covers: 'open runs page /O then the /PO set; close runs the /PC set then /C; visible and invisible fan only their sets; rootless trees and tree-less annotations are no steps.',
    plugins: plugins(OFF),
    engine: fanEngine,
    steps: [
      ...boot,
      reg('named'),
      listen('onExecuted'),
      dispatch(pageEvent('open', 5)),
      dispatch(pageEvent('close', 5)),
      dispatch(pageEvent('visible', 5)),
      dispatch(pageEvent('invisible', 5)),
      dispatch(pageEvent('open', 6)),
    ],
    snapshot: snap,
  },
  {
    name: 'a-failed-step-never-skips-its-siblings',
    covers: 'A /PC step that fails is partial; the page /C still runs, and the trigger folds to partial.',
    plugins: plugins(OFF),
    engine: engineWith({
      pages: [page(6, 0, { close: tree(named('pageC')) })],
      responses: [annotations(6, [annot(6, 21, { pageClose: tree(js('boom')) })])],
    }),
    steps: [...boot, reg('named'), reg('javascript', { returns: { status: 'failed', error: 'boom' } }), dispatch(pageEvent('close', 6))],
    snapshot: snap,
  },
  {
    name: 'single-tree-lifecycle-annotations',
    covers: 'An annotation bearing only one lifecycle tree (close, visible or invisible) is still in the lifecycle set.',
    plugins: plugins(OFF),
    engine: engineWith({
      pages: [page(6, 0)],
      responses: [
        annotations(6, [
          annot(6, 31, { pageClose: tree(named('PC-31')) }),
          annot(6, 32, { pageVisible: tree(named('PV-32')) }),
          annot(6, 33, { pageInvisible: tree(named('PI-33')) }),
        ]),
      ],
    }),
    steps: [...boot, reg('named'), dispatch(pageEvent('close', 6)), dispatch(pageEvent('visible', 6)), dispatch(pageEvent('invisible', 6))],
    snapshot: snap,
  },
  {
    name: 'an-all-inert-fan-out-is-inert',
    covers: 'Steps that all come back inert fold to inert.',
    plugins: plugins(OFF),
    engine: engineWith({ pages: [page(6, 0, { open: tree(js('orphan()')) })], responses: [annotations(6, [annot(6, 34, { pageOpen: tree(named('Print')) })])] }),
    steps: [...boot, dispatch(pageEvent('open', 6))],
    snapshot: snap,
  },
  {
    name: 'deferred-navigation-flushes-per-step',
    covers: 'A page /O goto fires inside its own step, before the /PO step runs.',
    plugins: plugins(OFF),
    engine: engineWith({
      pages: [page(7, 0, { open: tree(goto(2)) })],
      responses: [annotations(7, [annot(7, 31, { pageOpen: tree(named('PO')) })])],
    }),
    steps: [...boot, reg('named'), reg('goto'), dispatch(pageEvent('open', 7))],
    snapshot: snap,
  },
  {
    name: 'lifecycle-cache-and-invalidation',
    covers: 'Lifecycle trees are read once per page; any annotation.* event and stream.desynced clear the cache, other events do not; close unsubscribes.',
    plugins: plugins(OFF),
    engine: fanEngine,
    steps: [
      ...boot,
      reg('named'),
      dispatch(pageEvent('visible', 5)),
      dispatch(pageEvent('invisible', 5)),
      { op: 'emit', doc: 'a', event: { type: 'metadata.updated' } },
      dispatch(pageEvent('visible', 5)),
      { op: 'emit', doc: 'a', event: { type: 'annotation.updated' } },
      dispatch(pageEvent('visible', 5)),
      { op: 'emit', doc: 'a', event: { type: 'stream.desynced' } },
      dispatch(pageEvent('visible', 5)),
      close(),
    ],
    snapshot: 'none',
  },
  {
    name: 'trigger-gates',
    covers: 'Disabled families are inert with trigger-disabled and canDispatch false; activate is never gated.',
    plugins: plugins({ openSequence: 'off', triggers: { page: false, annotation: false, document: false } }),
    engine: engineWith({ pages: [page(1, 0, { open: tree(named('O')) })] }),
    steps: [
      ...boot,
      reg('named'),
      listen(),
      dispatch(pageEvent('open', 1)),
      dispatch(annotationEvent('cursorEnter', 1, aref(1, 1))),
      dispatch(docEvent('open')),
      cap('canDispatch', pageEvent('open', 1)),
      cap('canDispatch', annotationEvent('cursorEnter', 1, aref(1, 1))),
      cap('canDispatch', docEvent('will-save')),
      cap('canDispatch', activate(1, aref(1, 1))),
      dispatch(activate(1, aref(1, 1))),
    ],
    snapshot: snap,
  },
  {
    name: 'gates-default-open',
    covers: 'A triggers object with no families set leaves every family enabled.',
    plugins: plugins({ openSequence: 'off', triggers: {} }),
    engine: engineWith(),
    steps: [...boot, cap('canDispatch', pageEvent('open', 1)), cap('canDispatch', annotationEvent('focus', 1, aref(1, 1))), cap('canDispatch', docEvent('open'))],
    snapshot: 'none',
  },
];

// ── the open sequence, the barrier and the coordinator ─────────────────────────

const report = (currentPon: number | null, visiblePons: number[], cause = 'user', placed = true) =>
  cap('reportPageState', { currentPage: currentPon === null ? null : pr(currentPon), visiblePages: visiblePons.map(pr), placed, cause });
const drainTrigger = dispatch(annotationEvent('cursorEnter', 999, aref(999, 1)));
const openDocs = { openAction: tree(named('OpenAction')), openDestination: { kind: 'fit', page: pr(3) } };
const threePages = [page(1, 0, { open: tree(named('O-1')) }), page(3, 1, { open: tree(named('O-3')) })];

const sequenceCases: ScenarioCase[] = [
  {
    name: 'auto-fires-on-adapter-install',
    covers:
      'Pre-open reports are buffered and coalesced; an adapter install runs the openDestination goto, then OpenAction, then the page open the latest report asks for; another adapter never replays it.',
    plugins: plugins(),
    engine: engineWith({ services: ['actions'], pages: threePages, responses: [catalog(openDocs)] }),
    steps: [
      ...boot,
      reg('named'),
      reg('goto'),
      report(1, [1], 'programmatic'),
      report(3, [3], 'programmatic'),
      cap('setUiAdapter', adapter()),
      drainTrigger,
      cap('setUiAdapter', adapter()),
      drainTrigger,
    ],
    snapshot: snap,
  },
  {
    name: 'auto-fires-ahead-of-the-first-user-dispatch',
    covers: 'The first user-origin dispatch arms the sequence before taking its own queue slot.',
    plugins: plugins(),
    engine: engineWith({
      services: ['actions'],
      pages: [page(2, 0)],
      responses: [catalog({ openAction: tree(named('OpenAction')) }), annotations(2, [annot(2, 9, { activate: tree(named('click')) })])],
    }),
    steps: [...boot, reg('named'), dispatch(activate(2, aref(2, 9)))],
    snapshot: snap,
  },
  {
    name: 'open-sequence-completed-event',
    covers:
      'onOpenSequenceCompleted fires once with the folded sequence result after the document steps; a throwing listener is reported to the host sink and never stops its siblings.',
    plugins: plugins(),
    engine: engineWith({ services: ['actions'], pages: threePages, responses: [catalog(openDocs)] }),
    steps: [
      ...boot,
      reg('goto'),
      reg('named'),
      cap('onOpenSequenceCompleted', fnSpec('completed.bad', { throws: { message: 'listener broke' } })),
      listen('onOpenSequenceCompleted', 'completed'),
      cap('setUiAdapter', adapter()),
      drainTrigger,
      cap('setUiAdapter', adapter()),
      drainTrigger,
    ],
    snapshot: snap,
  },
  {
    name: 'auto-a-submit-handler-does-not-arm',
    covers: 'Installing a submit handler is not the latch release: nothing runs until an adapter arrives.',
    plugins: plugins(),
    engine: engineWith({ services: ['actions'], responses: [catalog({ openAction: tree(named('OpenAction')) })] }),
    steps: [...boot, reg('named'), cap('setSubmitHandler', fnSpec('handler')), cap('setUiAdapter', null), cap('setUiAdapter', adapter())],
    snapshot: snap,
  },
  {
    name: 'headless-falls-back-to-the-first-page',
    covers: 'headless fires at bring-up; with no stage report the initial page open is the first page.',
    plugins: plugins({ openSequence: 'headless' }),
    engine: engineWith({ services: ['actions'], pages: [page(8, 0, { open: tree(named('O-8')) })], responses: [catalog()] }),
    steps: [...boot, reg('named'), drainTrigger],
    snapshot: snap,
  },
  {
    name: 'headless-with-a-report',
    covers: 'headless with a buffered report opens the reported page instead of the first.',
    plugins: plugins({ openSequence: 'headless' }),
    engine: engineWith({ services: ['actions'], pages: threePages, responses: [catalog({}, { defer: 'catalog' })] }),
    steps: [...boot, reg('named'), report(3, [1, 3]), settle('catalog', { nameTreeScripts: [], openAction: null, openDestination: null }), drainTrigger],
    snapshot: snap,
  },
  {
    name: 'headless-with-no-pages',
    covers: 'headless on a document with no pages releases the barrier and opens nothing.',
    plugins: plugins({ openSequence: 'headless' }),
    engine: engineWith({ pages: [] }),
    steps: [...boot, reg('named'), drainTrigger],
    snapshot: snap,
  },
  {
    name: 'off-releases-the-barrier',
    covers: 'off never runs the sequence but a report emits straight away.',
    plugins: plugins(OFF),
    engine: engineWith({ services: ['actions'], pages: [page(1, 0, { open: tree(named('O-1')) })], responses: [catalog(openDocs)] }),
    steps: [...boot, reg('named'), reg('goto'), report(1, [1])],
    snapshot: snap,
  },
  {
    name: 'a-replayed-open-trigger',
    covers: 'A document-open trigger after the sequence fired reports open-sequence-replayed; before it fired, it runs the sequence.',
    plugins: plugins(),
    engine: engineWith({ services: ['actions'], responses: [catalog({ openAction: tree(null, true) })] }),
    steps: [...boot, listen(), dispatch(docEvent('open')), dispatch(docEvent('open'))],
    snapshot: snap,
  },
  {
    name: 'open-sequence-read-fails',
    covers: 'A failed catalog read inside the sequence is a trigger-failed diagnostic; the barrier still releases and a later report emits.',
    plugins: plugins({ openSequence: 'headless' }),
    engine: engineWith({
      services: ['actions'],
      pages: threePages,
      responses: [{ path: 'actions.read', times: 1, error: { name: 'EngineError', message: 'catalog gone', code: 'Io' } }, catalog()],
    }),
    steps: [...boot, reg('named'), listen(), drainTrigger, report(3, [3])],
    snapshot: snap,
  },
  {
    name: 'no-actions-service',
    covers: 'A document without the actions service reads as no catalog: the sequence has no steps.',
    plugins: plugins({ openSequence: 'headless' }),
    engine: engineWith({ pages: [page(1, 0)] }),
    steps: [...boot, dispatch(docEvent('will-save')), dispatch(docEvent('open'))],
    snapshot: snap,
  },
  {
    name: 'coordinator-diffs',
    covers:
      'Unplaced reports are ignored; a placed report emits close(old), invisible, visible, open(new); a report with no change emits nothing; a current page of null closes without opening.',
    plugins: plugins(OFF),
    engine: engineWith({
      pages: [page(1, 0, { close: tree(named('C-1')), open: tree(named('O-1')) }), page(2, 1, { open: tree(named('O-2')), close: tree(named('C-2')) })],
      responses: [
        annotations(1, [annot(1, 41, { pageInvisible: tree(named('PI-1')), pageVisible: tree(named('PV-1')) })]),
        annotations(2, [annot(2, 42, { pageVisible: tree(named('PV-2')), pageInvisible: tree(named('PI-2')) })]),
      ],
    }),
    steps: [
      ...boot,
      reg('named'),
      report(1, [1], 'user', false),
      report(1, [1]),
      report(2, [2]),
      report(2, [2]),
      report(2, [1, 2]),
      report(null, []),
    ],
    snapshot: snap,
  },
  {
    name: 'cascade-budget',
    covers: 'Consecutive programmatic rounds past the cap are suppressed with a cascade-budget diagnostic (state still tracked); a user-caused report resets the count.',
    plugins: plugins(OFF),
    engine: engineWith({ pages: [page(1, 0, { open: tree(named('O-1')) }), page(2, 1, { open: tree(named('O-2')) })] }),
    steps: [
      ...boot,
      reg('named'),
      listen(),
      report(1, []),
      ...Array.from({ length: 10 }, (_, round) => report(round % 2 === 0 ? 2 : 1, [], 'programmatic')),
      report(2, [], 'user'),
      report(1, [], 'programmatic'),
    ],
    snapshot: snap,
  },
  {
    name: 'cascade-budget-resets-on-user-activity',
    covers: 'A user-origin dispatch resets the cascade count, and so does a user report while the barrier is closed.',
    plugins: plugins(),
    engine: engineWith({ pages: [page(1, 0, { open: tree(named('O-1')) }), page(2, 1, { open: tree(named('O-2')) })], responses: [annotations(1, [annot(1, 5, { focus: tree(named('focus')) })])] }),
    steps: [
      ...boot,
      reg('named'),
      listen(),
      report(1, [], 'user'),
      cap('setUiAdapter', adapter()),
      ...Array.from({ length: 8 }, (_, round) => report(round % 2 === 0 ? 2 : 1, [], 'programmatic')),
      dispatch(annotationEvent('focus', 1, aref(1, 5))),
      ...Array.from({ length: 2 }, (_, round) => report(round % 2 === 0 ? 2 : 1, [], 'programmatic')),
    ],
    snapshot: snap,
  },
];

// ── document lifecycle events ─────────────────────────────────────────────────

const allTrees = {
  willSave: tree(js('ws')),
  didSave: tree(js('ds')),
  willPrint: tree(js('wp')),
  didPrint: tree(js('dp')),
  willClose: tree(js('wc')),
};
const docEngine = (over: Record<string, unknown> = {}, extra: ResponseSpec[] = []) =>
  engineWith({ services: ['actions'], responses: [...extra, catalog(over)] });
const verb = (verbName: string, answer: AnswerSpec = { returns: 'done' }) => cap('runDocumentVerb', verbName, fnSpec(`op.${verbName}`, answer));

const docEventCases: ScenarioCase[] = [
  {
    name: 'the-five-catalog-trees',
    covers: 'Each verb-shaped document event resolves its Table-200 tree through dispatch; a missing tree is inert.',
    plugins: plugins(OFF),
    engine: docEngine(allTrees),
    steps: [...boot, reg('javascript'), ...['will-save', 'did-save', 'will-print', 'did-print', 'will-close'].map((e) => dispatch(docEvent(e)))],
    snapshot: snap,
  },
  {
    name: 'missing-trees-are-inert',
    covers: 'A catalog without the tree, or with a rootless one, is inert.',
    plugins: plugins(OFF),
    engine: docEngine({ willSave: tree(null) }),
    steps: [...boot, dispatch(docEvent('will-save')), dispatch(docEvent('did-save'))],
    snapshot: snap,
  },
  {
    name: 'open-runs-before-the-first-document-event',
    covers: 'Under auto with the latch still armed, a first will-save runs the OpenAction inline first; the sequence then counts as fired.',
    plugins: plugins(),
    engine: docEngine({ openAction: tree(js('open')), willSave: tree(js('ws')) }),
    steps: [...boot, reg('javascript'), listen(), dispatch(docEvent('will-save')), dispatch(docEvent('open')), dispatch(docEvent('will-save'))],
    snapshot: snap,
  },
  {
    name: 'a-rejected-catalog-read-is-evicted',
    covers: 'A rejected catalog read degrades to trigger-failed and is evicted, so the next event reads again.',
    plugins: plugins(OFF),
    engine: engineWith({
      services: ['actions'],
      responses: [{ path: 'actions.read', times: 1, error: { message: 'transient read failure' } }, catalog({ willSave: tree(js('ws')) })],
    }),
    steps: [...boot, reg('javascript'), listen(), dispatch(docEvent('will-save')), dispatch(docEvent('will-save')), dispatch(docEvent('did-save'))],
    snapshot: snap,
  },
  {
    name: 'document-family-disabled',
    covers: 'triggers.document false: runDocumentVerb skips both trees and still runs the operation; dispatch of a document event is trigger-disabled.',
    plugins: plugins({ openSequence: 'off', triggers: { document: false } }),
    engine: docEngine(allTrees),
    steps: [...boot, reg('javascript'), verb('save', { returns: 'ok' }), dispatch(docEvent('will-save')), cap('prepareClose')],
    snapshot: snap,
  },
  {
    name: 'prepare-close',
    covers: 'prepareClose runs the will-close tree through the dispatch door.',
    plugins: plugins(OFF),
    engine: docEngine({ willClose: tree(js('wc')) }),
    steps: [...boot, reg('javascript'), cap('prepareClose')],
    snapshot: snap,
  },
  {
    name: 'run-document-verb-serializes',
    covers: 'Two concurrent save verbs never interleave: the second waits for the first operation and its did-save.',
    plugins: plugins(OFF),
    engine: docEngine({ willSave: tree(js('ws')), didSave: tree(js('ds')) }),
    steps: [...boot, reg('javascript'), verb('save', { seq: [{ defer: 'op1' }, { resolves: 'bytes-2' }] }), verb('save', { seq: [{ defer: 'op1' }, { resolves: 'bytes-2' }] }), settle('op1', 'bytes-1')],
    snapshot: snap,
  },
  {
    name: 'run-document-verb-failure-laws',
    covers: 'A failing will-save never cancels the operation; an operation that throws or rejects skips did-save and rethrows.',
    plugins: plugins(OFF),
    engine: docEngine({ willSave: tree(js('ws')), didSave: tree(js('ds')) }),
    steps: [
      ...boot,
      reg('javascript', { seq: [{ returns: { status: 'failed', error: 'ws broke' } }, EXECUTED] }),
      listen(),
      verb('save', { returns: 42 }),
      verb('save', { throws: { message: 'save failed' } }),
      verb('save', { rejects: { message: 'save rejected' } }),
    ],
    snapshot: snap,
  },
  {
    name: 'run-document-verb-print-holds-the-latch',
    covers: 'The print verb holds the print latch for its whole body: a Named Print inside will-print is suppressed as reentrant, and the latch is released after.',
    plugins: plugins({ openSequence: 'off', policy: { print: { user: 'adapter', hover: 'block', lifecycle: 'adapter' } } }),
    engine: docEngine({ willPrint: tree(js('wp', [named('Print')])), didPrint: tree(js('dp')) }),
    steps: [...boot, reg('javascript'), listen(), cap('setUiAdapter', adapter()), verb('print', { returns: 'printed' }), verb('print', { throws: { message: 'printer on fire' } }), cap('execute', tree(named('Print')), USER)],
    snapshot: snap,
  },
  {
    name: 'print-verb-runs-wp-adapter-dp',
    covers: 'A Named Print runs will-print, the adapter exactly once, then did-print; a Print nested in will-print is suppressed with reentrant-print.',
    plugins: plugins({ openSequence: 'off', policy: { print: { user: 'adapter', hover: 'block', lifecycle: 'adapter' } } }),
    engine: docEngine({ willPrint: tree(js('wp', [named('Print')])), didPrint: tree(js('dp')) }),
    steps: [...boot, reg('javascript'), listen(), cap('setUiAdapter', adapter()), cap('execute', tree(named('Print')), USER)],
    snapshot: snap,
  },
  {
    name: 'an-adapter-throw-skips-did-print',
    covers: 'An adapter print that throws rejects execute and skips did-print; the latch resets, so a later print works.',
    plugins: plugins(OFF),
    engine: docEngine({ willPrint: tree(js('wp')), didPrint: tree(js('dp')) }),
    steps: [
      ...boot,
      reg('javascript'),
      cap('setUiAdapter', { openUri: fnSpec('ui.openUri'), print: fnSpec('ui.print', { throws: { message: 'dialog exploded' } }) }),
      cap('execute', tree(named('Print')), USER),
      cap('setUiAdapter', adapter()),
      cap('execute', tree(named('Print')), USER),
    ],
    snapshot: snap,
  },
  {
    name: 'adapter-uninstalled-mid-print',
    covers: 'An adapter cleared while will-print runs leaves the print no-executor, without did-print.',
    plugins: plugins(OFF),
    engine: docEngine({ willPrint: tree(js('wp')), didPrint: tree(js('dp')) }),
    steps: [...boot, reg('javascript', { defer: 'wp' }), cap('setUiAdapter', adapter()), cap('execute', tree(named('Print')), USER), callRef(1), settle('wp', { status: 'executed' })],
    snapshot: snap,
  },
  {
    name: 'an-open-uri-throw-rejects-execute',
    covers: 'An adapter openUri that throws rejects execute; the counter does not move.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [...boot, cap('setUiAdapter', { openUri: fnSpec('ui.openUri', { throws: { message: 'popup blocked' } }), print: fnSpec('ui.print') }), cap('execute', tree(uri('https://a.test/')), USER)],
    snapshot: snap,
  },
];

// ── scripting ─────────────────────────────────────────────────────────────────

const JS_OFF = (over: Partial<HarnessJavaScript> = {}): HarnessConfig => ({
  openSequence: 'off',
  javascript: { enabled: true, sandbox: {}, ...over },
});
const scriptAnnots = [
  annot(1, 6, undefined, {
    nm: 'tip',
    contents: 'hello',
    author: 'Ada',
    subject: 'Note',
    color: { r: 255, g: 0, b: 51 },
    interiorColor: null,
    opacity: 0.5,
    strokeWidth: 2,
    borderStyle: 'dashed',
    dashArray: [3, 1],
    flags: { hidden: true, print: false, readOnly: true, locked: true, noView: false, toggleNoView: true },
  }),
  annot(1, 7, undefined, { subtype: 'link' }),
  annot(1, 8, undefined, { subtype: 'widget-text' }),
  annot(1, 9, undefined, { subtype: 'stamp', interiorColor: { r: 0, g: 128, b: 255 } }),
];
const scriptFields = [
  formField('name', 100, [41]),
  formField('choice', 101, [42], { family: 'combobox', options: [{ label: 'One', value: '1' }], valueEntry: { kind: 'array', values: ['1'] } }),
  formField('empty', 102, [43], { valueEntry: { kind: 'none' } }),
];
const scriptEngine = (over: Partial<FakeDocumentSpec> = {}, catalogOver: Record<string, unknown> = {}) =>
  engineWith({
    services: ['actions', 'forms.submit'],
    security: { allows: '*', identity: { user_id: 'u-1', display_name: 'Ada', group_id: 'g-1' } },
    ...over,
    responses: [
      ...(over.responses ?? []),
      forms(scriptFields),
      annotations(1, scriptAnnots),
      catalog({ nameTreeScripts: [{ name: 'lib', action: tree(js('function lib() {}', [js('var x = 1;')])) }], ...catalogOver }),
      { path: 'forms.submit', value: { submissionId: 's', receivedAt: 'now' } },
    ],
  });
const widgetCtx = (field: unknown, event: unknown = { scope: 'activate' }) => ({
  origin: 'user',
  source: { kind: 'widget', field, annotation: aref(1, 41), page: pr(1) },
  event,
});
const out = (over: Record<string, unknown>) => ({ output: over });
const sinks = [
  cap('registerFormCommitSink', fnSpec('sink.form', FORM_OK(1))),
  cap('registerAnnotCommitSink', fnSpec('sink.annot', ANNOT_OK([6]))),
];
const formEffect = { kind: 'setValue', ref: { kind: 'objectNumber', fieldObjectNumber: 100 }, value: 'x' };
const annotEffect = (ref: unknown = aref(1, 6)) => ({ ref, patch: { strokeColor: ['RGB', 0.1, 0.2, 0.3], contents: 'Annot:Mouse Enter' } });

const scriptCases: ScenarioCase[] = [
  {
    name: 'a-script-node-is-one-transaction',
    covers:
      'The javascript executor prefetches the page world (fields, script-addressable annots with their styles and defaults), boots the name tree once, runs the node with Field x Mouse Up and the target value, and commits form effects then annotation effects.',
    plugins: plugins(JS_OFF({ identity: { email: 'ada@x.test' }, fileName: fnSpec('fileName', { returns: 'report.pdf' }) })),
    engine: scriptEngine(),
    steps: [
      start,
      open('a', { name: 'Named.pdf' }),
      ...sinks,
      listen(),
      listen('onScriptDiagnostic'),
      cap(
        'execute',
        tree(js('event.value')),
        widgetCtx({ kind: 'objectNumber', fieldObjectNumber: 100 }),
      ),
    ],
    snapshot: snap,
  },
  {
    name: 'script-effects-commit',
    covers: 'Form effects reach the form sink and annotation effects the annotation sink (object-number refs with their page, other refs as -1 without one).',
    plugins: plugins(JS_OFF({ sandbox: { builds: [{ run: [out({ formEffects: [formEffect], annotEffects: [annotEffect(), annotEffect(nmRef(1, 'tip'))] })] }] } })),
    engine: scriptEngine({}, { nameTreeScripts: [] }),
    steps: [...boot, ...sinks, listen(), cap('execute', tree(js('commit()')), USER)],
    snapshot: snap,
  },
  {
    name: 'script-commit-failures',
    covers:
      'No form sink drops form effects with no-commit-sink; a form failure skips the annotation effects; no annotation sink drops them; an annotation failure fails the node.',
    plugins: plugins(JS_OFF({ sandbox: { builds: [{ run: [out({ formEffects: [formEffect], annotEffects: [annotEffect()] })] }] } })),
    engine: scriptEngine({}, { nameTreeScripts: [] }),
    steps: [
      ...boot,
      listen(),
      cap('execute', tree(js('a()')), USER),
      cap(
        'registerFormCommitSink',
        fnSpec('sink.form', {
          seq: [
            { returns: { results: [{ index: 0, status: 'failed', error: { message: 'value refused' } }], changedWidgets: [], meta: null } },
            { returns: { results: [{ index: 0, status: 'rejected' }], changedWidgets: [], meta: null } },
            FORM_OK(1),
          ],
        }),
      ),
      cap('execute', tree(js('b()')), USER),
      cap('execute', tree(js('c()')), USER),
      cap('execute', tree(js('d()')), USER),
      cap('registerAnnotCommitSink', fnSpec('sink.annot', { seq: [{ returns: { results: [{ annotObjectNumber: 6, status: 'failed', error: 'locked' }] } }, { returns: { results: [{ annotObjectNumber: 6, status: 'failed' }] } }] })),
      cap('execute', tree(js('e()')), USER),
      cap('execute', tree(js('f()')), USER),
    ],
    snapshot: snap,
  },
  {
    name: 'boot-effects-and-a-refetched-world',
    covers: 'A boot that stages effects commits them in the first transaction, surfaces its UI effects as phase boot, and the run sees a refetched world; later nodes do not boot again.',
    plugins: plugins(
      JS_OFF({
        sandbox: {
          builds: [
            {
              boot: [out({ formEffects: [formEffect], uiEffects: [{ kind: 'alert', message: 'booted', icon: 3 }], diagnostics: [{ code: 'unsupported-api', message: 'app.beep' }], error: { kind: 'exception', message: 'boot threw', stack: 'at boot' } })],
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
      listen('onScriptDiagnostic'),
      listen('onScriptError'),
      cap('execute', tree(js('a()', [js('b()')])), USER),
    ],
    snapshot: snap,
  },
  {
    name: 'boot-without-effects',
    covers: 'A boot output with nothing to commit does not refetch the world.',
    plugins: plugins(JS_OFF({ sandbox: { builds: [{ boot: [out({ diagnostics: [{ code: 'script-error', message: 'boot warned' }] })] }] } })),
    engine: scriptEngine(),
    steps: [...boot, listen('onScriptDiagnostic'), cap('execute', tree(js('a()')), USER)],
    snapshot: snap,
  },
  {
    name: 'run-errors-and-diagnostics',
    covers: 'A run error fails the node with its message, after its diagnostics and error reach the script hooks; its effects are not committed.',
    plugins: plugins(
      JS_OFF({
        sandbox: {
          builds: [
            {
              run: [
                out({ formEffects: [formEffect], diagnostics: [{ code: 'blocked-network', message: 'fetch refused' }], error: { kind: 'budget', message: 'interrupted: time budget' } }),
                out({ error: { kind: 'invalid-output', message: 'output too large' } }),
              ],
            },
          ],
        },
      }),
    ),
    engine: scriptEngine({}, { nameTreeScripts: [] }),
    steps: [...boot, ...sinks, listen(), listen('onScriptDiagnostic'), listen('onScriptError'), cap('execute', tree(js('a()')), USER), cap('execute', tree(js('b()')), USER)],
    snapshot: snap,
  },
  {
    name: 'external-effects-run-after-the-transaction',
    covers: 'print and submitForm UI effects from boot and run are held back and fired after the transaction (print through will-print/adapter/did-print, submit through the sink chain); alert and gotoPage surface inline.',
    plugins: plugins(
      JS_OFF({
        sandbox: {
          builds: [
            {
              boot: [out({ uiEffects: [{ kind: 'print' }] })],
              run: [
                out({
                  uiEffects: [
                    { kind: 'gotoPage', page: 2 },
                    { kind: 'submitForm', url: 'https://s.test/', fieldNames: ['name', 'choice'], includeEmpty: true, format: 'xfdf', method: 'get' },
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
      cap('registerSubmitResolver', resolverEcho()),
      cap('setSubmitHandler', fnSpec('handler')),
      cap('execute', tree(js('a()')), USER),
    ],
    snapshot: snap,
  },
  {
    name: 'script-node-budget',
    covers: 'maxScriptNodesPerDispatch caps JS nodes per dispatch: the rest are inert with the budget reason, and the next dispatch starts a fresh count.',
    plugins: plugins(JS_OFF({ maxScriptNodesPerDispatch: 2 })),
    engine: scriptEngine({}, { nameTreeScripts: [] }),
    steps: [...boot, listen(), cap('execute', tree(js('1', [js('2', [js('3', [js('4')])])])), USER), cap('execute', tree(js('5')), USER)],
    snapshot: snap,
  },
  {
    name: 'script-event-provenance',
    covers:
      'event.type and event.name from the trigger provenance: Field, Link, Annot, Page, Doc, and every annotation, page and document event name; a widget target by object number or name, with a value only for a scalar.',
    plugins: plugins(JS_OFF()),
    engine: scriptEngine({}, { nameTreeScripts: [] }),
    steps: [
      ...boot,
      ...[
        widgetCtx({ kind: 'objectNumber', fieldObjectNumber: 101 }),
        widgetCtx({ kind: 'fqn', name: 'name' }, { scope: 'annotation', name: 'focus' }),
        widgetCtx({ kind: 'fqn', name: 'ghost' }),
        widgetCtx({ kind: 'objectNumber', fieldObjectNumber: 102 }),
        widgetCtx({ kind: 'fqn', name: 'empty' }),
        { origin: 'hover', source: { kind: 'link', annotation: aref(1, 7), page: pr(1) }, event: { scope: 'annotation', name: 'cursorEnter' } },
        { origin: 'hover', source: { kind: 'link', annotation: aref(2, 7), page: pr(2) }, event: { scope: 'annotation', name: 'cursorEnter' } },
        { origin: 'user', source: { kind: 'annotation', annotation: aref(2, 6), page: pr(2) }, event: { scope: 'annotation', name: 'mouseDown' } },
        { origin: 'hover', source: { kind: 'annotation', annotation: aref(1, 6), page: pr(1) }, event: { scope: 'annotation', name: 'cursorExit' } },
        { origin: 'user', source: { kind: 'annotation', annotation: aref(1, 6), page: pr(1) }, event: { scope: 'annotation', name: 'mouseDown' } },
        { origin: 'user', source: { kind: 'annotation', annotation: aref(1, 6), page: pr(1) }, event: { scope: 'annotation', name: 'mouseUp' } },
        { origin: 'user', source: { kind: 'annotation', annotation: aref(1, 6), page: pr(1) }, event: { scope: 'annotation', name: 'blur' } },
        { origin: 'lifecycle', source: { kind: 'page', page: pr(2) }, event: { scope: 'page', name: 'open' } },
        { origin: 'lifecycle', source: { kind: 'page', page: pr(2) }, event: { scope: 'page', name: 'close' } },
        { origin: 'lifecycle', source: { kind: 'page', page: pr(2) }, event: { scope: 'page', name: 'visible' } },
        { origin: 'lifecycle', source: { kind: 'page', page: pr(2) }, event: { scope: 'page', name: 'invisible' } },
        { origin: 'lifecycle', source: { kind: 'document' }, event: { scope: 'document', name: 'open' } },
        { origin: 'lifecycle', source: { kind: 'document' }, event: { scope: 'document', name: 'will-save' } },
        { origin: 'lifecycle', source: { kind: 'document' }, event: { scope: 'document', name: 'did-save' } },
        { origin: 'lifecycle', source: { kind: 'document' }, event: { scope: 'document', name: 'will-print' } },
        { origin: 'lifecycle', source: { kind: 'document' }, event: { scope: 'document', name: 'did-print' } },
        { origin: 'lifecycle', source: { kind: 'document' }, event: { scope: 'document', name: 'will-close' } },
        { origin: 'user', source: { kind: 'link' }, event: { scope: 'activate' } },
        { origin: 'user', source: { kind: 'api' }, event: { scope: 'activate' } },
      ].map((ctx) => cap('execute', tree(js('probe()')), ctx)),
    ],
    snapshot: 'none',
  },
  {
    name: 'no-page-to-anchor',
    covers: 'A document with no pages has nothing to anchor the world on: the node is inert.',
    plugins: plugins(JS_OFF()),
    engine: scriptEngine({ pages: [] }),
    steps: [...boot, listen(), cap('execute', tree(js('a()')), USER)],
    snapshot: snap,
  },
  {
    name: 'an-unknown-page-fails-the-node',
    covers: 'A source page that is not in the document makes the world read throw; the transaction rejects and the node fails.',
    plugins: plugins(JS_OFF()),
    engine: scriptEngine(),
    steps: [...boot, listen(), cap('execute', tree(js('a()')), { origin: 'lifecycle', source: { kind: 'page', page: pr(42) }, event: { scope: 'page', name: 'open' } })],
    snapshot: snap,
  },
  {
    name: 'environment-inputs',
    covers: 'Configured now, UTC offset, seed, file name and a function identity reach every input; the defaults are the clock, zero, the FNV seed and the open name.',
    plugins: plugins(
      JS_OFF({
        now: fnSpec('now', { returns: 1720000000000 }),
        utcOffsetMinutes: fnSpec('utcOffsetMinutes', { returns: 180 }),
        randomSeed: fnSpec('randomSeed', { returns: 7 }),
        identity: fnSpec('identity', { returns: { name: 'Fn User', corporation: 'Fn Inc' } }),
      }),
    ),
    engine: scriptEngine(),
    steps: [...boot, cap('execute', tree(js('a()')), USER)],
    snapshot: 'none',
  },
  {
    name: 'environment-defaults',
    covers: 'With nothing configured: Date.now() from the clock, a zero offset, seedFrom(documentId, sequence), the name the document was opened with, and the engine identity.',
    plugins: plugins(JS_OFF()),
    engine: scriptEngine(),
    steps: [start, open('a', { name: 'Opened.pdf' }), { op: 'advance', ms: 2500 }, cap('execute', tree(js('a()', [js('b()')])), USER)],
    snapshot: 'none',
  },
  {
    name: 'environment-file-name-fallback',
    covers: 'A document opened without a name is document.pdf to scripts.',
    plugins: plugins(JS_OFF()),
    engine: scriptEngine(),
    steps: [...boot, cap('execute', tree(js('a()')), USER)],
    snapshot: 'none',
  },
  {
    name: 'a-fault-rebuilds-the-realm',
    covers: 'A sandbox that faults during a run is rebuilt and booted again for the next node.',
    plugins: plugins(JS_OFF({ sandbox: { builds: [{ run: [{ dispose: true }] }, {}] } })),
    engine: scriptEngine(),
    steps: [...boot, cap('execute', tree(js('a()')), USER), cap('execute', tree(js('b()')), USER)],
    snapshot: 'none',
  },
  {
    name: 'a-throwing-sandbox-fails-the-node',
    covers: 'A sandbox run that throws rejects the transaction; the node fails with the message.',
    plugins: plugins(JS_OFF({ sandbox: { builds: [{ run: [{ throws: { message: 'vm exploded' } }] }] } })),
    engine: scriptEngine({}, { nameTreeScripts: [] }),
    steps: [...boot, listen(), cap('execute', tree(js('a()')), USER)],
    snapshot: snap,
  },
  {
    name: 'budget-reaches-the-sandbox',
    covers: 'A configured budget is the host default for every boot and run.',
    plugins: plugins(JS_OFF({ budget: { maxExecutionMs: 5, maxMemoryBytes: 1024, maxStackBytes: 256, maxEffects: 2, maxOutputBytes: 64 } })),
    engine: scriptEngine(),
    steps: [...boot, cap('execute', tree(js('a()')), USER)],
    snapshot: 'none',
  },
  {
    name: 'no-sandbox-host',
    covers: 'A host with no sandbox (the TypeScript with scripting disabled): javascript nodes are inert, scripting unavailable, and the capability has no scriptRealm member.',
    plugins: plugins({ openSequence: 'off', javascript: { enabled: true, sandbox: null } }),
    engine: scriptEngine(),
    steps: [...boot, listen(), cap('execute', tree(js('a()')), USER), { op: 'call', target: 'kernel', method: 'capability', args: [{ $token: 'actions' }, 'a'] }],
    snapshot: snap,
  },
  {
    name: 'script-transaction-port',
    covers: 'With scripting on, the host lens has scriptRealm.transaction: a body shares the realm queue with the executor and resolves with the body result.',
    plugins: plugins(JS_OFF({ sandbox: { builds: [{ run: [{}] }] } })),
    engine: scriptEngine(),
    steps: [
      ...boot,
      { op: 'call', target: 'kernel', method: 'capability', args: [{ $token: 'actions' }, 'a'] },
      cap('execute', tree(js('a()')), USER),
      // ref 8 is `scriptRealm.transaction` in the capability read above.
      callRef(8, fnSpec('body', { resolves: 'body-result' })),
      callRef(8, fnSpec('body.throws', { throws: { message: 'body threw' } })),
    ],
    snapshot: 'none',
  },
  {
    name: 'closing-the-document-disposes-the-realm',
    covers: 'Closing the document disposes the script realm and releases the event subscription.',
    plugins: plugins(JS_OFF()),
    engine: scriptEngine(),
    steps: [...boot, cap('execute', tree(js('a()')), USER), close()],
    snapshot: 'none',
  },
  {
    name: 'script-boot-without-a-catalog',
    covers: 'A scripted document without the actions service, or whose catalog has no name-tree scripts, boots nothing.',
    plugins: plugins(JS_OFF()),
    engine: {
      documents: {
        a: doc({ responses: [forms(scriptFields)] }),
        b: doc({ services: ['actions'], responses: [forms(scriptFields), { path: 'actions.read', value: { openAction: null } }] }),
      },
    },
    steps: [start, open('a'), open('b'), cap('execute', tree(js('a()')), USER), { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'execute', args: [tree(js('b()')), USER] }],
    snapshot: 'none',
  },
  {
    name: 'detached-script-realm',
    covers:
      'createDetachedScriptRealm mints a realm for another document under this document\'s environment: a fresh sandbox from the same factory, the target\'s id and metadata, the owner\'s identity, a seed from the target id, the target\'s boot sources, and the configured budget. The caller owns it: the transaction port queues bodies, and dispose ends the realm.',
    plugins: [
      { factory: 'actions', config: JS_OFF({ sandbox: { builds: [{}, { run: [{ output: { uiEffects: [{ kind: 'alert', message: 'hi', icon: 0 }] } }] }] }, budget: { maxExecutionMs: 9, maxMemoryBytes: 1024, maxStackBytes: 256, maxEffects: 2, maxOutputBytes: 64 } }) },
      { factory: 'detached-realm', config: { bootSources: fnSpec('asset.boot', { resolves: ['var asset = 1;'] }) } },
    ],
    engine: { documents: { a: scriptEngine().documents.a, b: doc() } },
    steps: [
      start,
      open('a'),
      open('b', { name: 'Asset.pdf' }),
      cap('isScriptingEnabled'),
      cap('execute', tree(js('own()')), USER),
      { op: 'call', target: { token: 'detached-realm', doc: 'b' }, method: 'mint', args: [] },
      { op: 'call', target: { token: 'detached-realm', doc: 'b' }, method: 'run', args: ['stamp()'] },
      callRef(0, fnSpec('body', { resolves: 'body-result' })),
      callRef(1),
      { op: 'call', target: { token: 'detached-realm', doc: 'b' }, method: 'run', args: ['again()'] },
    ],
    snapshot: 'none',
  },
  {
    name: 'detached-script-realm-without-metadata',
    covers: 'A target with no document metadata runs as document.pdf with zero pages; with scripting off there is nothing to mint.',
    plugins: [
      { factory: 'actions', config: JS_OFF() },
      { factory: 'detached-realm', config: { meta: null } },
    ],
    engine: { documents: { a: scriptEngine().documents.a, b: doc(), c: doc() } },
    steps: [
      start,
      open('a'),
      open('b'),
      { op: 'call', target: { token: 'detached-realm', doc: 'b' }, method: 'mint', args: [] },
      { op: 'call', target: { token: 'detached-realm', doc: 'b' }, method: 'run', args: ['stamp()'] },
    ],
    snapshot: 'none',
  },
  {
    name: 'no-detached-realm-without-scripting',
    covers: 'With scripting off the host lens has no createDetachedScriptRealm, so a sibling mints nothing.',
    plugins: [
      { factory: 'actions', config: { openSequence: 'off', javascript: { enabled: false } } },
      { factory: 'detached-realm' },
    ],
    engine: { documents: { a: doc(), b: doc() } },
    steps: [start, open('a'), open('b'), { op: 'call', target: { token: 'detached-realm', doc: 'b' }, method: 'mint', args: [] }],
    snapshot: 'none',
  },
  {
    name: 'scripting-disabled',
    covers: 'javascript.enabled false: no realm, javascript nodes inert.',
    plugins: plugins({ openSequence: 'off', javascript: { enabled: false } }),
    engine: scriptEngine(),
    steps: [...boot, cap('execute', tree(js('a()')), USER), cap('isScriptingEnabled')],
    snapshot: snap,
  },
];

// ── the surface door ──────────────────────────────────────────────────────────

const surface = (result: Record<string, unknown>) =>
  cap('surfaceScriptResult', { uiEffects: [], diagnostics: [], origin: 'user', phase: 'user', ...result });
const surfaceCases: ScenarioCase[] = [
  {
    name: 'surface-ui-effects',
    covers: 'alert (with and without a title), gotoPage and print reach the adapter with the origin and phase; an adapter without alert or gotoPage ignores them; diagnostics and the error reach the script hooks.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [
      ...boot,
      listen('onScriptDiagnostic'),
      listen('onScriptError'),
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
      surface({ uiEffects: [{ kind: 'alert', message: 'm3', icon: 1 }, { kind: 'gotoPage', page: 1 }] }),
    ],
    snapshot: 'none',
  },
  {
    name: 'surface-refusals',
    covers: 'No adapter is a no-adapter diagnostic; a print without doc.print is withheld with ui-effect-suppressed; a print during a document print event is suppressed as reentrant.',
    plugins: plugins(OFF),
    engine: { documents: { a: doc({ services: ['actions'], security: { allows: ['doc.render'] }, responses: [catalog()] }), b: doc({ services: ['actions'], responses: [catalog()] }) } },
    steps: [
      start,
      open('a'),
      open('b'),
      listen(),
      listen('onScriptDiagnostic'),
      surface({ uiEffects: [{ kind: 'alert', message: 'm', icon: 0 }, { kind: 'print' }] }),
      { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'onScriptDiagnostic', args: [fnSpec('b.onScriptDiagnostic')] },
      { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'setUiAdapter', args: [adapter()] },
      { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'runDocumentVerb', args: ['print', fnSpec('op.print', { defer: 'printing' })] },
      { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'surfaceScriptResult', args: [{ uiEffects: [{ kind: 'print' }], diagnostics: [], origin: 'user', phase: 'user' }] },
      settle('printing', 'done'),
      { op: 'call', target: { token: 'actions', doc: 'b' }, method: 'surfaceScriptResult', args: [{ uiEffects: [{ kind: 'print' }], diagnostics: [], origin: 'user', phase: 'user' }] },
    ],
    snapshot: 'none',
  },
  {
    name: 'surface-submit',
    covers: 'A scripted submitForm from the form pipeline is detached through the same submit door (api source, activate event), with format fdf and method post by default and include-mode names.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [
      ...boot,
      listen(),
      cap('registerSubmitResolver', resolverEcho()),
      cap('setSubmitHandler', fnSpec('handler')),
      surface({ uiEffects: [{ kind: 'submitForm', url: null, fieldNames: null, includeEmpty: false }, { kind: 'submitForm', url: 'https://x.test', fieldNames: ['a'], includeEmpty: true, format: 'html', method: 'get' }], origin: 'lifecycle' }),
      surface({ uiEffects: [{ kind: 'submitForm', url: null, fieldNames: null, includeEmpty: false }], origin: 'user' }),
    ],
    snapshot: 'none',
  },
  {
    name: 'surface-a-detached-realm',
    covers:
      'A detached realm has no document surface: its print, gotoPage and submitForm requests are suppressed with ui-effect-suppressed, and only alerts reach the adapter.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [
      ...boot,
      listen(),
      listen('onScriptDiagnostic'),
      cap('registerSubmitResolver', resolverEcho()),
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
    snapshot: 'none',
  },
  {
    name: 'surface-script-commit',
    covers:
      'surfaceScriptCommit splits a K/V/C/F commit into its boot-phase effects (only when there are any) and its user-phase effects, which carry the diagnostics and the error even with no user effects; the realm kind rides both.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [
      ...boot,
      listen('onScriptDiagnostic'),
      listen('onScriptError'),
      cap('setUiAdapter', adapter({ alert: fnSpec('ui.alert'), gotoPage: fnSpec('ui.gotoPage') })),
      cap(
        'surfaceScriptCommit',
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
      cap('surfaceScriptCommit', { uiEffects: [], diagnostics: [] }, { origin: 'hover', realm: 'document' }),
      cap(
        'surfaceScriptCommit',
        { uiEffects: [], diagnostics: [{ code: 'unsupported-api', message: 'no effects' }], error: { kind: 'exception', message: 'no effects' } },
        { origin: 'user', realm: 'document' },
      ),
      cap(
        'surfaceScriptCommit',
        { uiEffects: [{ kind: 'print', phase: 'boot' }, { kind: 'alert', message: 'asset', icon: 0, phase: 'user' }], diagnostics: [] },
        { origin: 'user', realm: 'detached' },
      ),
    ],
    snapshot: 'none',
  },
  {
    name: 'script-hooks-isolate-observers',
    covers: 'A throwing onScriptDiagnostic or onScriptError observer is reported to the host sink.',
    plugins: plugins(OFF),
    engine: engineWith(),
    steps: [
      ...boot,
      cap('onScriptDiagnostic', fnSpec('bad.diag', { throws: { message: 'diag broke' } })),
      cap('onScriptError', fnSpec('bad.error', { throws: { message: 'error broke' } })),
      surface({ diagnostics: [{ code: 'script-error', message: 'd' }], error: { kind: 'exception', message: 'e' } }),
    ],
    snapshot: 'none',
  },
];

// ── the hover pump ────────────────────────────────────────────────────────────

const pumpPlugins = (dispatchSpec: 'actions' | Scripted) =>
  dispatchSpec === 'actions'
    ? [{ factory: 'actions', config: OFF }, { factory: 'hover-pump', config: { dispatch: 'actions' } }]
    : [{ factory: 'hover-pump', config: { dispatch: dispatchSpec } }];
const hover = (target: unknown) => ({ op: 'call', target: { token: 'hover-pump', doc: 'a' }, method: 'hover', args: [target] }) as Step;
const pumpReset: Step = { op: 'call', target: { token: 'hover-pump', doc: 'a' }, method: 'reset', args: [] };
const hoverTarget = (objectNumber: number, events?: unknown, ref?: unknown, pon = 1) => ({
  ref: ref ?? aref(pon, objectNumber),
  page: pr(pon),
  ...(events === undefined ? {} : { events }),
});
const scriptedDispatch = (labels: string[]) => fnSpec('dispatch', { seq: labels.map((label) => ({ defer: label })) });
const done = { status: 'executed', steps: [], diagnostics: [] };

const hoverCases: ScenarioCase[] = [
  {
    name: 'pairs-are-submitted-back-to-back',
    covers: 'Enter(A); then A to B while idle submits Exit(A) and Enter(B) together, synchronously.',
    plugins: pumpPlugins(scriptedDispatch(['d1', 'd2', 'd3'])),
    engine: engineWith(),
    steps: [...boot, hover(hoverTarget(1)), settle('d1', done), hover(hoverTarget(2)), settle('d2', done), settle('d3', done)],
    snapshot: 'none',
  },
  {
    name: 'a-slow-transition-is-never-split',
    covers: 'While a transition is in flight nothing else is submitted; after it settles the pump delivers the latest target.',
    plugins: pumpPlugins(scriptedDispatch(['d1', 'd2', 'd3'])),
    engine: engineWith(),
    steps: [...boot, hover(hoverTarget(1)), hover(hoverTarget(2)), settle('d1', done), settle('d2', done), settle('d3', done)],
    snapshot: 'none',
  },
  {
    name: 'intermediate-targets-are-skipped',
    covers: 'A to B to C to nothing to D while A is in flight delivers Exit(A) then Enter(D).',
    plugins: pumpPlugins(scriptedDispatch(['d1', 'd2', 'd3'])),
    engine: engineWith(),
    steps: [...boot, hover(hoverTarget(1)), hover(hoverTarget(2)), hover(hoverTarget(3)), hover(null), hover(hoverTarget(4)), settle('d1', done), settle('d2', done), settle('d3', done)],
    snapshot: 'none',
  },
  {
    name: 'same-target-is-free-and-exit-happens-once',
    covers: 'Hovering the same target submits nothing; leaving to nothing exits once, and nothing again stays silent.',
    plugins: pumpPlugins(scriptedDispatch(['d1', 'd2'])),
    engine: engineWith(),
    steps: [...boot, hover(hoverTarget(1)), settle('d1', done), hover(hoverTarget(1)), hover(null), settle('d2', done), hover(null)],
    snapshot: 'none',
  },
  {
    name: 'event-flags-skip-tree-less-halves',
    covers: 'events.enter false and events.exit false skip their half; a transition with nothing to submit settles on the next turn.',
    plugins: pumpPlugins(scriptedDispatch(['d1', 'd2'])),
    engine: engineWith(),
    steps: [
      ...boot,
      hover(hoverTarget(1, { enter: false })),
      hover(hoverTarget(2, { exit: false })),
      settle('d1', done),
      settle('d2', done),
      hover(null),
      hover(hoverTarget(3, {})),
    ],
    snapshot: 'none',
  },
  {
    name: 'reset-forgets-without-dispatching',
    covers: 'reset drops the delivered target, so the next hover enters without an exit.',
    plugins: pumpPlugins(scriptedDispatch(['d1', 'd2'])),
    engine: engineWith(),
    steps: [...boot, hover(hoverTarget(1)), settle('d1', done), pumpReset, hover(hoverTarget(2)), settle('d2', done)],
    snapshot: 'none',
  },
  {
    name: 'target-identity',
    covers: 'Targets compare by ref kind and page: object numbers, /NM, and index plus revision; a different kind or page is a new target.',
    plugins: pumpPlugins(fnSpec('dispatch', { resolves: done })),
    engine: engineWith(),
    steps: [
      ...boot,
      hover(hoverTarget(1, undefined, nmRef(1, 'tip'))),
      hover(hoverTarget(1, undefined, nmRef(1, 'tip'))),
      hover(hoverTarget(1, undefined, nmRef(1, 'other'))),
      hover(hoverTarget(1, undefined, indexRef(1, 2, 'r1'))),
      hover(hoverTarget(1, undefined, indexRef(1, 2, 'r1'))),
      hover(hoverTarget(1, undefined, indexRef(1, 2, 'r2'))),
      hover(hoverTarget(1, undefined, indexRef(1, 3, 'r2'))),
      hover(hoverTarget(1, undefined, aref(1, 1))),
      hover(hoverTarget(1, undefined, aref(2, 1), 2)),
      hover(hoverTarget(1, undefined, aref(2, 1), 2)),
      hover(hoverTarget(1, undefined, aref(2, 2), 2)),
    ],
    snapshot: 'none',
  },
  {
    name: 'a-rejected-dispatch-still-releases',
    covers: 'allSettled: a rejected dispatch releases the pump like a resolved one.',
    plugins: pumpPlugins(fnSpec('dispatch', { seq: [{ rejects: { message: 'no' } }, { resolves: done }] })),
    engine: engineWith(),
    steps: [...boot, hover(hoverTarget(1)), hover(hoverTarget(2))],
    snapshot: 'none',
  },
  {
    name: 'an-unknown-ref-kind-never-matches-itself',
    covers: 'sameTarget is false for a ref kind it does not know, even against itself, so every settle delivers the same target again.',
    plugins: pumpPlugins(scriptedDispatch(['d1', 'd2', 'd3'])),
    engine: engineWith(),
    steps: [...boot, hover(hoverTarget(1, undefined, { kind: 'mystery' })), settle('d1', done), hover(hoverTarget(1, undefined, { kind: 'mystery' }))],
    snapshot: 'none',
  },
  {
    name: 'drives-the-real-dispatcher',
    covers: 'Over the actions capability: the pump forwards the source hint, and the /E and /X trees run as hover-origin steps.',
    plugins: pumpPlugins('actions'),
    engine: engineWith({
      responses: [annotations(1, [annot(1, 5, { cursorEnter: tree(named('show')), cursorExit: tree(named('hide')) }), annot(1, 6, { cursorEnter: tree(named('show-6')) })])],
    }),
    steps: [
      ...boot,
      reg('named'),
      listen('onExecuted'),
      hover({ ...hoverTarget(5), source: { kind: 'link', annotation: aref(1, 5), page: pr(1) } }),
      hover(hoverTarget(6)),
      hover(null),
    ],
    snapshot: snap,
  },
];

// ── pure helpers ──────────────────────────────────────────────────────────────

const triggers = [
  activate(1, aref(1, 1)),
  ...['cursorEnter', 'cursorExit', 'mouseDown', 'mouseUp', 'focus', 'blur'].map((event) => annotationEvent(event, 1, aref(1, 1))),
  ...['open', 'close', 'visible', 'invisible'].map((event) => pageEvent(event, 1)),
  ...['open', 'will-save', 'did-save', 'will-print', 'did-print', 'will-close'].map(docEvent),
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
      ],
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
