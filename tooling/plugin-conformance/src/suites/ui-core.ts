/**
 * `packages/core/ui`: the chrome schema, the fit solver, the overflow
 * projection, the schema transforms and the shortcut parser. Replayed by
 * `crates/plugins/ui-core/tests/conformance.rs`.
 *
 * Every function here is pure. Where the TypeScript takes a callback, the
 * suite takes the callback's answers as data and builds the callback:
 *
 *   FitMetrics         a table (`metricsFrom`): per-key, per-variant widths
 *                      with per-variant and per-group defaults. A width the
 *                      table lacks is `undefined`, a missing measurement.
 *   ResolveMenuTarget  a table from command id to `{menu?}` or null; an id
 *                      the table lacks resolves to null.
 *   filterBar's keep   the list of unit keys to keep.
 *   projectStrip's     the list of visible command ids.
 *     visible
 *
 * The shortcut module reads no platform global: `isMac` is already an input.
 * Lookups use `Object.hasOwn`, so a key like "constructor" is a missing
 * entry and not `Object.prototype`'s member.
 */
import {
  DEFAULT_IMPORTANCE,
  PINNED,
  custom as upstreamCustom,
  filterBar,
  formatShortcut,
  group as upstreamGroup,
  item,
  matchShortcut,
  normalizeBar,
  parseShortcut,
  projectOverflow,
  projectShed,
  projectStrip,
  solve,
  type BarSchema,
  type FitMetrics,
  type FitResult,
  type KeyStroke,
  type NormalizedBar,
  type NormalizedGroup,
  type ParsedShortcut,
  type ResolveMenuTarget,
} from '../../../../packages/core/ui/src/index';

import { removedUpstream } from '../removed';
import type { FunctionCase, Suite } from '../vectors';

// d65fe1380 replaced the chrome schema with the layout model ("The chrome
// schema helpers are removed"): `defineChrome`, `validateChrome`, `addItem`,
// `removeItems`, `replaceItem` and the `chromeHelpers` bundle are gone. The
// cases keep their schema inputs.
type ChromeSchema = Record<string, unknown>;
const defineChrome = removedUpstream('defineChrome', 'd65fe1380');
const validateChrome = removedUpstream('validateChrome', 'd65fe1380');
const addItem = removedUpstream('addItem', 'd65fe1380');
const removeItems = removedUpstream('removeItems', 'd65fe1380');
const replaceItem = removedUpstream('replaceItem', 'd65fe1380');
const chromeHelpers = removedUpstream('chromeHelpers', 'd65fe1380');

// cdf4586c7 reordered the builders: `group(id, items, options?)` and
// `custom(slot, terminal?, options?)`, where they took `(id, options, items?)`
// and `(slot, options)`. The cases' bars are built with the old builders,
// copied here as they were, and the `group` and `custom` cases reach
// upstream's.
type GroupOptions = Record<string, unknown> & { items?: readonly unknown[] };
function group(id: string, optsOrItems: readonly unknown[] | GroupOptions, items?: readonly unknown[]): never {
  if (Array.isArray(optsOrItems)) return { id, items: optsOrItems } as never;
  const { items: optItems, ...opts } = optsOrItems as GroupOptions;
  return { id, ...opts, items: items ?? optItems ?? [] } as never;
}
const custom = (slot: string, opts: Record<string, unknown>): never => ({ slot, ...opts }) as never;

// ── callback tables ──────────────────────────────────────────────────────────

type Table<T> = Readonly<Record<string, T>>;

const own = <T>(table: Table<T> | undefined, key: string): T | undefined =>
  table !== undefined && Object.hasOwn(table, key) ? table[key] : undefined;

interface MetricsSpec {
  /** Width of a unit key at a variant. */
  readonly units?: Table<Table<number>>;
  /** Width of any unit at a variant, when `units` has none. */
  readonly variants?: Table<number>;
  readonly collapsed?: Table<number>;
  readonly collapsedDefault?: number;
  readonly triggers?: Table<number>;
  readonly triggerDefault?: number;
  readonly overflowTrigger: number;
  readonly gap: number;
  readonly separator: number;
}

function metricsFrom(spec: MetricsSpec): FitMetrics {
  return {
    unit: (key, variant) => own(own(spec.units, key), variant) ?? own(spec.variants, variant),
    groupCollapsed: (id) => own(spec.collapsed, id) ?? spec.collapsedDefault,
    groupTrigger: (id) => own(spec.triggers, id) ?? spec.triggerDefault,
    overflowTrigger: spec.overflowTrigger,
    gap: spec.gap,
    separator: spec.separator,
  };
}

type ResolveTable = Table<{ menu?: string } | null>;

const resolveFrom =
  (table: ResolveTable): ResolveMenuTarget =>
  (id) =>
    own(table, id) ?? null;

// ── wrappers ─────────────────────────────────────────────────────────────────

const schemaFunctions = {
  constants: () => ({ PINNED, DEFAULT_IMPORTANCE }),
  item,
  custom: upstreamCustom,
  group: upstreamGroup,
  normalizeBar,
  filterBar: (bar: NormalizedBar, keepKeys: readonly string[]) =>
    filterBar(bar, (unit) => keepKeys.includes(unit.key)),
  defineChrome,
  validateChrome,
};

const solverFunctions = {
  solve: (bar: NormalizedBar, metrics: MetricsSpec, width: number) =>
    solve(bar, metricsFrom(metrics), width),
  /** `normalizeBar` then `solve`, for cases written as authored schema. */
  solveSchema: (bar: BarSchema, metrics: MetricsSpec, width: number) =>
    solve(normalizeBar(bar), metricsFrom(metrics), width),
};

const projectionFunctions = {
  projectOverflow: (bar: NormalizedBar, fit: FitResult, resolve: ResolveTable) =>
    projectOverflow(bar, fit, resolveFrom(resolve)),
  projectShed: (group: NormalizedGroup, fit: FitResult, resolve: ResolveTable) =>
    projectShed(group, fit, resolveFrom(resolve)),
  projectStrip: (bar: NormalizedBar, visible: readonly string[]) =>
    projectStrip(bar, (command) => visible.includes(command)),
  /** The renderer's pipeline: normalize, solve, then both projections. */
  solveAndProject: (bar: BarSchema, metrics: MetricsSpec, width: number, resolve: ResolveTable) => {
    const normalized = normalizeBar(bar);
    const fit = solve(normalized, metricsFrom(metrics), width);
    const shed: Record<string, unknown> = {};
    for (const section of normalized.sections)
      for (const g of section.groups) {
        const rows = projectShed(g, fit, resolveFrom(resolve));
        if (rows.length) shed[g.id] = rows;
      }
    return { fit, overflow: projectOverflow(normalized, fit, resolveFrom(resolve)), shed };
  },
};

const transformFunctions = { addItem, removeItems, replaceItem };

const shortcutFunctions = {
  parseShortcut,
  matchShortcut: (parsed: ParsedShortcut, stroke: KeyStroke, opts: { isMac: boolean }) =>
    matchShortcut(parsed, stroke, opts),
  /** `parseShortcut` then `matchShortcut`. */
  matchText: (shortcut: string, stroke: KeyStroke, isMac: boolean) =>
    matchShortcut(parseShortcut(shortcut), stroke, { isMac }),
  formatShortcut,
};

const helperFunctions = {
  /** The bundle's member names, in order: it is public API. */
  keys: chromeHelpers,
  /** Each member is the module's own function, not a copy. */
  identity: chromeHelpers,
  /** Call a member by name, as a `chrome: (base, h) => …` transform would. */
  call: chromeHelpers,
};

// ── shared fixtures ──────────────────────────────────────────────────────────

const M: MetricsSpec = {
  variants: { 'icon+label': 100, icon: 40, label: 80, default: 60, wide: 120, narrow: 30, inline: 150 },
  collapsedDefault: 50,
  triggerDefault: 36,
  overflowTrigger: 40,
  gap: 10,
  separator: 20,
};
const withM = (over: Partial<MetricsSpec>): MetricsSpec => ({ ...M, ...over });

const IL = ['icon+label', 'icon'] as const;
const ILL = ['icon+label', 'label', 'icon'] as const;

/** Plain groups, variant ladders, mixed importance, two sections. */
const barPlain: BarSchema = {
  id: 'plain',
  sections: {
    start: [
      group('a', [item('a1', { variants: IL }), item('a2', { variants: ILL, importance: 2 }), 'a3']),
      group('b', { importance: 4 }, [item('b1', { variants: IL, importance: 4 }), item('b2', { importance: 1 })]),
    ],
    end: [group('c', [item('c1', { variants: IL, importance: 5 }), item('c2', { variants: IL })])],
  },
};

/** A shed tab group, a plain group, a pinned group, three sections. */
const barShed: BarSchema = {
  id: 'shed',
  sections: {
    start: [group('nav', [item('nav:back', { importance: 5 }), 'nav:fwd'])],
    center: [
      group('modes', { role: 'tabs', shed: true, labelKey: 'modes' }, [
        item('mode:view', { variants: ['label'] }),
        item('mode:annotate', { variants: ['label'], importance: 4 }),
        item('mode:shapes', { variants: ['label'], importance: 2 }),
        item('mode:form', { variants: ['label'] }),
      ]),
    ],
    end: [group('tail', ['t1', item('t2', { importance: 2 })])],
  },
};

/** Collapse menu, collapse select, shed+collapse, custom items. */
const barCollapse: BarSchema = {
  id: 'collapse',
  sections: {
    start: [
      group('zoom', { collapse: 'menu', importance: 2, labelKey: 'zoom' }, [
        custom('zoom-strip', { variants: ['inline', 'narrow'], terminal: 'zoom:menu' }),
        item('zoom:in', { variants: IL }),
      ]),
    ],
    center: [
      group('tabs', { role: 'tabs', shed: true, collapse: 'select' }, [
        item('tab:1', { variants: ['label'] }),
        item('tab:2', { variants: ['label'] }),
        item('tab:3', { variants: ['label'], importance: 1 }),
      ]),
    ],
    end: [
      group('tools', { collapse: 'menu', importance: 4 }, ['tool:a', 'tool:b']),
      group('pinned', { importance: 5, collapse: 'menu' }, [item('pin:a', { importance: 5 }), 'pin:b']),
    ],
  },
};

/** Every section, customs with default ladders, a plain group per section. */
const barCustom: BarSchema = {
  id: 'custom',
  sections: {
    start: [group('s', [custom('slot:a', { terminal: 'cmd:a' }), custom('slot:b', { variants: ['wide', 'default', 'narrow'], terminal: 'cmd:b', importance: 2 })])],
    center: [group('m', [item('m1', { variants: ['label', 'icon'] }), 'm2'])],
    end: [group('e', { shed: true }, ['e1', 'e2', item('e3', { importance: 1 })])],
  },
};

/** Widths 0..max in `step`, plus the boundaries just around `extras`. */
function widths(max: number, step: number, extras: readonly number[] = []): number[] {
  const out = new Set<number>();
  for (let w = 0; w <= max; w += step) out.add(w);
  for (const e of extras) {
    out.add(e - 1);
    out.add(e);
    out.add(e + 1);
  }
  return [...out].filter((w) => w >= 0).sort((a, b) => a - b);
}

function sweep(name: string, bar: BarSchema, metrics: MetricsSpec, ws: readonly number[]): FunctionCase[] {
  return ws.map((w) => ({ name: `${name}@${w}`, fn: 'solveSchema', args: [bar, metrics, w] }));
}

// ── schema ───────────────────────────────────────────────────────────────────

const chromeBase: ChromeSchema = {
  bars: {
    main: {
      id: 'main',
      sections: {
        start: [group('zoom', [custom('zoom-controls', { terminal: 'zoom:menu' })])],
        center: [group('modes', { role: 'tabs' }, [item('mode:view', { variants: ['label'] })])],
        end: [group('panels', ['panel:search', 'panel:comment'])],
      },
    },
  },
  modeBars: {
    'mode:annotate': { id: 'annotate', sections: { center: [group('markup', ['annotation:add-highlight', 'annotation:add-ink'])] } },
    'mode:shapes': { id: 'shapes', sections: { start: [group('shape', [item('shape:square', { variants: IL })])], end: [] } },
  },
  menus: {
    document: { id: 'document', sections: [{ items: ['document:download', 'document:print'] }, { labelKey: 'more', items: ['document:close'] }] },
    empty: { id: 'empty', sections: [] },
  },
  strips: {
    annotation: { id: 'annotation-strip', sections: { center: [group('actions', ['annotation:delete', custom('color', { terminal: 'annotation:color' })])] } },
  },
  frame: { toolbar: 'bottom', tabs: 'multiple', header: false },
};

const known = (...ids: string[]) => ({ $set: ids });

const schemaCases: FunctionCase[] = [
  { name: 'constants', fn: 'constants', args: [] },
  // authoring sugar
  { name: 'item-bare', fn: 'item', args: ['doc:print'] },
  { name: 'item-opts', fn: 'item', args: ['doc:print', { variants: ['icon+label', 'label'], importance: 2 }] },
  { name: 'item-empty-opts', fn: 'item', args: ['doc:print', {}] },
  { name: 'item-undefined-member', fn: 'item', args: ['x', { variants: { $undefined: true }, importance: 5 }] },
  { name: 'item-unicode', fn: 'item', args: ['ünïcødé:命令'] },
  { name: 'custom-minimal', fn: 'custom', args: ['zoom', { terminal: 'zoom:menu' }] },
  { name: 'custom-full', fn: 'custom', args: ['zoom', { variants: ['inline', 'compact'], terminal: 'zoom:menu', importance: 1 }] },
  { name: 'group-items', fn: 'group', args: ['g', ['a', item('b'), custom('s', { terminal: 't' })]] },
  { name: 'group-items-empty', fn: 'group', args: ['g', []] },
  { name: 'group-items-ignores-third', fn: 'group', args: ['g', ['a'], ['b']] },
  { name: 'group-opts-and-items', fn: 'group', args: ['g', { role: 'tabs', shed: true, labelKey: 'k' }, ['a']] },
  { name: 'group-opts-items-member', fn: 'group', args: ['g', { collapse: 'menu', items: ['from-opts'] }] },
  { name: 'group-third-beats-opts-items', fn: 'group', args: ['g', { items: ['from-opts'] }, ['from-arg']] },
  { name: 'group-opts-no-items', fn: 'group', args: ['g', { importance: 4, collapse: 'select' }] },
  { name: 'group-opts-empty', fn: 'group', args: ['g', {}] },
  { name: 'group-opts-undefined-third', fn: 'group', args: ['g', { items: ['x'] }, { $undefined: true }] },
  // normalizeBar
  { name: 'normalize-empty-sections', fn: 'normalizeBar', args: [{ id: 'b', sections: {} }] },
  { name: 'normalize-empty-arrays-drop', fn: 'normalizeBar', args: [{ id: 'b', sections: { start: [], center: [], end: [] } }] },
  { name: 'normalize-section-order', fn: 'normalizeBar', args: [{ id: 'b', sections: { end: [group('e', ['x'])], start: [group('s', ['y'])], center: [group('c', ['z'])] } }] },
  { name: 'normalize-defaults', fn: 'normalizeBar', args: [{ id: 'b', sections: { start: [group('g', ['a', item('b'), custom('s', { terminal: 't' })])] } }] },
  {
    name: 'normalize-everything',
    fn: 'normalizeBar',
    args: [
      {
        id: 'b',
        sections: {
          start: [group('g', { labelKey: 'lbl', role: 'tabs', shed: true, collapse: 'select', importance: 1 }, [item('a', { variants: ['label'], importance: 5 }), custom('s', { variants: ['x', 'y'], terminal: 't', importance: 2 })])],
          center: [group('h', { role: 'buttons', shed: false, collapse: 'menu' }, [])],
        },
      },
    ],
  },
  { name: 'normalize-plain-bar', fn: 'normalizeBar', args: [barPlain] },
  { name: 'normalize-collapse-bar', fn: 'normalizeBar', args: [barCollapse] },
  { name: 'normalize-same-command-two-groups', fn: 'normalizeBar', args: [{ id: 'b', sections: { start: [group('g', ['a'])], end: [group('h', ['a'])] } }] },
  { name: 'normalize-slot-and-command-same-name', fn: 'normalizeBar', args: [{ id: 'b', sections: { start: [group('g', ['a', custom('b', { terminal: 'a' })])] } }] },
  { name: 'normalize-slot-wins-over-command', fn: 'normalizeBar', args: [{ id: 'b', sections: { start: [group('g', [{ slot: 's', command: 'c', terminal: 't' }])] } }] },
  { name: 'reject-duplicate-unit', fn: 'normalizeBar', args: [{ id: 'b', sections: { start: [group('g', ['a', item('a', { variants: ['label'] })])] } }] },
  { name: 'reject-duplicate-unit-slot-vs-command', fn: 'normalizeBar', args: [{ id: 'b', sections: { start: [group('g', ['zoom', custom('zoom', { terminal: 'z' })])] } }] },
  { name: 'reject-duplicate-group-same-section', fn: 'normalizeBar', args: [{ id: 'b', sections: { start: [group('g', ['a']), group('g', ['b'])] } }] },
  { name: 'reject-duplicate-group-across-sections', fn: 'normalizeBar', args: [{ id: 'b', sections: { start: [group('g', ['a'])], end: [group('g', ['b'])] } }] },
  { name: 'reject-empty-command-ladder', fn: 'normalizeBar', args: [{ id: 'b', sections: { start: [group('g', [item('a', { variants: [] })])] } }] },
  { name: 'reject-empty-custom-ladder', fn: 'normalizeBar', args: [{ id: 'b', sections: { center: [group('g', [custom('s', { variants: [], terminal: 't' })])] } }] },
  { name: 'reject-empty-ladder-before-duplicate', fn: 'normalizeBar', args: [{ id: 'b', sections: { start: [group('g', ['a', item('a', { variants: [] })])] } }] },
  { name: 'reject-first-problem-in-order', fn: 'normalizeBar', args: [{ id: 'b', sections: { start: [group('g', ['a', 'a'])], center: [group('g', ['b'])] } }] },
  { name: 'reject-group-before-its-units', fn: 'normalizeBar', args: [{ id: 'b', sections: { start: [group('g', ['a'])], end: [group('g', ['a', 'a'])] } }] },
  { name: 'normalize-unicode-keys', fn: 'normalizeBar', args: [{ id: 'b', sections: { start: [group('grüppe', ['命令', custom('🔍', { terminal: 'søk' })])] } }] },
  // filterBar
  {
    name: 'filter-keeps-listed',
    fn: 'filterBar',
    args: [normalizeBar(barPlain), ['a:a1', 'a:a3', 'c:c2']],
  },
  { name: 'filter-keeps-none', fn: 'filterBar', args: [normalizeBar(barCollapse), []] },
  { name: 'filter-keeps-all', fn: 'filterBar', args: [normalizeBar(barShed), normalizeBar(barShed).sections.flatMap((s) => s.groups.flatMap((g) => g.units.map((u) => u.key)))] },
  { name: 'filter-empty-bar', fn: 'filterBar', args: [{ id: 'x', sections: [] }, ['a']] },
  // defineChrome
  { name: 'define-returns-input', fn: 'defineChrome', args: [chromeBase] },
  { name: 'define-bars-only', fn: 'defineChrome', args: [{ bars: {} }] },
  { name: 'define-rejects-bar', fn: 'defineChrome', args: [{ bars: { main: { id: 'main', sections: { start: [group('g', ['a', 'a'])] } } } }] },
  { name: 'define-rejects-mode-bar', fn: 'defineChrome', args: [{ bars: {}, modeBars: { 'mode:x': { id: 'x', sections: { end: [group('g', ['a']), group('g', ['b'])] } } } }] },
  { name: 'define-rejects-strip', fn: 'defineChrome', args: [{ bars: {}, strips: { s: { id: 's', sections: { center: [group('g', [item('a', { variants: [] })])] } } } }] },
  { name: 'define-ignores-menus', fn: 'defineChrome', args: [{ bars: {}, menus: { m: { id: 'm', sections: [{ items: ['a', 'a'] }] } } }] },
  {
    name: 'define-first-bad-bar-in-key-order',
    fn: 'defineChrome',
    args: [{ bars: { zeta: { id: 'z', sections: { start: [group('g', ['z', 'z'])] } }, alpha: { id: 'a', sections: { start: [group('g', ['a', 'a'])] } } } }],
  },
  {
    name: 'define-integer-keys-first',
    fn: 'defineChrome',
    args: [{ bars: { zeta: { id: 'z', sections: { start: [group('g', ['z', 'z'])] } }, 10: { id: 'ten', sections: { start: [group('g', ['t', 't'])] } }, 9: { id: 'nine', sections: { start: [group('g', ['n', 'n'])] } } } }],
  },
  {
    name: 'define-bars-before-mode-bars',
    fn: 'defineChrome',
    args: [{ strips: { s: { id: 's', sections: { start: [group('g', ['s', 's'])] } } }, modeBars: { m: { id: 'm', sections: { start: [group('g', ['m', 'm'])] } } }, bars: { b: { id: 'b', sections: { start: [group('g', ['b', 'b'])] } } } }],
  },
  // validateChrome
  { name: 'validate-all-known', fn: 'validateChrome', args: [chromeBase, known('zoom:menu', 'mode:view', 'panel:search', 'panel:comment', 'annotation:add-highlight', 'annotation:add-ink', 'shape:square', 'document:download', 'document:print', 'document:close', 'annotation:delete', 'annotation:color')] },
  { name: 'validate-nothing-known', fn: 'validateChrome', args: [chromeBase, known()] },
  { name: 'validate-slots-are-not-commands', fn: 'validateChrome', args: [chromeBase, known('zoom-controls', 'color', 'mode:view')] },
  { name: 'validate-bars-only', fn: 'validateChrome', args: [{ bars: { main: { id: 'main', sections: { start: [group('g', ['a', 'b'])] } } } }, known('b')] },
  { name: 'validate-empty-chrome', fn: 'validateChrome', args: [{ bars: {} }, known()] },
  { name: 'validate-duplicates-report-each', fn: 'validateChrome', args: [{ bars: { x: { id: 'x', sections: { start: [group('g', ['a'])], end: [group('h', ['a'])] } } }, menus: { m: { id: 'm', sections: [{ items: ['a', 'a'] }] } } }, known()] },
  { name: 'validate-key-order', fn: 'validateChrome', args: [{ bars: { zeta: { id: 'z', sections: { start: [group('g', ['z'])] } }, 2: { id: 'two', sections: { start: [group('g', ['two'])] } }, alpha: { id: 'a', sections: { start: [group('g', ['a'])] } }, 1: { id: 'one', sections: { start: [group('g', ['one'])] } } } }, known()] },
  { name: 'validate-throws-on-bad-bar', fn: 'validateChrome', args: [{ bars: { ok: { id: 'ok', sections: { start: [group('g', ['a'])] } }, bad: { id: 'bad', sections: { start: [group('g', ['b', 'b'])] } } } }, known()] },
  { name: 'validate-throws-on-bad-strip', fn: 'validateChrome', args: [{ bars: {}, strips: { s: { id: 's', sections: { start: [group('g', [custom('c', { variants: [], terminal: 't' })])] } } } }, known()] },
  { name: 'validate-unicode', fn: 'validateChrome', args: [{ bars: { 'бар': { id: 'b', sections: { start: [group('g', ['命令', 'known'])] } } }, menus: { 'мен': { id: 'm', sections: [{ items: ['🔍'] }] } } }, known('known')] },
  { name: 'validate-case-sensitive', fn: 'validateChrome', args: [{ bars: { b: { id: 'b', sections: { start: [group('g', ['Doc:Print'])] } } } }, known('doc:print')] },
];

// ── solver ───────────────────────────────────────────────────────────────────

const single = (items: BarSchema['sections']['start']): BarSchema => ({ id: 't', sections: { start: items } });

const solverCases: FunctionCase[] = [
  // the package's own tests, as vectors
  { name: 'wide-keeps-richest', fn: 'solveSchema', args: [single([group('g', [item('a', { variants: IL }), item('b')])]), M, 1000] },
  { name: 'variants-before-overflow', fn: 'solveSchema', args: [single([group('g', [item('a', { variants: IL }), item('b', { variants: IL })])]), M, 100] },
  { name: 'rightmost-first', fn: 'solveSchema', args: [single([group('g', [item('a', { variants: IL }), item('b', { variants: IL })])]), M, 160] },
  { name: 'importance-before-position', fn: 'solveSchema', args: [single([group('g', [item('low', { variants: IL, importance: 1 }), item('high', { variants: IL, importance: 4 })])]), M, 160] },
  { name: 'overflow-reserves-trigger', fn: 'solveSchema', args: [single([group('g', ['a', 'b', 'c'])]), M, 100] },
  { name: 'fits-without-trigger', fn: 'solveSchema', args: [single([group('g', ['a', 'b'])]), M, 100] },
  { name: 'trigger-fixpoint', fn: 'solveSchema', args: [single([group('g', ['a', 'b'])]), M, 89] },
  { name: 'exact-fit-is-a-fit', fn: 'solveSchema', args: [single([group('g', ['a', 'b'])]), M, 90] },
  { name: 'pinned-never-overflows', fn: 'solveSchema', args: [single([group('g', [item('p', { importance: 5, variants: IL }), 'x'])]), M, 0] },
  { name: 'pinned-floor-over-budget', fn: 'solveSchema', args: [single([group('g', [item('p', { importance: 5 }), item('q', { importance: 5 })])]), M, 10] },
  { name: 'pinned-group-never-collapses', fn: 'solveSchema', args: [single([group('g', { importance: 5, collapse: 'menu' }, ['a', 'b'])]), M, 0] },
  { name: 'pinned-group-plain-children-overflow', fn: 'solveSchema', args: [single([group('g', { importance: 5 }, ['a', 'b'])]), M, 0] },
  { name: 'pinned-shed-group-sheds-then-stops', fn: 'solveSchema', args: [single([group('g', { importance: 5, shed: true }, ['a', 'b', 'c'])]), M, 0] },
  { name: 'shed-floor-is-one-visible', fn: 'solveSchema', args: [single([group('g', { shed: true, importance: 5 }, ['a', 'b', 'c', 'd'])]), M, 50] },
  { name: 'shed-pinned-children-stay', fn: 'solveSchema', args: [single([group('g', { shed: true }, [item('a', { importance: 5 }), 'b', item('c', { importance: 5 })])]), M, 0] },
  { name: 'shed-then-overflow-whole', fn: 'solveSchema', args: [single([group('g', { shed: true }, ['a', 'b', 'c'])]), M, 20] },
  { name: 'shed-then-collapse', fn: 'solveSchema', args: [single([group('g', { shed: true, collapse: 'select' }, ['a', 'b', 'c'])]), M, 60] },
  { name: 'shed-then-collapse-then-overflow', fn: 'solveSchema', args: [single([group('g', { shed: true, collapse: 'select' }, ['a', 'b', 'c'])]), M, 45] },
  { name: 'collapse-waits-for-variants', fn: 'solveSchema', args: [single([group('g', { collapse: 'menu' }, [item('a', { variants: ILL }), item('b', { variants: IL })])]), M, 120] },
  { name: 'collapse-when-exhausted', fn: 'solveSchema', args: [single([group('g', { collapse: 'menu' }, [item('a', { variants: ILL }), item('b', { variants: IL })])]), M, 80] },
  { name: 'collapse-pinned-child-still-collapses', fn: 'solveSchema', args: [single([group('g', { collapse: 'menu' }, [item('a', { importance: 5 }), 'b'])]), M, 60] },
  { name: 'collapsed-then-overflows', fn: 'solveSchema', args: [single([group('g', { collapse: 'menu' }, ['a', 'b'])]), M, 30] },
  { name: 'collapsed-pinned-group-with-pinned-children', fn: 'solveSchema', args: [single([group('g', { collapse: 'menu', importance: 5 }, [item('a', { importance: 5 })])]), M, 0] },
  { name: 'plain-group-overflows-child-by-child', fn: 'solveSchema', args: [single([group('g', ['a', 'b', 'c', 'd'])]), M, 150] },
  { name: 'plain-group-empties-and-overflows', fn: 'solveSchema', args: [single([group('g', ['a', 'b']), group('h', [item('p', { importance: 5 })])]), M, 60] },
  { name: 'group-importance-vs-unit-importance', fn: 'solveSchema', args: [single([group('g', { collapse: 'menu', importance: 1 }, [item('a', { importance: 4 })]), group('h', [item('b', { importance: 2 })])]), M, 80] },
  { name: 'ties-prefer-variant-over-collapse', fn: 'solveSchema', args: [single([group('g', { collapse: 'menu', importance: 3 }, ['a']), group('h', [item('b', { variants: IL })])]), M, 100] },
  { name: 'ties-prefer-shed-over-overflow', fn: 'solveSchema', args: [single([group('g', ['a']), group('h', { shed: true }, ['b', 'c'])]), M, 130] },
  { name: 'ties-prefer-collapse-over-overflow', fn: 'solveSchema', args: [single([group('g', { collapse: 'menu' }, ['a']), group('h', ['b'])]), M, 120] },
  { name: 'ties-rightmost-across-groups', fn: 'solveSchema', args: [single([group('g', ['a']), group('h', ['b']), group('i', ['c'])]), M, 150] },
  { name: 'ties-rightmost-across-sections', fn: 'solveSchema', args: [{ id: 't', sections: { start: [group('g', ['a'])], center: [group('h', ['b'])], end: [group('i', ['c'])] } }, M, 100] },
  { name: 'separators-within-section-only', fn: 'solveSchema', args: [{ id: 't', sections: { start: [group('g', ['a']), group('h', ['b'])], end: [group('i', ['c']), group('j', ['d']), group('k', ['e'])] } }, M, 1000] },
  { name: 'shed-trigger-costs-width', fn: 'solveSchema', args: [single([group('g', { shed: true }, ['a', 'b', 'c'])]), withM({ triggers: { g: 5 } }), 100] },
  { name: 'collapsed-width-per-group', fn: 'solveSchema', args: [single([group('g', { collapse: 'menu' }, ['a', 'b']), group('h', { collapse: 'select' }, ['c', 'd'])]), withM({ collapsed: { g: 11, h: 22 } }), 60] },
  { name: 'per-key-widths', fn: 'solveSchema', args: [single([group('g', [item('a', { variants: IL }), item('b', { variants: IL })])]), withM({ units: { 'g:a': { 'icon+label': 300, icon: 20 }, 'g:b': { 'icon+label': 30 } } }), 100] },
  // incomplete measurement
  { name: 'incomplete-unit', fn: 'solveSchema', args: [single([group('g', ['a', custom('b', { variants: ['unmeasured'], terminal: 'b' })])]), M, 1000] },
  { name: 'incomplete-unit-counts-zero', fn: 'solveSchema', args: [single([group('g', ['a', custom('b', { variants: ['unmeasured', 'icon'], terminal: 'b' })])]), M, 60] },
  { name: 'incomplete-collapsed', fn: 'solveSchema', args: [single([group('g', { collapse: 'menu' }, ['a', 'b'])]), withM({ collapsedDefault: undefined }), 60] },
  { name: 'incomplete-trigger', fn: 'solveSchema', args: [single([group('g', { shed: true }, ['a', 'b', 'c'])]), withM({ triggerDefault: undefined }), 100] },
  { name: 'incomplete-only-if-consulted', fn: 'solveSchema', args: [single([group('g', { shed: true, collapse: 'menu' }, ['a', 'b'])]), withM({ triggerDefault: undefined, collapsedDefault: undefined }), 1000] },
  { name: 'nothing-measured', fn: 'solveSchema', args: [barCollapse, { overflowTrigger: 0, gap: 0, separator: 0 }, 0] },
  { name: 'nothing-measured-with-gaps', fn: 'solveSchema', args: [barCollapse, { overflowTrigger: 0, gap: 4, separator: 8 }, 0] },
  { name: 'constructor-is-not-measured', fn: 'solveSchema', args: [single([group('g', [custom('constructor', { variants: ['toString'], terminal: 'c' })])]), M, 1000] },
  // degenerate bars and budgets
  { name: 'empty-bar', fn: 'solveSchema', args: [{ id: 'e', sections: {} }, M, 0] },
  { name: 'empty-bar-negative-budget', fn: 'solveSchema', args: [{ id: 'e', sections: {} }, M, -1] },
  { name: 'empty-group-plain', fn: 'solveSchema', args: [single([group('g', [])]), M, 0] },
  { name: 'empty-group-collapses', fn: 'solveSchema', args: [single([group('g', { collapse: 'menu' }, []), group('h', ['a'])]), M, 0] },
  { name: 'empty-group-shed', fn: 'solveSchema', args: [single([group('g', { shed: true }, []), group('h', ['a'])]), M, 0] },
  { name: 'empty-group-collapse-position-zero', fn: 'solveSchema', args: [single([group('h', { collapse: 'menu' }, ['a']), group('g', { collapse: 'menu' }, [])]), withM({ collapsed: { g: 500 } }), 100] },
  { name: 'equal-position-first-considered-wins', fn: 'solveSchema', args: [single([group('h', { collapse: 'menu' }, ['a']), group('g', { collapse: 'menu' }, [])]), withM({ collapsed: { g: 500 }, units: { 'h:a': { icon: 200 } } }), 100] },
  { name: 'rank-beats-order-at-equal-importance', fn: 'solveSchema', args: [single([group('g', [item('a', { variants: IL }), 'b'])]), M, 140] },
  { name: 'nan-gap-single-unit', fn: 'solveSchema', args: [single([group('g', ['a'])]), withM({ gap: { $number: 'NaN' } as never }), 100] },
  { name: 'infinite-gap-single-unit', fn: 'solveSchema', args: [single([group('g', ['a'])]), withM({ gap: { $number: 'Infinity' } as never }), 100] },
  { name: 'infinite-separator-single-group', fn: 'solveSchema', args: [single([group('g', ['a', 'b'])]), withM({ separator: { $number: 'Infinity' } as never }), 100] },
  { name: 'negative-budget', fn: 'solveSchema', args: [barPlain, M, -1] },
  { name: 'infinite-budget', fn: 'solveSchema', args: [barPlain, M, { $number: 'Infinity' }] },
  { name: 'negative-infinite-budget', fn: 'solveSchema', args: [barShed, M, { $number: '-Infinity' }] },
  { name: 'nan-budget', fn: 'solveSchema', args: [barPlain, M, { $number: 'NaN' }] },
  { name: 'nan-trigger', fn: 'solveSchema', args: [barPlain, withM({ overflowTrigger: { $number: 'NaN' } as never }), 100] },
  { name: 'nan-gap', fn: 'solveSchema', args: [barPlain, withM({ gap: { $number: 'NaN' } as never }), 100] },
  { name: 'infinite-separator', fn: 'solveSchema', args: [barPlain, withM({ separator: { $number: 'Infinity' } as never }), 1000] },
  { name: 'infinite-unit-width', fn: 'solveSchema', args: [single([group('g', ['a', item('b', { importance: 5 })])]), withM({ units: { 'g:b': { icon: { $number: 'Infinity' } as never } } }), 1000] },
  { name: 'nan-unit-width', fn: 'solveSchema', args: [single([group('g', ['a', 'b'])]), withM({ units: { 'g:a': { icon: { $number: 'NaN' } as never } } }), 10] },
  { name: 'negative-widths', fn: 'solveSchema', args: [single([group('g', ['a', 'b', 'c'])]), withM({ variants: { icon: -40 }, gap: -1 }), -100] },
  { name: 'fractional-widths-sum-in-order', fn: 'solveSchema', args: [{ id: 't', sections: { start: [group('g', ['a', 'b']), group('h', ['c'])], end: [group('i', ['d'])] } }, withM({ variants: { icon: 0.1 }, units: { 'g:b': { icon: 0.2 }, 'h:c': { icon: 0.7 } }, gap: 0.3, separator: 0.1, overflowTrigger: 0.2 }), 1.3] },
  { name: 'fractional-widths-overflow', fn: 'solveSchema', args: [{ id: 't', sections: { start: [group('g', ['a', 'b']), group('h', ['c'])], end: [group('i', ['d'])] } }, withM({ variants: { icon: 0.1 }, units: { 'g:b': { icon: 0.2 }, 'h:c': { icon: 0.7 } }, gap: 0.3, separator: 0.1, overflowTrigger: 0.2 }), 0.9] },
  { name: 'huge-widths', fn: 'solveSchema', args: [single([group('g', ['a', 'b'])]), withM({ variants: { icon: 1e308 } }), 1.7e308] },
  // hand-built normalized bars (the solver takes any NormalizedBar)
  {
    name: 'duplicate-keys-last-assignment-wins',
    fn: 'solve',
    args: [
      {
        id: 'x',
        sections: [
          { name: 'start', groups: [{ id: 'g', role: 'buttons', shed: false, importance: 3, units: [{ kind: 'command', key: 'k', command: 'a', variants: ['icon'], importance: 3 }, { kind: 'command', key: 'k', command: 'b', variants: ['label'], importance: 5 }] }] },
          { name: 'end', groups: [{ id: 'z', role: 'buttons', shed: false, importance: 3, units: [{ kind: 'command', key: 'first', command: 'c', variants: ['icon'], importance: 3 }] }] },
        ],
      },
      M,
      90,
    ],
  },
  {
    name: 'duplicate-group-ids',
    fn: 'solve',
    args: [
      {
        id: 'x',
        sections: [
          { name: 'start', groups: [
            { id: 'g', role: 'buttons', shed: false, collapse: 'menu', importance: 3, units: [{ kind: 'command', key: 'g:a', command: 'a', variants: ['icon'], importance: 3 }] },
            { id: 'h', role: 'buttons', shed: false, importance: 3, units: [] },
            { id: 'g', role: 'buttons', shed: false, importance: 3, units: [{ kind: 'command', key: 'g:b', command: 'b', variants: ['icon'], importance: 3 }] },
          ] },
        ],
      },
      M,
      10,
    ],
  },
  {
    name: 'repeated-section-name-merges-separators',
    fn: 'solve',
    args: [
      {
        id: 'x',
        sections: [
          { name: 'start', groups: [{ id: 'a', role: 'buttons', shed: false, importance: 3, units: [{ kind: 'command', key: 'a:1', command: '1', variants: ['icon'], importance: 3 }] }] },
          { name: 'end', groups: [{ id: 'b', role: 'buttons', shed: false, importance: 3, units: [{ kind: 'command', key: 'b:1', command: '1', variants: ['icon'], importance: 3 }] }] },
          { name: 'start', groups: [{ id: 'c', role: 'buttons', shed: false, importance: 3, units: [{ kind: 'command', key: 'c:1', command: '1', variants: ['icon'], importance: 3 }] }] },
        ],
      },
      M,
      1000,
    ],
  },
  {
    name: 'importance-outside-ladder-order',
    fn: 'solve',
    args: [
      {
        id: 'x',
        sections: [
          { name: 'center', groups: [{ id: 'g', role: 'tabs', shed: true, importance: 2, units: [
            { kind: 'custom', key: 'g:s', slot: 's', terminal: 't', variants: ['wide', 'narrow'], importance: 4 },
            { kind: 'command', key: 'g:a', command: 'a', variants: ['label', 'icon'], importance: 1 },
            { kind: 'command', key: 'g:b', command: 'b', variants: ['icon'], importance: 2 },
          ] }] },
        ],
      },
      M,
      100,
    ],
  },
  // sweeps: every width through each bar's whole ladder
  ...sweep('plain', barPlain, M, widths(600, 15, [140, 90, 40])),
  ...sweep('shed', barShed, M, widths(560, 16, [218])),
  ...sweep('collapse', barCollapse, M, widths(620, 16, [50, 40, 176])),
  ...sweep('custom', barCustom, M, widths(620, 17, [60, 150])),
  ...sweep('shed-no-gaps', barShed, withM({ gap: 0, separator: 0, overflowTrigger: 25, triggerDefault: 0 }), widths(480, 20)),
  ...sweep('collapse-wide-trigger', barCollapse, withM({ overflowTrigger: 200, collapsedDefault: 5 }), widths(700, 25)),
];

// ── projection ───────────────────────────────────────────────────────────────

const projBar = normalizeBar({
  id: 'main',
  sections: {
    start: [
      group('workspace', [item('panel:sidebar', { importance: 5 }), 'page:settings']),
    ],
    center: [
      group('zoom', { labelKey: 'zoom.title' }, [custom('zoom-controls', { variants: ['inline'], terminal: 'zoom:menu' })]),
      group('tools', ['pan:toggle', 'pointer:toggle']),
      group('modes', { role: 'tabs', shed: true, labelKey: 'modes' }, ['mode:view', 'mode:annotate', 'mode:shapes']),
    ],
    end: [group('empty', [])],
  },
});
const projResolve: ResolveTable = {
  'page:settings': { menu: 'page-settings' },
  'zoom:menu': { menu: 'zoom' },
  'pan:toggle': {},
  'pointer:toggle': null,
  'mode:shapes': { menu: '' },
  'mode:annotate': { menu: 'annotate-menu' },
};
const fitOf = (units: [string, unknown][]) => ({
  units: { $map: units },
  groups: { $map: [] },
  hasOverflow: true,
  width: 0,
  complete: true,
});
const allOverflow = fitOf(projBar.sections.flatMap((s) => s.groups.flatMap((g) => g.units.map((u) => [u.key, { kind: 'overflow' }] as [string, unknown]))));
const allShed = fitOf(projBar.sections.flatMap((s) => s.groups.flatMap((g) => g.units.map((u) => [u.key, { kind: 'shed' }] as [string, unknown]))));
const modesGroup = projBar.sections[1].groups[2];

const projectionCases: FunctionCase[] = [
  { name: 'overflow-everything', fn: 'projectOverflow', args: [projBar, allOverflow, projResolve] },
  { name: 'overflow-nothing-resolves', fn: 'projectOverflow', args: [projBar, allOverflow, {}] },
  { name: 'overflow-empty-fit', fn: 'projectOverflow', args: [projBar, fitOf([]), projResolve] },
  { name: 'overflow-ignores-shed', fn: 'projectOverflow', args: [projBar, allShed, projResolve] },
  {
    name: 'overflow-partial-groups',
    fn: 'projectOverflow',
    args: [
      projBar,
      fitOf([
        ['workspace:panel:sidebar', { kind: 'variant', variant: 'icon' }],
        ['workspace:page:settings', { kind: 'overflow' }],
        ['zoom:zoom-controls', { kind: 'collapsed' }],
        ['tools:pan:toggle', { kind: 'variant', variant: 'icon' }],
        ['tools:pointer:toggle', { kind: 'overflow' }],
        ['modes:mode:view', { kind: 'shed' }],
        ['modes:mode:annotate', { kind: 'overflow' }],
        ['modes:mode:shapes', { kind: 'overflow' }],
      ]),
      projResolve,
    ],
  },
  { name: 'overflow-empty-bar', fn: 'projectOverflow', args: [{ id: 'x', sections: [] }, fitOf([]), projResolve] },
  { name: 'overflow-custom-by-terminal', fn: 'projectOverflow', args: [projBar, fitOf([['zoom:zoom-controls', { kind: 'overflow' }]]), { 'zoom-controls': { menu: 'not-this' } }] },
  { name: 'shed-rows', fn: 'projectShed', args: [modesGroup, allShed, projResolve] },
  { name: 'shed-ignores-overflow', fn: 'projectShed', args: [modesGroup, allOverflow, projResolve] },
  { name: 'shed-partial', fn: 'projectShed', args: [modesGroup, fitOf([['modes:mode:shapes', { kind: 'shed' }], ['modes:mode:view', { kind: 'variant', variant: 'icon' }], ['modes:mode:annotate', { kind: 'shed' }]]), projResolve] },
  { name: 'shed-empty-group', fn: 'projectShed', args: [projBar.sections[2].groups[0], allShed, projResolve] },
  { name: 'shed-custom-terminal', fn: 'projectShed', args: [projBar.sections[1].groups[0], allShed, projResolve] },
  { name: 'strip-all-visible', fn: 'projectStrip', args: [projBar, ['panel:sidebar', 'page:settings', 'zoom:menu', 'pan:toggle', 'pointer:toggle', 'mode:view', 'mode:annotate', 'mode:shapes']] },
  { name: 'strip-none-visible', fn: 'projectStrip', args: [projBar, []] },
  { name: 'strip-some-visible', fn: 'projectStrip', args: [projBar, ['page:settings', 'mode:shapes', 'mode:view']] },
  { name: 'strip-custom-slot-is-not-its-command', fn: 'projectStrip', args: [projBar, ['zoom-controls']] },
  { name: 'strip-custom-terminal', fn: 'projectStrip', args: [projBar, ['zoom:menu']] },
  { name: 'strip-empty-bar', fn: 'projectStrip', args: [{ id: 'x', sections: [] }, ['a']] },
  ...[0, 40, 90, 100, 130, 200, 260, 300, 400, 500].map((w) => ({
    name: `pipeline@${w}`,
    fn: 'solveAndProject',
    args: [
      {
        id: 'main',
        sections: {
          start: [group('workspace', [item('panel:sidebar', { importance: 5 }), 'page:settings'])],
          center: [
            group('zoom', { labelKey: 'zoom.title', collapse: 'menu', importance: 2 }, [custom('zoom-controls', { variants: ['inline', 'narrow'], terminal: 'zoom:menu' })]),
            group('tools', ['pan:toggle', 'pointer:toggle']),
            group('modes', { role: 'tabs', shed: true, labelKey: 'modes' }, ['mode:view', 'mode:annotate', item('mode:shapes', { importance: 2 })]),
          ],
        },
      },
      M,
      w,
      projResolve,
    ],
  })),
];

// ── transforms ───────────────────────────────────────────────────────────────

const ambiguous: ChromeSchema = {
  bars: { main: { id: 'x', sections: {} }, x: { id: 'y', sections: {} } },
  strips: { s: { id: 'x', sections: {} } },
};

const transformCases: FunctionCase[] = [
  // addItem
  { name: 'add-append', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'end', group: 'panels', item: 'acme:send' }] },
  { name: 'add-at-start', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'end', group: 'panels', item: 'acme:send', at: 0 }] },
  { name: 'add-at-middle', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'end', group: 'panels', item: item('acme:send', { variants: IL }), at: 1 }] },
  { name: 'add-at-past-end', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'end', group: 'panels', item: 'x', at: 99 }] },
  { name: 'add-at-negative', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'end', group: 'panels', item: 'x', at: -1 }] },
  { name: 'add-at-very-negative', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'end', group: 'panels', item: 'x', at: -99 }] },
  { name: 'add-at-fraction', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'end', group: 'panels', item: 'x', at: 1.9 }] },
  { name: 'add-at-negative-fraction', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'end', group: 'panels', item: 'x', at: -0.5 }] },
  { name: 'add-at-nan', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'end', group: 'panels', item: 'x', at: { $number: 'NaN' } }] },
  { name: 'add-at-infinity', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'end', group: 'panels', item: 'x', at: { $number: 'Infinity' } }] },
  { name: 'add-at-negative-infinity', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'end', group: 'panels', item: 'x', at: { $number: '-Infinity' } }] },
  { name: 'add-at-undefined', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'end', group: 'panels', item: 'x', at: { $undefined: true } }] },
  { name: 'add-new-group', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'start', group: 'acme', item: custom('acme-slot', { terminal: 'acme:go' }), at: 5 }] },
  { name: 'add-new-group-new-section', fn: 'addItem', args: [chromeBase, { bar: 'annotate', section: 'end', group: 'acme', item: 'acme:go' }] },
  { name: 'add-to-empty-section', fn: 'addItem', args: [chromeBase, { bar: 'shapes', section: 'end', group: 'acme', item: 'acme:go' }] },
  { name: 'add-by-mode-bar-key', fn: 'addItem', args: [chromeBase, { bar: 'mode:annotate', section: 'center', group: 'markup', item: 'annotation:add-arrow', at: 1 }] },
  { name: 'add-by-mode-bar-id', fn: 'addItem', args: [chromeBase, { bar: 'annotate', section: 'center', group: 'markup', item: 'annotation:add-arrow' }] },
  { name: 'add-by-strip-id', fn: 'addItem', args: [chromeBase, { bar: 'annotation-strip', section: 'center', group: 'actions', item: 'annotation:copy' }] },
  { name: 'add-by-strip-key', fn: 'addItem', args: [chromeBase, { bar: 'annotation', section: 'center', group: 'actions', item: 'annotation:copy' }] },
  { name: 'add-duplicate-item-allowed', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'end', group: 'panels', item: 'panel:search' }] },
  { name: 'add-no-bar', fn: 'addItem', args: [chromeBase, { bar: 'nope', section: 'end', group: 'g', item: 'x' }] },
  { name: 'add-no-bar-empty-chrome', fn: 'addItem', args: [{ bars: {} }, { bar: 'main', section: 'end', group: 'g', item: 'x' }] },
  { name: 'add-ambiguous', fn: 'addItem', args: [ambiguous, { bar: 'x', section: 'start', group: 'g', item: 'i' }] },
  { name: 'add-ambiguous-same-collection', fn: 'addItem', args: [{ bars: { a: { id: 'b', sections: {} }, b: { id: 'c', sections: {} } } }, { bar: 'b', section: 'start', group: 'g', item: 'i' }] },
  { name: 'add-key-matches-own-id', fn: 'addItem', args: [{ bars: { main: { id: 'main', sections: {} } } }, { bar: 'main', section: 'center', group: 'g', item: 'i' }] },
  { name: 'add-group-in-other-section', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'start', group: 'panels', item: 'x' }] },
  { name: 'add-group-in-other-section-center', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'end', group: 'modes', item: 'x' }] },
  { name: 'add-group-in-other-section-start', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'center', group: 'zoom', item: 'x' }] },
  { name: 'add-keeps-group-options', fn: 'addItem', args: [chromeBase, { bar: 'main', section: 'center', group: 'modes', item: item('mode:new', { variants: ['label'] }) }] },
  { name: 'add-first-of-two-same-id-groups', fn: 'addItem', args: [{ bars: { b: { id: 'b', sections: { start: [group('g', ['a']), group('g', ['b'])] } } } }, { bar: 'b', section: 'start', group: 'g', item: 'x' }] },
  // removeItems
  { name: 'remove-commands', fn: 'removeItems', args: [chromeBase, ['panel:search', 'document:print']] },
  { name: 'remove-slot', fn: 'removeItems', args: [chromeBase, ['zoom-controls']] },
  { name: 'remove-terminal-keeps-slot', fn: 'removeItems', args: [chromeBase, ['zoom:menu']] },
  { name: 'remove-empties-groups-and-sections', fn: 'removeItems', args: [chromeBase, ['mode:view', 'annotation:add-highlight', 'annotation:add-ink']] },
  { name: 'remove-empties-menus-keeps-key', fn: 'removeItems', args: [chromeBase, ['document:download', 'document:print', 'document:close']] },
  { name: 'remove-nothing', fn: 'removeItems', args: [chromeBase, []] },
  { name: 'remove-unknown', fn: 'removeItems', args: [chromeBase, ['nope']] },
  { name: 'remove-drops-empty-arrays', fn: 'removeItems', args: [{ bars: { b: { id: 'b', sections: { start: [], center: [group('g', [])] } } } }, []] },
  { name: 'remove-bars-only', fn: 'removeItems', args: [{ bars: { b: { id: 'b', sections: { start: [group('g', ['a', item('b'), custom('c', { terminal: 'a' })])] } } } }, ['a', 'c']] },
  { name: 'remove-object-items', fn: 'removeItems', args: [{ bars: { b: { id: 'b', sections: { end: [group('g', { shed: true }, [item('a', { importance: 1 }), 'b'])] } } } }, ['a']] },
  { name: 'remove-keeps-frame', fn: 'removeItems', args: [{ bars: {}, frame: { toolbar: 'top' } }, ['x']] },
  { name: 'remove-duplicate-ids', fn: 'removeItems', args: [chromeBase, ['panel:search', 'panel:search']] },
  // replaceItem
  { name: 'replace-with-command', fn: 'replaceItem', args: [chromeBase, 'panel:search', 'acme:search'] },
  { name: 'replace-with-item', fn: 'replaceItem', args: [chromeBase, 'document:print', item('acme:print', { variants: IL })] },
  { name: 'replace-with-custom-skips-menus', fn: 'replaceItem', args: [chromeBase, 'document:print', custom('acme', { terminal: 'acme:print' })] },
  { name: 'replace-object-item', fn: 'replaceItem', args: [chromeBase, 'mode:view', 'mode:read'] },
  { name: 'replace-does-not-touch-slots', fn: 'replaceItem', args: [chromeBase, 'zoom-controls', 'x'] },
  { name: 'replace-does-not-touch-terminals', fn: 'replaceItem', args: [chromeBase, 'zoom:menu', 'x'] },
  { name: 'replace-unknown', fn: 'replaceItem', args: [chromeBase, 'nope', 'x'] },
  { name: 'replace-everywhere', fn: 'replaceItem', args: [{ bars: { a: { id: 'a', sections: { start: [group('g', ['x', 'x'])], end: [] } } }, strips: { s: { id: 's', sections: { center: [group('h', [item('x', { importance: 2 })])] } } }, menus: { m: { id: 'm', sections: [{ items: ['x', 'y', 'x'] }] } } }, 'x', 'z'] },
  { name: 'replace-bars-only', fn: 'replaceItem', args: [{ bars: { a: { id: 'a', sections: { start: [group('g', ['x'])] } } } }, 'x', 'y'] },
  { name: 'replace-custom-without-menus', fn: 'replaceItem', args: [{ bars: { a: { id: 'a', sections: { start: [group('g', ['x'])] } } } }, 'x', custom('c', { terminal: 'x' })] },
  { name: 'replace-with-itself', fn: 'replaceItem', args: [chromeBase, 'panel:search', 'panel:search'] },
];

// ── shortcuts ────────────────────────────────────────────────────────────────

const stroke = (key: string, over: Partial<KeyStroke> = {}): KeyStroke => ({
  key,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  ...over,
});

const parseInputs = [
  'Mod+K', 'mod+k', 'MOD+K', 'Ctrl+Shift+Z', 'Control+z', 'Meta+NumpadAdd', 'Cmd+s', 'Command+S',
  'Alt+D', 'Option+d', 'Shift+Alt+D', 'Mod+Ctrl+Meta+Alt+Shift+X', 'Ctrl+=', 'Ctrl++', '+', '',
  'Shift+', 'Ctrl+Shift++', ' Ctrl + K ', 'Ctrl+ ', 'k', 'K', 'F1', 'Escape', 'ArrowLeft', 'Space',
  'Mod+Mod+K', 'Ctrl+Control+K', '++', '+++', 'Ctrl++K', 'Hyper+K', 'Ctrl+Hyper+K', 'Ctrl+K+L',
  'a+b', 'Shift+Ä', 'Shift+ß', 'Ctrl+İ', 'Ctrl+Σ', 'Alt+ΑΣ', 'Ctrl+ǅ', 'Mod+😀', 'Mod+𐐀', 'Ctrl+ﬀ',
  ' Ctrl +K', '﻿Ctrl+K', '\u0085Ctrl+K', '　Alt+ X ', 'Ctrl+\t', '\tCtrl\n+\rK',
  'Ctrl+​K', 'CTRL+ｋ', 'ＣＴＲＬ+K', 'Ctrl+NumpadAdd', 'shift+F10', 'Ctrl+Shift+Alt+Meta+Delete',
];

const formatInputs = [
  'Mod+K', 'Ctrl+K', 'Meta+K', 'Ctrl+Shift+Z', 'Mod+Shift+Z', 'Alt+F4', 'Shift+Alt+D', 'Mod+Ctrl+Meta+Alt+Shift+X',
  'Ctrl+=', 'Ctrl++', '+', 'Escape', 'arrowleft', 'NUMPADADD', 'f1', 'k', 'Shift+ß', 'Ctrl+ﬀ', 'Ctrl+ŉ',
  'Ctrl+İ', 'Alt+ǆx', 'Mod+😀', 'Mod+𐐨', 'Mod+𐐨x', 'Ctrl+éclair', 'Ctrl+ßx', 'Ctrl+Σ', 'Hyper+K', 'Mod+Meta+K',
];

const matchShortcuts = [
  'Mod+K', 'Ctrl+K', 'Meta+K', 'Mod+Shift+Z', 'Shift+Alt+D', 'Ctrl+=', 'Ctrl++', 'Ctrl+NumpadAdd', 'F1',
  'Shift+F1', 'Escape', 'k', 'Alt+ß', 'Ctrl+İ', 'Mod+😀',
];

const matchStrokes: [string, KeyStroke][] = [
  ['k', stroke('k')],
  ['K-shift', stroke('K', { shiftKey: true })],
  ['ctrl-k', stroke('k', { ctrlKey: true })],
  ['meta-k', stroke('k', { metaKey: true })],
  ['ctrl-meta-k', stroke('k', { ctrlKey: true, metaKey: true })],
  ['ctrl-alt-k', stroke('K', { ctrlKey: true, altKey: true })],
  ['meta-shift-z', stroke('z', { metaKey: true, shiftKey: true })],
  ['ctrl-shift-Z', stroke('Z', { ctrlKey: true, shiftKey: true })],
  ['alt-shift-D', stroke('D', { altKey: true, shiftKey: true, code: 'KeyD' })],
  ['ctrl-=', stroke('=', { ctrlKey: true, code: 'Equal' })],
  ['ctrl-shift-+', stroke('+', { ctrlKey: true, shiftKey: true, code: 'Equal' })],
  ['ctrl-numpad+', stroke('+', { ctrlKey: true, code: 'NumpadAdd' })],
  ['ctrl-numpad+-nocode', stroke('+', { ctrlKey: true })],
  ['f1', stroke('F1', { code: 'F1' })],
  ['shift-f1', stroke('F1', { shiftKey: true, code: 'F1' })],
  ['escape', stroke('Escape')],
  ['esc-by-code', stroke('Unidentified', { code: 'Escape' })],
  ['alt-ß', stroke('ß', { altKey: true })],
  ['alt-ẞ-shift', stroke('ẞ', { altKey: true, shiftKey: true })],
  ['ctrl-i-dot', stroke('i̇', { ctrlKey: true })],
  ['ctrl-İ', stroke('İ', { ctrlKey: true })],
  ['meta-😀', stroke('😀', { metaKey: true })],
  ['ctrl-😀', stroke('😀', { ctrlKey: true })],
  ['ctrl-empty-code', stroke('x', { ctrlKey: true, code: '' })],
];

const shortcutCases: FunctionCase[] = [
  ...parseInputs.map((s, i) => ({ name: `parse-${i}:${JSON.stringify(s)}`, fn: 'parseShortcut', args: [s] })),
  ...formatInputs.flatMap((s, i) => [
    { name: `format-mac-${i}:${JSON.stringify(s)}`, fn: 'formatShortcut', args: [s, { isMac: true }] },
    { name: `format-other-${i}:${JSON.stringify(s)}`, fn: 'formatShortcut', args: [s, { isMac: false }] },
  ]),
  ...matchShortcuts.flatMap((s) =>
    matchStrokes.flatMap(([label, k]) => [
      { name: `match-mac:${s}:${label}`, fn: 'matchText', args: [s, k, true] },
      { name: `match-other:${s}:${label}`, fn: 'matchText', args: [s, k, false] },
    ]),
  ),
  // hand-built parsed shortcuts: the matcher takes any ParsedShortcut
  { name: 'match-uppercase-parsed-key-never-matches', fn: 'matchShortcut', args: [{ key: 'K', mod: false, ctrl: true, meta: false, alt: false, shift: false }, stroke('K', { ctrlKey: true }), { isMac: false }] },
  { name: 'match-mod-and-ctrl-on-mac', fn: 'matchShortcut', args: [{ key: 'k', mod: true, ctrl: true, meta: false, alt: false, shift: false }, stroke('k', { ctrlKey: true, metaKey: true }), { isMac: true }] },
  { name: 'match-mod-and-meta-elsewhere', fn: 'matchShortcut', args: [{ key: 'k', mod: true, ctrl: false, meta: true, alt: false, shift: false }, stroke('k', { ctrlKey: true, metaKey: true }), { isMac: false }] },
  { name: 'match-long-key-by-code-case', fn: 'matchShortcut', args: [{ key: 'numpadadd', mod: false, ctrl: false, meta: false, alt: false, shift: false }, stroke('+', { code: 'NUMPADADD' }), { isMac: false }] },
  { name: 'match-short-key-never-by-code', fn: 'matchShortcut', args: [{ key: 'a', mod: false, ctrl: false, meta: false, alt: false, shift: false }, stroke('q', { code: 'a' }), { isMac: false }] },
  { name: 'match-empty-key', fn: 'matchShortcut', args: [{ key: '', mod: false, ctrl: false, meta: false, alt: false, shift: false }, stroke(''), { isMac: false }] },
  { name: 'match-two-unit-key-by-code', fn: 'matchShortcut', args: [{ key: '😀', mod: false, ctrl: false, meta: false, alt: false, shift: false }, stroke('x', { code: '😀' }), { isMac: false }] },
  { name: 'match-shift-with-two-unit-key', fn: 'matchShortcut', args: [{ key: '😀', mod: false, ctrl: false, meta: false, alt: false, shift: false }, stroke('😀', { shiftKey: true }), { isMac: false }] },
  { name: 'match-shift-with-one-unit-key', fn: 'matchShortcut', args: [{ key: 'é', mod: false, ctrl: false, meta: false, alt: false, shift: false }, stroke('É', { shiftKey: true }), { isMac: false }] },
];

// ── helpers ──────────────────────────────────────────────────────────────────

const helperCases: FunctionCase[] = [
  { name: 'keys', fn: 'keys', args: [] },
  { name: 'identity', fn: 'identity', args: [] },
  { name: 'call-item', fn: 'call', args: ['item', 'x', { importance: 1 }] },
  { name: 'call-group', fn: 'call', args: ['group', 'g', ['x']] },
  { name: 'call-group-opts', fn: 'call', args: ['group', 'g', { shed: true }, ['x']] },
  { name: 'call-custom', fn: 'call', args: ['custom', 's', { terminal: 't' }] },
  { name: 'call-define', fn: 'call', args: ['defineChrome', chromeBase] },
  { name: 'call-define-rejects', fn: 'call', args: ['defineChrome', { bars: { b: { id: 'b', sections: { start: [group('g', ['a', 'a'])] } } } }] },
  { name: 'call-add', fn: 'call', args: ['addItem', chromeBase, { bar: 'main', section: 'end', group: 'panels', item: 'acme:send' }] },
  { name: 'call-remove', fn: 'call', args: ['removeItems', chromeBase, ['panel:search']] },
  { name: 'call-replace', fn: 'call', args: ['replaceItem', chromeBase, 'panel:search', 'acme:search'] },
];

export const suites: Suite[] = [
  { kind: 'function', file: 'core/ui-schema.json', describes: 'packages/core/ui/src/schema.ts: authoring sugar, normalizeBar, filterBar, defineChrome, validateChrome.', functions: schemaFunctions, cases: schemaCases },
  { kind: 'function', file: 'core/ui-solver.json', describes: 'packages/core/ui/src/solver.ts: the fit solver over table-driven FitMetrics.', functions: solverFunctions, cases: solverCases },
  { kind: 'function', file: 'core/ui-projection.json', describes: 'packages/core/ui/src/projection.ts: overflow, shed and strip projections over table-driven resolvers.', functions: projectionFunctions, cases: projectionCases },
  { kind: 'function', file: 'core/ui-transforms.json', describes: 'packages/core/ui/src/transforms.ts: addItem, removeItems, replaceItem.', functions: transformFunctions, cases: transformCases },
  { kind: 'function', file: 'core/ui-shortcuts.json', describes: 'packages/core/ui/src/shortcuts.ts: parse, match and format, isMac as an input.', functions: shortcutFunctions, cases: shortcutCases },
  { kind: 'function', file: 'core/ui-helpers.json', describes: 'packages/core/ui/src/helpers.ts: the chromeHelpers bundle.', functions: helperFunctions, cases: helperCases },
];
