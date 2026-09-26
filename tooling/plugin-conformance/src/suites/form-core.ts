/**
 * `packages/plugin/form`'s pure modules: the model (`core/model.ts`), the
 * fill projection (`core/fill-items.ts`), the reducer, the tool table
 * (`tools.ts`) and the ISO field selection (`field-selection.ts`). Replayed by
 * `crates/plugins/form/tests/conformance.rs`.
 *
 * Models are built here with the real `update` and recorded as arguments, so
 * a case can also hand a function a model the reducer would never produce
 * (a widget index that points past its field).
 */
import type { ActionDiagnostic, SubmitIntent } from '../../../../packages/plugin/actions/src/contract';
import { fillItemForWidget, fillItems, projectWidget } from '../../../../packages/plugin/form/src/core/fill-items';
import {
  fieldByKey,
  fieldForWidget,
  fieldKeyOf,
  initialModel,
  update,
  type Model,
  type Msg,
} from '../../../../packages/plugin/form/src/core/model';
import { buildSubmitEntries, resolveFieldSelection } from '../../../../packages/plugin/form/src/field-selection';
import { formReducer, initialFormState } from '../../../../packages/plugin/form/src/model';
import { FORM_TOOL_BY_ID, FORM_TOOLS, PLACE_TAGS } from '../../../../packages/plugin/form/src/tools/definitions';

import type { FunctionCase, Suite } from '../vectors';
import {
  checkbox,
  combobox,
  FLAGS,
  listbox,
  pushbutton,
  radio,
  signature,
  snapshot,
  text,
  v3Widget,
} from './form-fixtures';

const run = (msgs: Msg[]): Model => msgs.reduce(update, initialModel());

const functions = {
  fieldKeyOf,
  initialModel,
  update,
  fieldByKey,
  fieldForWidget,
  projectWidget,
  fillItems,
  fillItemForWidget,
  initialFormState,
  formReducer,
  formTools: () => ({ tools: FORM_TOOLS, placeTags: PLACE_TAGS }),
  formToolById: (id: string) => FORM_TOOL_BY_ID.get(id) ?? null,
  /** `listed` is a predicate; the case records it applied to every field. */
  resolveFieldSelection: (all: never[], targets: never, exclude: boolean) => {
    const { selected, listed } = resolveFieldSelection(all, targets, exclude);
    return { selected, listed: all.map((field) => listed(field)) };
  },
  /** `diagnose` is a sink; the case records what it received, in order. */
  buildSubmitEntries: (fields: never[], intent: SubmitIntent) => {
    const diagnostics: ActionDiagnostic[] = [];
    const entries = buildSubmitEntries(fields, intent, (d) => diagnostics.push(d));
    return { entries, diagnostics };
  },
};

const fc = (fn: string, label: string, ...args: unknown[]): FunctionCase => ({ name: `${fn}/${label}`, fn, args });

// ── fixtures ──────────────────────────────────────────────────────────────────

const TEXT = text(4, 'maxlen_text', 'abc', { maxLength: 5, alternateName: 'Max length' }, [[104, 3]]);
const RO = text(5, 'ro', 'x', { flags: FLAGS({ readOnly: true, raw: 1 }) }, [[105, 3]]);
const MULTI = text(6, 'notes', 'l1\nl2', { multiline: true, password: true, comb: true }, [[106, 3]]);
const CHECK = checkbox(7, 'agree', true, {}, [{ annotObjectNumber: 107, pageObjectNumber: 3, onState: 'On' }]);
const CHECK_BARE = checkbox(8, 'bare', false, {}, [[108, 3]]);
const RADIO = radio(9, 'size', [
  { annotObjectNumber: 109, pageObjectNumber: 3, onState: 'S', checked: false },
  { annotObjectNumber: 110, pageObjectNumber: 3, onState: 'M', checked: true },
  [111, 3],
]);
const COMBO = combobox(12, 'colour', 'r', { edit: true }, [[112, 3]]);
const COMBO_EMPTY = combobox(13, 'colour2', '', {}, [[113, 3]]);
const LIST = listbox(14, 'colours', ['r', 'g'], {}, [[114, 3]]);
const BUTTON = pushbutton(15, 'go', {}, [[115, 3]]);
const SIG = signature(16, 'sig', {}, [[116, 3]]);
const UNKNOWN = { ...pushbutton(17, 'mystery', {}, [[117, 3]]), family: 'unknown' };
const FQN = text(0, 'loose.field', 'q', {}, [[0, 3], [118, 4]]);
const TWO_PAGES = text(19, 'spread', 'v', {}, [[119, 3], [120, 4], [121, 3]]);

const ALL_FIELDS = [TEXT, RO, MULTI, CHECK, CHECK_BARE, RADIO, COMBO, COMBO_EMPTY, LIST, BUTTON, SIG, UNKNOWN, FQN, TWO_PAGES];
const box = (x: number, y: number, width = 20, height = 10) => ({ x, y, width, height });
const GEOM_3 = Object.fromEntries(
  [104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 119, 121].map((n, i) => [n, box(i * 10, i * 5)]),
);

const loaded = run([{ t: 'snapshot', snapshot: snapshot(ALL_FIELDS) as never }]);
const withGeom = update(loaded, { t: 'pageGeom', pageObjectNumber: 3, boxes: GEOM_3 });
const writing = update(withGeom, { t: 'writeStart', key: 'obj:4' });
const noSnapshot = update(initialModel(), { t: 'pageGeom', pageObjectNumber: 3, boxes: GEOM_3 });
/** A widget index that points at a field that does not list the widget. */
const inconsistent: Model = { ...withGeom, byWidget: { ...withGeom.byWidget, 999: 0 } };
/** A key index past the end of the field list. */
const pastEnd: Model = { ...loaded, byKey: { ...loaded.byKey, 'obj:77': 99 }, byWidget: { ...loaded.byWidget, 177: 99 } };

const modelCases: FunctionCase[] = [
  fc('fieldKeyOf', 'object-number', TEXT),
  fc('fieldKeyOf', 'fqn-when-direct', FQN),
  fc('fieldKeyOf', 'negative-object-number', { fieldObjectNumber: -1, name: 'neg' }),
  fc('initialModel', 'empty'),
  fc('update', 'snapshot-indexes-keys-and-widgets', initialModel(), { t: 'snapshot', snapshot: snapshot(ALL_FIELDS) }),
  fc('update', 'snapshot-clears-writing-keeps-geom', writing, { t: 'snapshot', snapshot: snapshot([TEXT]) }),
  fc('update', 'snapshot-later-field-wins-a-shared-widget', initialModel(), {
    t: 'snapshot',
    snapshot: snapshot([text(1, 'a', '', {}, [[50, 1]]), text(2, 'b', '', {}, [[50, 1]])]),
  }),
  fc('update', 'write-start', loaded, { t: 'writeStart', key: 'obj:4' }),
  fc('update', 'write-start-twice', writing, { t: 'writeStart', key: 'obj:5' }),
  fc('update', 'write-done-patches-and-reindexes', writing, {
    t: 'writeDone',
    key: 'obj:4',
    field: { ...TEXT, value: 'abcde', widgets: [v3Widget({ annotObjectNumber: 204, pageObjectNumber: 3 })] },
  }),
  fc('update', 'write-done-without-snapshot', initialModel(), { t: 'writeDone', key: 'obj:4', field: TEXT }),
  fc('update', 'write-done-unknown-key', writing, { t: 'writeDone', key: 'obj:404', field: TEXT }),
  fc('update', 'write-done-past-the-end-pads-with-holes', pastEnd, { t: 'writeDone', key: 'obj:77', field: TEXT }),
  fc('update', 'write-failed', writing, { t: 'writeFailed', key: 'obj:4' }),
  fc('update', 'write-failed-not-writing', loaded, { t: 'writeFailed', key: 'obj:4' }),
  fc('update', 'page-geom', loaded, { t: 'pageGeom', pageObjectNumber: 3, boxes: { 104: box(1, 2) } }),
  fc('update', 'page-geom-replaces-a-page', withGeom, { t: 'pageGeom', pageObjectNumber: 3, boxes: {} }),
  fc('update', 'clear-geom-all', withGeom, { t: 'clearGeom' }),
  fc('update', 'clear-geom-one-page', update(withGeom, { t: 'pageGeom', pageObjectNumber: 4, boxes: { 118: box(0, 0) } }), {
    t: 'clearGeom',
    pageObjectNumber: 3,
  }),
  fc('update', 'clear-geom-absent-page', withGeom, { t: 'clearGeom', pageObjectNumber: 9 }),
  fc('fieldByKey', 'found', loaded, 'obj:4'),
  fc('fieldByKey', 'fqn', loaded, 'fqn:loose.field'),
  fc('fieldByKey', 'unknown', loaded, 'obj:404'),
  fc('fieldByKey', 'no-snapshot', initialModel(), 'obj:4'),
  fc('fieldByKey', 'index-past-the-end', pastEnd, 'obj:77'),
  fc('fieldForWidget', 'found', loaded, 110),
  fc('fieldForWidget', 'direct-widget-is-not-indexed', loaded, 0),
  fc('fieldForWidget', 'unknown', loaded, 404),
  fc('fieldForWidget', 'index-past-the-end', pastEnd, 177),
];

const projectionCases: FunctionCase[] = [
  fc('projectWidget', 'text', loaded, TEXT, 104, box(1, 2)),
  fc('projectWidget', 'text-default-box', loaded, TEXT, 104),
  fc('projectWidget', 'text-flags', loaded, MULTI, 106, box(0, 0)),
  fc('projectWidget', 'read-only-is-disabled', loaded, RO, 105),
  fc('projectWidget', 'writing-is-disabled', writing, TEXT, 104),
  fc('projectWidget', 'checkbox-on-state', loaded, CHECK, 107),
  fc('projectWidget', 'checkbox-default-on-state', loaded, CHECK_BARE, 108),
  fc('projectWidget', 'checkbox-unknown-widget', loaded, CHECK, 999),
  fc('projectWidget', 'radio-checked', loaded, RADIO, 110),
  fc('projectWidget', 'radio-unchecked', loaded, RADIO, 109),
  fc('projectWidget', 'radio-bare-widget', loaded, RADIO, 111),
  fc('projectWidget', 'radio-unknown-widget', loaded, RADIO, 999),
  fc('projectWidget', 'combobox', loaded, COMBO, 112),
  fc('projectWidget', 'combobox-empty', loaded, COMBO_EMPTY, 113),
  fc('projectWidget', 'listbox', loaded, LIST, 114),
  fc('projectWidget', 'listbox-single', loaded, { ...LIST, multiSelect: false }, 114),
  fc('projectWidget', 'pushbutton', loaded, BUTTON, 115),
  fc('projectWidget', 'signature-has-no-control', loaded, SIG, 116),
  fc('projectWidget', 'unknown-has-no-control', loaded, UNKNOWN, 117),
  fc('projectWidget', 'fqn-key', loaded, FQN, 118),
  fc('fillItems', 'page-with-geometry', withGeom, 3),
  fc('fillItems', 'writing', writing, 3),
  fc('fillItems', 'page-without-geometry', withGeom, 4),
  fc('fillItems', 'no-snapshot', noSnapshot, 3),
  fc('fillItems', 'partial-geometry', update(loaded, { t: 'pageGeom', pageObjectNumber: 3, boxes: { 110: box(5, 5) } }), 3),
  fc('fillItemForWidget', 'with-geometry', withGeom, 104),
  fc('fillItemForWidget', 'without-geometry', loaded, 104),
  fc('fillItemForWidget', 'other-page-geometry', update(loaded, { t: 'pageGeom', pageObjectNumber: 4, boxes: {} }), 104),
  fc('fillItemForWidget', 'unknown', withGeom, 404),
  fc('fillItemForWidget', 'no-control', withGeom, 116),
  fc('fillItemForWidget', 'index-without-the-widget', inconsistent, 999),
];

const reducerCases: FunctionCase[] = [
  fc('initialFormState', 'empty'),
  fc('formReducer', 'set-model', initialFormState(), { type: 'SET_MODEL', model: loaded }),
  fc('formReducer', 'other-action-is-no-change', initialFormState(), { type: 'OTHER' }),
  fc('formTools', 'the-palette'),
  fc('formToolById', 'text', 'form-text'),
  fc('formToolById', 'listbox', 'form-listbox'),
  fc('formToolById', 'unknown', 'form-signature'),
];

// ── field selection ──────────────────────────────────────────────────────────

const sel = (name: string, over: Record<string, unknown> = {}) => ({
  ref: { kind: 'objectNumber', fieldObjectNumber: (over.fieldObjectNumber as number | undefined) ?? 0 },
  fieldObjectNumber: 0,
  name,
  family: 'text',
  origin: 'acroform',
  flags: FLAGS(),
  alternateName: null,
  mappingName: null,
  valueEntry: { kind: 'scalar', value: `${name}-value` },
  defaultValueEntry: { kind: 'none' },
  widgets: [],
  ...over,
});
const TREE = [
  sel('parent.c1'),
  sel('parent.c2'),
  sel('parentheses'),
  sel('other'),
  sel('byNumber', { fieldObjectNumber: 42 }),
  { ...sel('fqnRef'), ref: { kind: 'fqn', name: 'fqnRef' } },
];
const n = (name: string) => ({ kind: 'name', name });
const o = (objectNumber: number) => ({ kind: 'objectNumber', objectNumber });
const intent = (over: Partial<SubmitIntent> = {}): SubmitIntent => ({
  url: 'https://home.test/x',
  fields: null,
  exclude: false,
  includeNoValueFields: false,
  format: 'fdf',
  method: 'post',
  flagsRaw: 0,
  ...over,
});

const selectionCases: FunctionCase[] = [
  fc('resolveFieldSelection', 'null-selects-all', TREE, null, false),
  fc('resolveFieldSelection', 'null-ignores-exclude', TREE, null, true),
  fc('resolveFieldSelection', 'empty-include-selects-nothing', TREE, [], false),
  fc('resolveFieldSelection', 'empty-exclude-selects-all', TREE, [], true),
  fc('resolveFieldSelection', 'a-parent-name-selects-descendants', TREE, [n('parent')], false),
  fc('resolveFieldSelection', 'exact-name', TREE, [n('other')], false),
  fc('resolveFieldSelection', 'exclude-a-subtree', TREE, [n('parent')], true),
  fc('resolveFieldSelection', 'object-number', TREE, [o(42)], false),
  fc('resolveFieldSelection', 'object-number-never-matches-an-fqn-ref', TREE, [o(0)], false),
  fc('resolveFieldSelection', 'a-numberless-target-finds-a-numberless-ref', [...TREE, { ...sel('bare'), ref: { kind: 'objectNumber' } }], [{ kind: 'objectNumber' }], false),
  fc('resolveFieldSelection', 'mixed-targets', TREE, [o(42), n('parentheses'), { kind: 'other' }], false),
  fc('buildSubmitEntries', 'fields-absent-silently-drops-ineligible', [
    sel('plain'),
    sel('secret', { flags: FLAGS({ noExport: true }) }),
    sel('button', { family: 'pushbutton' }),
    sel('sig', { family: 'signature' }),
  ], intent()),
  fc('buildSubmitEntries', 'explicit-no-export-is-diagnosed', [sel('secret', { flags: FLAGS({ noExport: true }) }), sel('plain')], intent({ fields: [n('secret'), n('plain')] as never })),
  fc('buildSubmitEntries', 'explicit-button-and-signature-are-diagnosed', [sel('button', { family: 'pushbutton' }), sel('sig', { family: 'signature' })], intent({ fields: [n('button'), n('sig')] as never })),
  fc('buildSubmitEntries', 'exclude-mode-is-never-explicit', [sel('button', { family: 'pushbutton' }), sel('secret', { flags: FLAGS({ noExport: true }) }), sel('plain')], intent({ fields: [n('plain')] as never, exclude: true })),
  fc('buildSubmitEntries', 'unsupported-value-shape-is-always-diagnosed', [sel('weird', { valueEntry: { kind: 'unsupported' } }), sel('plain')], intent()),
  fc('buildSubmitEntries', 'unknown-value-kind-is-diagnosed', [sel('odd', { valueEntry: { kind: 'future' } })], intent()),
  fc('buildSubmitEntries', 'valueless-skipped-by-default', [sel('empty', { valueEntry: { kind: 'none' } })], intent()),
  fc('buildSubmitEntries', 'valueless-name-only-when-included', [sel('empty', { valueEntry: { kind: 'none' } })], intent({ includeNoValueFields: true })),
  fc('buildSubmitEntries', 'descendants-and-arrays', [sel('parent.c1'), sel('parent.c2', { valueEntry: { kind: 'array', values: ['a', 'b'] } }), sel('other')], intent({ fields: [n('parent')] as never })),
];

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'plugins/form-core.json',
    describes: 'packages/plugin/form: the pure model, the fill projection, the reducer, the tool table and the ISO field selection.',
    functions: functions as never,
    cases: [...modelCases, ...projectionCases, ...reducerCases, ...selectionCases],
  },
];
