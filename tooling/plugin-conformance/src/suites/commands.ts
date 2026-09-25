/**
 * `packages/plugin/commands`. Replayed by
 * `crates/plugins/commands/tests/conformance.rs`.
 *
 * Two files: the pure parts (the reducer, `resolvedCommandsEqual`,
 * `registerCommand`, and what `commandsPlugin(config)` builds) as function
 * cases, and the workspace capability as scenarios, alone and next to the
 * i18n and shell plugins it reads.
 *
 * A command carries functions (`run`, `enabled`, `active`, `visible`,
 * `iconAccent`), so a vector writes each as a `$fn` spec. Commands in a
 * `register` step's arguments are decoded by the runner, as every `$fn` is.
 * Commands in `config.commands` are decoded by this factory, which the
 * runner leaves config to, and the factory's functions take one extra
 * member: `uses`, a list of things to do with the `CommandCtx` they are
 * handed before they answer. That is how a vector reaches `ctx.get`,
 * `ctx.tryGet` and `ctx.core`:
 *
 *   {"get": token, "call"?: method, "args"?: [...]}     ctx.get(token)[method](...args)
 *   {"tryGet": token, "call"?: method, "args"?: [...]}  the same through ctx.tryGet
 *   {"core": member}                                    ctx.core()[member]
 *
 * Each use answers `{returned}` or `{threw}` (a `get` without `call`
 * answers `{returned: "resolved"}`), and the list is logged as the callback
 * `<name>:uses` after the function's own call. A use's `args` are decoded
 * the same way, so a use can register a command.
 */
import {
  commandsPlugin,
  commandsReducer,
  CommandsToken,
  initialCommandsState,
  resolvedCommandsEqual,
} from '../../../../packages/plugin/commands/src/index';
import { registerCommand, type CommandRegistry } from '../../../../packages/plugin/commands/src/capability';
import type {
  CommandCtx,
  CommandDef,
  CommandsAction,
  CommandsConfig,
  CommandsState,
  ResolvedCommand,
} from '../../../../packages/plugin/commands/src/types';

import { makeError, type FakeDocumentSpec } from '../fake-engine';
import { registerPlugin, tokenNamed, type FnSpec, type PluginEnv, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';

// ── the factory ───────────────────────────────────────────────────────────────

interface Use {
  readonly get?: string;
  readonly tryGet?: string;
  readonly core?: string;
  readonly call?: string;
  readonly args?: readonly unknown[];
}

interface CommandFnSpec extends FnSpec {
  readonly uses?: readonly Use[];
}

function decodeCommandValue(value: unknown, env: PluginEnv): unknown {
  if (Array.isArray(value)) return value.map((v) => decodeCommandValue(v, env));
  if (value === null || typeof value !== 'object') return value;
  const object = value as Record<string, unknown>;
  if ('$undefined' in object) return undefined;
  if ('$fn' in object) return commandFn(object as unknown as CommandFnSpec, env);
  const out: Record<string, unknown> = {};
  for (const [key, member] of Object.entries(object)) out[key] = decodeCommandValue(member, env);
  return out;
}

function runUse(use: Use, ctx: CommandCtx, env: PluginEnv): unknown {
  if (use.core !== undefined) return { returned: (ctx.core() as unknown as Record<string, unknown>)[use.core] };
  let capability: Record<string, (...args: unknown[]) => unknown> | null;
  try {
    capability =
      use.get !== undefined
        ? (ctx.get(tokenNamed(use.get)) as typeof capability)
        : (ctx.tryGet(tokenNamed(use.tryGet!)) as typeof capability);
  } catch (error) {
    return { threw: error };
  }
  if (capability === null) return { returned: null };
  if (use.call === undefined) return { returned: 'resolved' };
  try {
    const args = (use.args ?? []).map((arg) => decodeCommandValue(arg, env));
    return { returned: capability[use.call](...args) };
  } catch (error) {
    return { threw: error };
  }
}

function commandFn(spec: CommandFnSpec, env: PluginEnv): (ctx: CommandCtx) => unknown {
  let calls = 0;
  return (ctx: CommandCtx) => {
    env.log(spec.$fn, [ctx]);
    if (spec.uses) env.log(`${spec.$fn}:uses`, spec.uses.map((use) => runUse(use, ctx, env)));
    const answer = spec.seq ? spec.seq[Math.min(calls, spec.seq.length - 1)] : spec;
    calls += 1;
    if (answer.throws) throw makeError(answer.throws);
    if (answer.defer !== undefined) {
      const label = answer.defer;
      return new Promise((resolve, reject) => env.defer(label, resolve, reject));
    }
    if (answer.rejects) return Promise.reject(makeError(answer.rejects));
    if ('resolves' in answer) return Promise.resolve(decodeCommandValue(answer.resolves, env));
    return decodeCommandValue(answer.returns, env);
  };
}

registerPlugin('commands', {
  factory: (raw, env) => {
    if (raw === undefined) return commandsPlugin();
    return commandsPlugin(decodeCommandValue(raw, env) as CommandsConfig);
  },
  tokens: [CommandsToken],
});

// ── function cases ────────────────────────────────────────────────────────────

const reduce = (state: CommandsState, action: CommandsAction) => {
  const next = commandsReducer(state, action);
  return { same: next === state, next };
};

/** Register each definition into a fresh registry, in order; the registry
 *  as it stands, and the error that stopped it, if one did. */
const register = (defs: CommandDef[]) => {
  const registry: CommandRegistry = new Map();
  let error: unknown;
  try {
    for (const def of defs) registerCommand(registry, def);
  } catch (e) {
    error = e;
  }
  const entries = [...registry].map(([id, entry]) => ({ id, shortcuts: entry.shortcuts, parsed: entry.parsed }));
  return error === undefined ? { entries } : { entries, error };
};

/** What `commandsPlugin(config)` builds: its identity and initial state. */
const plugin = (config?: CommandsConfig) => {
  const def = commandsPlugin(config);
  return { id: def.id, scope: def.scope, initialState: def.initialState };
};

/** `resolvedCommandsEqual(a, a)`: one object on both sides. */
const equalSelf = (a: ResolvedCommand) => resolvedCommandsEqual(a, a);

const cats = (...disabledCategories: string[]): CommandsState => ({ disabledCategories });

const reducerCases: FunctionCase[] = [
  { name: 'initial-state', fn: 'initialCommandsState', args: [] },
  { name: 'disable-new', fn: 'commandsReducer', args: [cats('a'), { type: 'COMMANDS/DISABLE_CATEGORY', category: 'b' }] },
  { name: 'disable-into-empty', fn: 'commandsReducer', args: [cats(), { type: 'COMMANDS/DISABLE_CATEGORY', category: 'a' }] },
  { name: 'disable-existing-is-ignored', fn: 'commandsReducer', args: [cats('a', 'b'), { type: 'COMMANDS/DISABLE_CATEGORY', category: 'b' }] },
  { name: 'enable-removes-every-copy', fn: 'commandsReducer', args: [cats('a', 'b', 'a'), { type: 'COMMANDS/ENABLE_CATEGORY', category: 'a' }] },
  { name: 'enable-missing-is-ignored', fn: 'commandsReducer', args: [cats('a'), { type: 'COMMANDS/ENABLE_CATEGORY', category: 'z' }] },
  { name: 'enable-last', fn: 'commandsReducer', args: [cats('a'), { type: 'COMMANDS/ENABLE_CATEGORY', category: 'a' }] },
  { name: 'set-replaces', fn: 'commandsReducer', args: [cats('a'), { type: 'COMMANDS/SET_DISABLED_CATEGORIES', categories: ['b', 'b', 'c'] }] },
  { name: 'set-equal-is-a-new-state', fn: 'commandsReducer', args: [cats('a'), { type: 'COMMANDS/SET_DISABLED_CATEGORIES', categories: ['a'] }] },
  { name: 'set-empty', fn: 'commandsReducer', args: [cats('a'), { type: 'COMMANDS/SET_DISABLED_CATEGORIES', categories: [] }] },
  { name: 'unknown-action-is-ignored', fn: 'commandsReducer', args: [cats('a'), { type: 'COMMANDS/NOPE', category: 'a' }] },
];

const rc = (over: Partial<ResolvedCommand> = {}): ResolvedCommand => ({
  id: 'zoom:in',
  label: 'Zoom In',
  icon: 'zoomIn',
  iconAccent: { primary: '#000', secondary: '#fff' },
  shortcuts: ['Mod+=', 'Mod+Plus'],
  menu: 'zoom',
  enabled: true,
  active: false,
  visible: true,
  categories: ['zoom'],
  ...over,
});
const eq = (name: string, a: unknown, b: unknown): FunctionCase => ({ name, fn: 'resolvedCommandsEqual', args: [a, b] });

const equalCases: FunctionCase[] = [
  eq('both-null', null, null),
  eq('null-left', null, rc()),
  eq('null-right', rc(), null),
  eq('equal-values', rc(), rc()),
  { name: 'same-object', fn: 'resolvedCommandsEqualSelf', args: [rc()] },
  eq('id-differs', rc(), rc({ id: 'zoom:out' })),
  eq('label-differs', rc(), rc({ label: 'Zoom in' })),
  eq('icon-differs', rc(), rc({ icon: 'zoomOut' })),
  eq('icon-absent-on-one-side', rc(), rc({ icon: undefined })),
  eq('icon-absent-on-both', rc({ icon: undefined }), rc({ icon: undefined })),
  eq('accent-primary-differs', rc(), rc({ iconAccent: { primary: '#111', secondary: '#fff' } })),
  eq('accent-secondary-differs', rc(), rc({ iconAccent: { primary: '#000', secondary: '#eee' } })),
  eq('accent-absent-on-one-side', rc(), rc({ iconAccent: undefined })),
  eq('accent-absent-vs-empty', rc({ iconAccent: undefined }), rc({ iconAccent: {} })),
  eq('accent-primary-only-both', rc({ iconAccent: { primary: '#000' } }), rc({ iconAccent: { primary: '#000' } })),
  eq('menu-differs', rc(), rc({ menu: 'other' })),
  eq('menu-absent-on-one-side', rc(), rc({ menu: undefined })),
  eq('enabled-differs', rc(), rc({ enabled: false })),
  eq('active-differs', rc(), rc({ active: true })),
  eq('visible-differs', rc(), rc({ visible: false })),
  eq('shortcut-count-differs', rc(), rc({ shortcuts: ['Mod+='] })),
  eq('shortcut-differs', rc(), rc({ shortcuts: ['Mod+=', 'Mod+Add'] })),
  eq('shortcut-order-differs', rc(), rc({ shortcuts: ['Mod+Plus', 'Mod+='] })),
  eq('no-shortcuts-both', rc({ shortcuts: [] }), rc({ shortcuts: [] })),
  eq('categories-are-not-compared', rc(), rc({ categories: ['other', 'more'] })),
];

const def = (id: string, over: Partial<CommandDef> = {}): CommandDef => ({ id, labelKey: id, ...over });
const reg = (name: string, defs: CommandDef[]): FunctionCase => ({ name, fn: 'registerCommand', args: [defs] });

const registerCases: FunctionCase[] = [
  reg('none', []),
  reg('no-shortcut', [def('a')]),
  reg('one-shortcut', [def('a', { shortcut: 'Mod+K' })]),
  reg('many-shortcuts', [def('a', { shortcut: ['Ctrl+=', 'Shift+Alt+D', 'Meta+NumpadAdd', 'Ctrl++', 'Escape'] })]),
  reg('empty-shortcut-list', [def('a', { shortcut: [] })]),
  reg('empty-shortcut-is-plus', [def('a', { shortcut: '' })]),
  reg('insertion-order', [def('z'), def('m', { shortcut: 'M' }), def('a'), def('10'), def('2')]),
  reg('duplicate-throws-and-keeps-the-first', [def('a', { shortcut: 'A' }), def('b'), def('a', { shortcut: 'B' }), def('c')]),
  reg('bad-modifier-throws-and-registers-nothing', [def('a'), def('b', { shortcut: ['Ctrl+B', 'Hyper+B'] }), def('c')]),
  reg('bad-modifier-in-a-string', [def('a', { shortcut: 'Mod+Hyper+K' })]),
];

const pluginCases: FunctionCase[] = [
  { name: 'plugin-no-config', fn: 'commandsPlugin', args: [] },
  { name: 'plugin-empty-config', fn: 'commandsPlugin', args: [{}] },
  { name: 'plugin-disabled-categories', fn: 'commandsPlugin', args: [{ disabledCategories: ['pro', 'beta', 'pro'] }] },
  { name: 'plugin-commands', fn: 'commandsPlugin', args: [{ commands: [def('a', { shortcut: 'Mod+A' }), def('b')] }] },
  { name: 'plugin-duplicate-command-throws', fn: 'commandsPlugin', args: [{ commands: [def('a'), def('a')] }] },
  { name: 'plugin-bad-shortcut-throws', fn: 'commandsPlugin', args: [{ commands: [def('a', { shortcut: 'Super+A' })] }] },
];

// ── scenarios ─────────────────────────────────────────────────────────────────

const box = { left: 0, bottom: 0, right: 600, top: 800 };
const pages = [{ index: 0, pageObjectNumber: 1, label: null, size: { width: 600, height: 800 }, rotation: 0, userUnit: 1, boxes: { media: box, crop: box } }];
const doc = (over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec => ({ pages, ...over });

const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const destroy: Step = { op: 'call', target: 'kernel', method: 'destroy' };
const open = (id: string): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id, bytes: { $bytes: '' } }],
});
const close = (id: string): Step => ({ op: 'call', target: 'documents', method: 'close', args: [id] });
const setActive = (id: string): Step => ({ op: 'call', target: 'documents', method: 'setActive', args: [id] });
const cmd = (method: string, ...args: unknown[]): Step => ({ op: 'call', target: { token: 'commands' }, method, args });
const shell = (method: string, args: unknown[] = [], doc = 'a'): Step => ({ op: 'call', target: { token: 'shell', doc }, method, args });
const i18n = (method: string, ...args: unknown[]): Step => ({ op: 'call', target: { token: 'i18n' }, method, args });

/** A `$fn` spec. */
const fn = (name: string, answer: Omit<CommandFnSpec, '$fn'> = {}) => ({ $fn: name, ...answer });
const yes = (name: string) => fn(name, { returns: true });
const no = (name: string) => fn(name, { returns: false });
const boom = (name: string, message = `${name} failed`) => fn(name, { throws: { name: 'Error', message } });

type VectorDef = Record<string, unknown> & { id: string; labelKey: string };
const vdef = (id: string, over: Record<string, unknown> = {}): VectorDef => ({ id, labelKey: id, ...over });
const commands = (defs: VectorDef[], disabledCategories?: string[]) => ({
  factory: 'commands',
  config: disabledCategories === undefined ? { commands: defs } : { commands: defs, disabledCategories },
});
const withShell = { factory: 'shell' };
const en = {
  code: 'en',
  name: 'English',
  translations: { commands: { zoomIn: 'Zoom In', zoomOut: 'Zoom Out', search: 'Search Document', print: 'Print' } },
};
const es = {
  code: 'es',
  name: 'Español',
  translations: { commands: { zoomIn: 'Acercar', zoomOut: 'Alejar', search: 'Buscar' } },
};
const withI18n = { factory: 'i18n', config: { locales: [en, es], locale: 'en' } };

const stroke = (key: string, over: Record<string, unknown> = {}) => ({
  key,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  ...over,
});
const mac = { isMac: true };
const pc = { isMac: false };

const cases: ScenarioCase[] = [
  {
    name: 'capability-shape',
    covers: 'The capability is workspace-scoped and exists from createKernel(); its members, in the object literal order; the slice holds only disabledCategories.',
    plugins: [{ factory: 'commands' }],
    steps: [
      { op: 'call', target: 'kernel', method: 'capability', args: [{ $token: 'commands' }] },
      { op: 'call', target: 'kernel', method: 'scopeOf', args: [{ $token: 'commands' }] },
      cmd('ids'),
      cmd('disabledCategories'),
      start,
      cmd('ids'),
      { op: 'call', target: { ref: 3 } },
      { op: 'call', target: { ref: 9 } },
    ],
  },
  {
    name: 'registry',
    covers:
      'Config commands register in order; register appends and throws on a duplicate or an unparsable shortcut (registering nothing); unregister removes (a missing id is a no-op), and re-registering moves an id to the end; has, ids and menuTarget read the registry.',
    plugins: [commands([vdef('b', { menu: 'zoom' }), vdef('a', { shortcut: 'Mod+A' })])],
    steps: [
      cmd('ids'),
      cmd('has', 'a'),
      cmd('has', 'z'),
      cmd('register', vdef('c', { shortcut: ['Ctrl+C', 'Meta+C'] })),
      cmd('ids'),
      cmd('register', vdef('a', { labelKey: 'again' })),
      cmd('register', vdef('d', { shortcut: ['Ctrl+D', 'Hyper+D'] })),
      cmd('has', 'd'),
      cmd('register', vdef('e', { shortcut: 'Nope+E' })),
      cmd('ids'),
      cmd('unregister', 'b'),
      cmd('unregister', 'b'),
      cmd('unregister', 'missing'),
      cmd('has', 'b'),
      cmd('register', vdef('b')),
      cmd('ids'),
      cmd('menuTarget', 'b'),
      cmd('register', vdef('m', { menu: 'file' })),
      cmd('menuTarget', 'm'),
      cmd('menuTarget', 'nope'),
      cmd('register', vdef('', { labelKey: 'empty id' })),
      cmd('has', ''),
      cmd('resolve', ''),
      cmd('ids'),
    ],
  },
  {
    name: 'resolve-defaults',
    covers:
      'With no i18n and no shell, a command resolves to its labelKey verbatim, enabled and visible, not active, with no accent, categories [] and its shortcuts as written; a missing id resolves to null. Surface targets without a shell are inactive.',
    plugins: [
      commands([
        vdef('plain'),
        vdef('full', { labelKey: 'commands.full', icon: 'star', shortcut: 'Mod+F', menu: 'more', categories: ['x', 'y'] }),
        vdef('many', { shortcut: ['Ctrl+=', 'Ctrl+Plus'] }),
        vdef('panel', { panel: 'search' }),
        vdef('panel-object', { panel: { id: 'thumbs', exclusive: 'left' } }),
        vdef('modal', { modal: 'print' }),
      ]),
    ],
    engine: { documents: { a: doc() } },
    steps: [
      cmd('resolve', 'plain'),
      cmd('resolve', 'full'),
      cmd('resolve', 'many'),
      cmd('resolve', 'missing'),
      cmd('resolve', 'panel'),
      start,
      open('a'),
      cmd('resolve', 'panel-object'),
      cmd('resolve', 'modal'),
      cmd('resolve', 'full', 'a'),
    ],
  },
  {
    name: 'derivations',
    covers:
      'resolve() calls active, then iconAccent, then enabled, then visible, each with a CommandCtx bound to the explicit document, else the active one (null with none open, and an empty string is kept); their answers land on the resolved command.',
    plugins: [
      commands([
        vdef('d', {
          active: fn('active', { seq: [{ returns: true }, { returns: false }] }),
          iconAccent: fn('accent', { returns: { primary: '#e5484d', secondary: '#ffffff' } }),
          enabled: fn('enabled', { seq: [{ returns: false }, { returns: true }] }),
          visible: fn('visible', { seq: [{ returns: true }, { returns: false }, { returns: true }] }),
        }),
      ]),
    ],
    engine: { documents: { a: doc(), b: doc() } },
    steps: [
      cmd('resolve', 'd'),
      start,
      open('a'),
      open('b'),
      cmd('resolve', 'd'),
      cmd('resolve', 'd', 'a'),
      cmd('resolve', 'd', 'zzz'),
      cmd('resolve', 'd', ''),
      cmd('resolve', 'd', null),
      cmd('resolve', 'missing', 'a'),
    ],
  },
  {
    name: 'throwing-derivations-fall-back',
    covers:
      'A derivation that throws falls back: enabled and visible to true, active to false, iconAccent to none. The command still resolves, and execute still runs it.',
    plugins: [
      commands([
        vdef('t', {
          active: boom('active'),
          iconAccent: boom('accent'),
          enabled: boom('enabled'),
          visible: boom('visible'),
          run: fn('run'),
        }),
      ]),
    ],
    steps: [cmd('resolve', 't'), cmd('execute', 't'), cmd('search', 't')],
  },
  {
    name: 'icon-accent',
    covers:
      'iconAccent carries what the derivation returns: both colours, one, an empty object; null and undefined mean no accent; a throw means no accent; an absent derivation is never called.',
    plugins: [
      commands([
        vdef('both', { icon: 'square', iconAccent: fn('both', { returns: { primary: '#e5484d', secondary: '#ffffff' } }) }),
        vdef('primary', { iconAccent: fn('primary', { returns: { primary: '#000' } }) }),
        vdef('secondary', { iconAccent: fn('secondary', { returns: { secondary: '#fff' } }) }),
        vdef('empty', { iconAccent: fn('empty', { returns: {} }) }),
        vdef('null', { iconAccent: fn('null', { returns: null }) }),
        vdef('undefined', { iconAccent: fn('undefined', { returns: { $undefined: true } }) }),
        vdef('throws', { iconAccent: boom('throws', 'no provider') }),
        vdef('absent'),
      ]),
    ],
    steps: ['both', 'primary', 'secondary', 'empty', 'null', 'undefined', 'throws', 'absent'].map((id) => cmd('resolve', id)),
  },
  {
    name: 'categories',
    covers:
      'config.disabledCategories seeds the slice (copied); a command in a disabled category is disabled and hidden though its derivations still run; disable, enable and set dispatch every time, and the reducer ignores what changes nothing; execute refuses a hidden command.',
    plugins: [
      commands(
        [
          vdef('pro', { categories: ['pro'], enabled: yes('pro.enabled'), visible: yes('pro.visible'), run: fn('pro.run') }),
          vdef('both', { categories: ['free', 'beta'], run: fn('both.run') }),
          vdef('none', { run: fn('none.run') }),
        ],
        ['pro'],
      ),
    ],
    steps: [
      cmd('disabledCategories'),
      cmd('isCategoryDisabled', 'pro'),
      cmd('isCategoryDisabled', 'free'),
      cmd('resolve', 'pro'),
      cmd('execute', 'pro'),
      cmd('search', ''),
      cmd('disableCategory', 'pro'),
      cmd('disableCategory', 'beta'),
      cmd('resolve', 'both'),
      cmd('execute', 'both'),
      cmd('enableCategory', 'pro'),
      cmd('enableCategory', 'pro'),
      cmd('resolve', 'pro'),
      cmd('execute', 'pro'),
      cmd('setDisabledCategories', ['free', 'free']),
      cmd('disabledCategories'),
      cmd('resolve', 'both'),
      cmd('enableCategory', 'free'),
      cmd('disabledCategories'),
      cmd('execute', 'both'),
      cmd('setDisabledCategories', []),
      cmd('execute', 'none'),
    ],
  },
  {
    name: 'i18n-labels',
    covers:
      'With i18n present the label is t(labelKey), following the locale; a missing key comes back as the key (and i18n warns once); search matches the translated label.',
    plugins: [
      withI18n,
      commands([
        vdef('zoom:in', { labelKey: 'commands.zoomIn' }),
        vdef('zoom:out', { labelKey: 'commands.zoomOut' }),
        vdef('print', { labelKey: 'commands.print' }),
        vdef('missing', { labelKey: 'commands.nope' }),
      ]),
    ],
    steps: [
      cmd('resolve', 'zoom:in'),
      cmd('resolve', 'missing'),
      cmd('resolve', 'missing'),
      cmd('search', 'zoom'),
      i18n('setLocale', 'es'),
      cmd('resolve', 'zoom:in'),
      cmd('resolve', 'print'),
      cmd('search', 'acer'),
      cmd('search', 'zoom'),
      cmd('search', 'print'),
    ],
  },
  {
    name: 'shell-derived-active',
    covers:
      'Without an active override, a menu command is active while its menu is open, a panel (string or object) or modal command while its surface is open; a menu wins over a panel, a panel over a modal; an empty menu or modal id is no target, an empty panel id is one; an active derivation overrides the shell; the shell is the target document\'s.',
    plugins: [
      withShell,
      commands([
        vdef('menu', { menu: 'file' }),
        vdef('panel', { panel: 'search' }),
        vdef('panel-object', { panel: { id: 'thumbs', exclusive: 'left' } }),
        vdef('modal', { modal: 'print' }),
        vdef('menu-and-panel', { menu: 'file', panel: 'search' }),
        vdef('panel-and-modal', { panel: 'thumbs', modal: 'print' }),
        vdef('empty-menu', { menu: '', panel: 'search' }),
        vdef('empty-modal', { modal: '' }),
        vdef('empty-panel', { panel: '' }),
        vdef('override', { menu: 'file', active: no('override.active') }),
        vdef('none'),
      ]),
    ],
    engine: { documents: { a: doc(), b: doc() } },
    steps: [
      cmd('resolve', 'menu'),
      start,
      open('a'),
      open('b'),
      setActive('a'),
      ...['menu', 'panel', 'panel-object', 'modal', 'menu-and-panel', 'panel-and-modal', 'empty-menu', 'empty-modal', 'empty-panel', 'override', 'none'].map((id) => cmd('resolve', id)),
      shell('openMenu', ['file']),
      shell('open', ['search']),
      shell('open', ['print']),
      shell('open', ['']),
      shell('open', ['thumbs']),
      ...['menu', 'panel', 'panel-object', 'modal', 'menu-and-panel', 'panel-and-modal', 'empty-menu', 'empty-modal', 'empty-panel', 'override', 'none'].map((id) => cmd('resolve', id)),
      cmd('resolve', 'menu', 'b'),
      cmd('resolve', 'panel', 'b'),
      shell('closeMenu', ['file']),
      cmd('resolve', 'menu-and-panel'),
      cmd('resolve', 'menu', 'zzz'),
    ],
  },
  {
    name: 'execute-routing',
    covers:
      'Without run, execute routes a surface target through the target document\'s shell: toggleMenu for a menu, toggle with the panel\'s exclusive tag (or none), toggle exclusive "modal" for a modal; a menu wins over a panel and a panel over a modal; empty menu and modal ids are no target. Without a shell, or a target, nothing happens.',
    plugins: [
      withShell,
      commands([
        vdef('menu', { menu: 'file' }),
        vdef('panel', { panel: 'search' }),
        vdef('panel-object', { panel: { id: 'thumbs', exclusive: 'left' } }),
        vdef('panel-object-bare', { panel: { id: 'outline' } }),
        vdef('modal', { modal: 'print' }),
        vdef('menu-and-panel', { menu: 'view', panel: 'search' }),
        vdef('panel-and-modal', { panel: 'layers', modal: 'print' }),
        vdef('empty-menu', { menu: '', panel: 'search' }),
        vdef('empty-modal', { modal: '' }),
        vdef('empty-panel', { panel: '' }),
        vdef('none'),
      ]),
    ],
    engine: { documents: { a: doc(), b: doc() } },
    steps: [
      cmd('execute', 'menu'),
      start,
      cmd('execute', 'panel'),
      open('a'),
      open('b'),
      setActive('a'),
      cmd('execute', 'menu'),
      cmd('execute', 'menu'),
      cmd('execute', 'panel'),
      cmd('execute', 'panel-object'),
      cmd('execute', 'panel-object-bare'),
      cmd('execute', 'modal'),
      cmd('execute', 'menu-and-panel'),
      cmd('execute', 'panel-and-modal'),
      cmd('execute', 'empty-menu'),
      cmd('execute', 'empty-modal'),
      cmd('execute', 'empty-panel'),
      cmd('execute', 'none'),
      cmd('execute', 'missing'),
      cmd('execute', 'panel-object', 'b'),
      cmd('execute', 'menu', 'zzz'),
      shell('openSurfaces', [], 'a'),
      shell('openSurfaces', [], 'b'),
    ],
  },
  {
    name: 'execute-guards-and-run',
    covers:
      'execute resolves first (every derivation runs), then refuses a disabled or hidden command; otherwise it calls run with a fresh CommandCtx and skips the surface routing. A run that throws throws out of execute; one that rejects or stays pending is not awaited.',
    plugins: [
      withShell,
      commands([
        vdef('disabled', { enabled: no('disabled.enabled'), run: fn('disabled.run') }),
        vdef('hidden', { visible: no('hidden.visible'), run: fn('hidden.run') }),
        vdef('runs', { menu: 'file', run: fn('runs.run', { returns: 'ignored' }) }),
        vdef('throws', { run: boom('throws.run', 'run failed') }),
        vdef('rejects', { run: fn('rejects.run', { rejects: { name: 'Error', message: 'async run failed' } }) }),
        vdef('defers', { run: fn('defers.run', { defer: 'run' }) }),
        vdef('resolves', { run: fn('resolves.run', { resolves: 1 }) }),
      ]),
    ],
    engine: { documents: { a: doc(), b: doc() } },
    steps: [
      start,
      open('a'),
      open('b'),
      cmd('execute', 'disabled'),
      cmd('execute', 'hidden'),
      cmd('execute', 'runs'),
      cmd('execute', 'runs', 'a'),
      shell('openMenus', [], 'a'),
      cmd('execute', 'throws'),
      cmd('execute', 'rejects'),
      cmd('execute', 'defers'),
      { op: 'settle', label: 'run', value: 'late' },
      cmd('execute', 'resolves'),
    ],
  },
  {
    name: 'command-ctx',
    covers:
      'What a command function can do with its CommandCtx: get resolves a document-scoped token for the target document and throws the kernel\'s refusal when it cannot; tryGet answers null instead; workspace tokens resolve whatever the target; an empty target falls back to the active document; core() is the kernel\'s core state.',
    plugins: [
      withI18n,
      withShell,
      { factory: 'probe', config: { id: 'broken', scope: 'document', token: 'broken', capabilityThrows: { name: 'Error', message: 'broken capability' } } },
      commands([
        vdef('probe', {
          run: fn('probe.run', {
            uses: [
              { get: 'shell', call: 'isOpen', args: ['left'] },
              { tryGet: 'shell', call: 'openMenus' },
              { get: 'shell' },
              { get: 'i18n', call: 'locale' },
              { tryGet: 'i18n', call: 't', args: ['commands.zoomIn'] },
              { get: 'nope' },
              { tryGet: 'nope' },
              { get: 'broken' },
              { tryGet: 'broken' },
              { get: 'shell', call: 'open', args: ['left', { exclusive: 'side' }] },
              { core: 'activeId' },
              { core: 'order' },
            ],
          }),
        }),
      ]),
    ],
    engine: {
      documents: { a: doc(), b: doc(), slow: doc() },
      responses: [{ path: 'open', args: [{ kind: 'bytes', id: 'slow', bytes: { $bytes: '' } }, {}], defer: 'open-slow' }],
    },
    steps: [
      cmd('execute', 'probe'),
      start,
      open('a'),
      open('b'),
      cmd('execute', 'probe'),
      cmd('execute', 'probe', 'a'),
      cmd('execute', 'probe', ''),
      cmd('execute', 'probe', 'zzz'),
      open('slow'),
      cmd('execute', 'probe', 'slow'),
      { op: 'settle', label: 'open-slow', value: { $document: 'slow' } },
      cmd('execute', 'probe', 'slow'),
    ],
  },
  {
    name: 'follows-the-active-document',
    covers: 'An unaddressed resolve or execute binds to whichever document is active at the call, and to none once the last one closes.',
    plugins: [withShell, commands([vdef('panel', { panel: 'search', visible: yes('visible') })])],
    engine: { documents: { a: doc(), b: doc() } },
    steps: [
      start,
      open('a'),
      open('b'),
      cmd('execute', 'panel'),
      cmd('resolve', 'panel'),
      setActive('a'),
      cmd('resolve', 'panel'),
      cmd('execute', 'panel'),
      cmd('resolve', 'panel'),
      close('a'),
      cmd('resolve', 'panel'),
      close('b'),
      cmd('resolve', 'panel'),
      cmd('execute', 'panel'),
    ],
  },
  {
    name: 'search',
    covers:
      'search trims and lowercases the query, then keeps visible commands, in registry order, whose lowercased label contains it or whose id (not lowercased) does; an empty query keeps every visible command; every command is resolved, hidden ones included.',
    plugins: [
      commands([
        vdef('zoom:in', { labelKey: 'Zoom In' }),
        vdef('Zoom:Out', { labelKey: 'Shrink' }),
        vdef('panel:search', { labelKey: 'Find' }),
        vdef('hidden', { labelKey: 'Zoom Hidden', visible: no('hidden.visible') }),
        vdef('gated', { labelKey: 'Zoom Gated', categories: ['pro'] }),
        vdef('ümlaut', { labelKey: 'ÜBER Straße' }),
      ], ['pro']),
    ],
    steps: [
      cmd('search', ''),
      cmd('search', '   '),
      cmd('search', 'zoom'),
      cmd('search', '  ZOOM\t\n'),
      cmd('search', 'zoom:o'),
      cmd('search', 'Zoom:O'),
      cmd('search', 'Out'),
      cmd('search', ':'),
      cmd('search', 'über'),
      cmd('search', 'STRASSE'),
      cmd('search', 'straße'),
      cmd('search', 'nothing'),
      cmd('search', '﻿find '),
    ],
  },
  {
    name: 'match-stroke',
    covers:
      'matchStroke returns the first command, in registry order, any of whose shortcuts matches the stroke on the given platform, and null when none does; it ignores enablement.',
    plugins: [
      commands([
        vdef('palette', { shortcut: 'Mod+K', enabled: no('palette.enabled') }),
        vdef('zoom:in', { shortcut: ['Mod+=', 'Mod+Shift+=', 'Ctrl+NumpadAdd'] }),
        vdef('other-k', { shortcut: 'Ctrl+K' }),
        vdef('escape', { shortcut: 'Escape' }),
        vdef('none'),
      ]),
    ],
    steps: [
      cmd('matchStroke', stroke('k', { metaKey: true }), mac),
      cmd('matchStroke', stroke('k', { ctrlKey: true }), mac),
      cmd('matchStroke', stroke('k', { ctrlKey: true }), pc),
      cmd('matchStroke', stroke('K', { ctrlKey: true, shiftKey: true }), pc),
      cmd('matchStroke', stroke('=', { ctrlKey: true }), pc),
      cmd('matchStroke', stroke('+', { ctrlKey: true, shiftKey: true }), pc),
      cmd('matchStroke', stroke('=', { ctrlKey: true, shiftKey: true }), pc),
      cmd('matchStroke', stroke('+', { code: 'NumpadAdd', ctrlKey: true }), mac),
      cmd('matchStroke', stroke('Escape', { code: 'Escape' }), pc),
      cmd('matchStroke', stroke('Escape', { shiftKey: true }), pc),
      cmd('matchStroke', stroke('x'), pc),
      cmd('unregister', 'palette'),
      cmd('matchStroke', stroke('k', { ctrlKey: true }), pc),
      cmd('register', vdef('palette', { shortcut: 'Mod+K' })),
      cmd('matchStroke', stroke('k', { ctrlKey: true }), pc),
      cmd('matchStroke', stroke('k', { metaKey: true }), mac),
    ],
  },
  {
    name: 'registered-functions',
    covers: 'A command registered through the capability carries its functions like a configured one: derivations and run are called with the CommandCtx.',
    plugins: [{ factory: 'commands' }],
    engine: { documents: { a: doc() } },
    steps: [
      cmd('register', {
        id: 'live',
        labelKey: 'live',
        icon: 'bolt',
        iconAccent: fn('live.accent', { returns: { primary: 'red' } }),
        active: yes('live.active'),
        enabled: fn('live.enabled', { seq: [{ returns: true }, { returns: false }] }),
        visible: yes('live.visible'),
        run: fn('live.run'),
      }),
      start,
      open('a'),
      cmd('execute', 'live'),
      cmd('execute', 'live'),
      cmd('resolve', 'live'),
    ],
  },
  {
    name: 're-entrant-registry',
    covers:
      'A command function may use the commands capability itself: a run that executes another command, and a visible derivation that, mid-search, unregisters a command not yet visited (skipped) and registers a new one (visited, as a Map iteration does); the second time, the registration is a duplicate.',
    plugins: [
      commands([
        vdef('first', {
          labelKey: 'x first',
          visible: fn('first.visible', {
            returns: true,
            uses: [
              { get: 'commands', call: 'unregister', args: ['second'] },
              { get: 'commands', call: 'register', args: [{ id: 'late', labelKey: 'x late', visible: { $fn: 'late.visible', returns: true } }] },
              { get: 'commands', call: 'ids' },
            ],
          }),
        }),
        vdef('second', { labelKey: 'x second', visible: yes('second.visible') }),
        vdef('third', { labelKey: 'x third', visible: yes('third.visible') }),
        vdef('chain', { run: fn('chain.run', { uses: [{ get: 'commands', call: 'execute', args: ['target'] }] }) }),
        vdef('target', { run: fn('target.run') }),
      ]),
    ],
    steps: [cmd('search', 'x'), cmd('ids'), cmd('search', 'x'), cmd('execute', 'chain')],
  },
  {
    name: 'after-destroy',
    covers:
      'A capability reference kept past destroy still reads and changes the registry and matches strokes, but everything that reads the slice throws the TypeError of reading a member of the missing state; category intents dispatch into nothing.',
    plugins: [withI18n, withShell, commands([vdef('menu', { labelKey: 'commands.zoomIn', menu: 'file', shortcut: 'Mod+M', run: fn('menu.run', { uses: [{ tryGet: 'i18n', call: 'locale' }, { core: 'activeId' }] }) })])],
    engine: { documents: { a: doc() } },
    steps: [
      { op: 'call', target: 'kernel', method: 'capability', args: [{ $token: 'commands' }] },
      start,
      open('a'),
      { op: 'call', target: { ref: 4 }, args: ['menu'] },
      { op: 'call', target: { ref: 7 }, args: ['menu'] },
      destroy,
      { op: 'call', target: { ref: 3 } },
      { op: 'call', target: { ref: 2 }, args: ['menu'] },
      { op: 'call', target: { ref: 6 }, args: ['menu'] },
      { op: 'call', target: { ref: 8 }, args: [stroke('m', { ctrlKey: true }), pc] },
      { op: 'call', target: { ref: 0 }, args: [vdef('late')] },
      { op: 'call', target: { ref: 1 }, args: ['menu'] },
      { op: 'call', target: { ref: 3 } },
      { op: 'call', target: { ref: 4 }, args: ['late'] },
      { op: 'call', target: { ref: 4 }, args: ['missing'] },
      { op: 'call', target: { ref: 7 }, args: ['late'] },
      { op: 'call', target: { ref: 7 }, args: ['missing'] },
      { op: 'call', target: { ref: 5 }, args: [''] },
      { op: 'call', target: { ref: 9 } },
      { op: 'call', target: { ref: 10 }, args: ['x'] },
      { op: 'call', target: { ref: 11 }, args: ['x'] },
      { op: 'call', target: { ref: 12 }, args: ['x'] },
      { op: 'call', target: { ref: 13 }, args: [['x']] },
    ],
  },
];

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'plugins/commands/functions.json',
    describes:
      'packages/plugin/commands: commandsReducer and initialCommandsState (`same` is reference identity), resolvedCommandsEqual, registerCommand, and what commandsPlugin(config) builds.',
    functions: {
      commandsReducer: reduce,
      initialCommandsState: () => initialCommandsState,
      resolvedCommandsEqual,
      resolvedCommandsEqualSelf: equalSelf,
      registerCommand: register,
      commandsPlugin: plugin,
    },
    cases: [...reducerCases, ...equalCases, ...registerCases, ...pluginCases],
  },
  { kind: 'scenario', file: 'plugins/commands/plugin.json', describes: 'packages/plugin/commands: the workspace capability.', cases },
];
