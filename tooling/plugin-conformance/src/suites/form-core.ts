/** Shipping form index, prediction, projection and selection functions. */
import type {
  ActionDiagnostic,
  SubmitIntent,
} from '../../../../packages/plugin/actions/src/contract';
import type {
  FormFieldDTO,
  PdfActionTargetRef,
  DocumentEvent,
} from '@embedpdf/engine-core/runtime';
import { encodeFieldRefKey } from '@embedpdf/engine-core/runtime';
import { fillItems, projectWidget } from '../../../../packages/plugin/form/src/read/fill-items';
import {
  emptyFieldIndex,
  indexFields,
  indexLoadedFields,
  upsertFields,
  upsertWidgets,
  removeWidgets,
  foldFormEvent,
  fieldByKey,
  fieldForWidget,
  fieldKeyOf,
  widgetAt,
  widgetRowOf,
  pageWidgetsOf,
  initialFormState,
  type FieldIndex,
  type PageWidgets,
} from '../../../../packages/plugin/form/src/model';
import {
  predictFormOp,
  undoLookOf,
  fieldsNamedBy,
} from '../../../../packages/plugin/form/src/sync/predict';
import {
  buildSubmitEntries,
  resolveFieldSelection,
} from '../../../../packages/plugin/form/src/field-selection';
import { toFieldRef } from '../../../../packages/plugin/form/src/contract';
import {
  FORM_TOOL_BY_ID,
  FORM_TOOLS,
  PLACE_TAGS,
} from '../../../../packages/plugin/form/src/tools/definitions';
import type { FunctionCase, FunctionSuite } from '../vectors';
import {
  checkbox,
  combobox,
  listbox,
  pushbutton,
  radio,
  signature,
  unknown,
  snapshot,
  text,
  placed,
  ref,
  fqn,
  row,
  pr,
} from './form-current-fixtures';
import { REMOTE } from './annotation-live-fixtures';
const functions = {
  fieldKeyOf,
  emptyFieldIndex,
  indexFields,
  indexLoadedFields,
  upsertFields,
  upsertWidgets,
  removeWidgets,
  foldFormEvent,
  fieldByKey,
  fieldForWidget,
  projectWidget,
  widgetAt,
  widgetRowOf,
  pageWidgetsOf,
  initialFormState,
  predictFormOp,
  undoLookOf,
  fieldsNamedBy,
  toFieldRef,
  encodeFieldRefKey,
  // A named fixed host predicate: JSON cannot carry a function. All other arguments are unchanged.
  fillItemsAllowed: (index: FieldIndex, page: number, widgets: PageWidgets | undefined) =>
    fillItems(index, page, widgets, () => true),
  formTools: () => ({ tools: FORM_TOOLS, placeTags: PLACE_TAGS }),
  formToolById: (id: string) => FORM_TOOL_BY_ID.get(id) ?? null,
  resolveFieldSelection: (
    all: FormFieldDTO[],
    targets: PdfActionTargetRef[] | null,
    exclude: boolean,
  ) => {
    const { selected, listed } = resolveFieldSelection(all, targets, exclude);
    return { selected, listed: all.map((field) => listed(field)) };
  },
  buildSubmitEntries: (fields: FormFieldDTO[], intent: SubmitIntent) => {
    const diagnostics: ActionDiagnostic[] = [];
    return { entries: buildSubmitEntries(fields, intent, (d) => diagnostics.push(d)), diagnostics };
  },
};
function fc<K extends keyof typeof functions>(
  name: string,
  fn: K,
  args: Parameters<(typeof functions)[K]>,
  covers = 'Current typed input passed to the named shipping function',
): FunctionCase {
  return { name, fn, args, covers };
}
/** Out-of-contract inputs are explicitly named and passed unchanged. No ordinary record takes this path. */
function negative(
  name: string,
  fn: keyof typeof functions,
  args: unknown[],
  covers: string,
): FunctionCase {
  return { name, fn, args, covers };
}
const box = (x: number, y: number, width = 20, height = 10) => ({ x, y, width, height });
const TEXT = text(4, 'maxlen_text', 'abc', { maxLength: 5, alternateName: 'Max length' }, [
  placed(104, 3),
]);
const RO = text(5, 'ro', 'x', { readOnly: true }, [placed(105, 3)]);
const MULTI = text(6, 'notes', 'l1\nl2', { multiline: true, password: true, comb: true }, [
  placed(106, 3),
]);
const CHECK = checkbox(7, 'agree', true, {
  widgets: [{ ...placed(107, 3), onState: 'On', exportValue: 'On', checked: true }],
  exportValue: 'On',
  valueEntry: { kind: 'scalar', value: 'On' },
});
const CHECK_BARE = checkbox(8, 'bare', false, {
  widgets: [{ ...placed(108, 3), onState: 'Yes', exportValue: 'Yes', checked: false }],
});
const RADIO = radio(
  9,
  'size',
  [
    { ...placed(109, 3), onState: 'S', exportValue: 'S', checked: false },
    { ...placed(110, 3), onState: 'M', exportValue: 'M', checked: true },
    { ...placed(111, 3), onState: '', exportValue: '', checked: false },
  ],
  { value: 'M', valueEntry: { kind: 'scalar', value: 'M' } },
);
const COMBO = combobox(12, 'colour', 'r', { edit: true, widgets: [placed(112, 3)] });
const COMBO_EMPTY = combobox(13, 'colour2', '', { widgets: [placed(113, 3)] });
const LIST = listbox(14, 'colours', ['r', 'g'], { widgets: [placed(114, 3)] });
const BUTTON = pushbutton(15, 'go', { widgets: [placed(115, 3)] });
const SIG = signature(16, 'sig', { widgets: [placed(116, 3)] });
const UNKNOWN = unknown(17, 'mystery', { widgets: [placed(117, 3)] });
const FQN = text(0, 'loose.field', 'q', {}, [placed(0, 3), placed(118, 4)]);
const TWO_PAGES = text(19, 'spread', 'v', {}, [placed(119, 3), placed(120, 4), placed(121, 3)]);
const ALL_FIELDS = [
  TEXT,
  RO,
  MULTI,
  CHECK,
  CHECK_BARE,
  RADIO,
  COMBO,
  COMBO_EMPTY,
  LIST,
  BUTTON,
  SIG,
  UNKNOWN,
  FQN,
  TWO_PAGES,
];
const ROWS = [104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 119, 121].map(
  (n, i) => {
    const field = ALL_FIELDS.find((f) => f.widgets.some((w) => w.objectNumber === n));
    return row(n, field?.ref ?? null, 3, box(i * 10, i * 5), {
      fieldFamily: field?.family ?? 'unknown',
    });
  },
);
const loaded = indexFields(snapshot(ALL_FIELDS));
const withGeom = indexFields(snapshot(ALL_FIELDS, ROWS));
const geom = pageWidgetsOf(withGeom, 3);
const setValue = { type: 'forms.setValue' as const, field: ref(4), value: { value: 'abcde' } };
const writing = predictFormOp(withGeom, setValue);
const empty = emptyFieldIndex();
const pastEnd: FieldIndex = {
  ...loaded,
  byKey: { ...loaded.byKey, 'obj:77': 99 },
  byWidget: { ...loaded.byWidget, 177: 99 },
};
const inconsistent: FieldIndex = { ...withGeom, byWidget: { ...withGeom.byWidget, 999: 0 } };
const picture = {
  box: box(1, 2),
  look: {
    border: null,
    borderWidth: 1,
    borderStyle: 'solid' as const,
    background: null,
    color: null,
    fontFamily: null,
    fontSize: null,
    textAlign: 'left' as const,
  },
};
const unrelated: DocumentEvent = {
  type: 'annotations.deleted',
  page: pr(3),
  deleted: [],
  origin: REMOTE,
  meta: {
    affectedPages: [pr(3)],
    cacheDelta: null,
    opId: 'unrelated',
    undoable: true,
    changed: [],
  },
};
const sel = (name: string, n = 0, over: Partial<ReturnType<typeof text>> = {}) =>
  text(n, name, `${name}-value`, over, []);
const TREE = [
  sel('parent.c1'),
  sel('parent.c2'),
  sel('parentheses'),
  sel('other'),
  sel('byNumber', 42),
  sel('fqnRef'),
];
const n = (name: string): PdfActionTargetRef => ({ kind: 'name', name });
const o = (objectNumber: number): PdfActionTargetRef => ({ kind: 'objectNumber', objectNumber });
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
const cases: FunctionCase[] = [
  fc('fieldKeyOf/object-number', 'fieldKeyOf', [TEXT]),
  fc('fieldKeyOf/fqn-when-direct', 'fieldKeyOf', [FQN]),
  fc(
    'fieldKeyOf/negative-object-number',
    'fieldKeyOf',
    [text(-1, 'neg')],
    'Non-durable numbers are represented by current fqn refs',
  ),
  fc(
    'initialModel/empty',
    'emptyFieldIndex',
    [],
    'The field mirror index replaces the removed message model',
  ),
  fc(
    'update/snapshot-indexes-keys-and-widgets',
    'indexFields',
    [snapshot(ALL_FIELDS, ROWS)],
    'Indexes current field refs and complete widget rows',
  ),
  fc(
    'update/snapshot-clears-writing-keeps-geom',
    'indexLoadedFields',
    [
      writing,
      snapshot(
        [TEXT],
        ROWS.filter((r) => r.ref.kind === 'objectNumber' && r.ref.objectNumber === 104),
      ),
    ],
    'A load replaces pending values with confirmed fields and rows, advancing picture versions',
  ),
  fc(
    'update/snapshot-later-field-wins-a-shared-widget',
    'indexFields',
    [snapshot([text(1, 'a', '', {}, [placed(50)]), text(2, 'b', '', {}, [placed(50)])])],
    'The later field owns the shared widget index',
  ),
  fc(
    'update/write-start',
    'predictFormOp',
    [withGeom, setValue],
    'A held change predicts its value instead of setting a writing flag',
  ),
  fc(
    'update/write-start-twice',
    'predictFormOp',
    [writing, { type: 'forms.setValue', field: ref(5), value: { value: 'y' } }],
    'Later fills see earlier pending values',
  ),
  fc(
    'update/write-done-patches-and-reindexes',
    'upsertFields',
    [writing, [{ ...TEXT, value: 'abcde', widgets: [placed(204, 3)] }]],
    'Current confirmation reindexes changed widget refs',
  ),
  fc(
    'update/write-done-without-snapshot',
    'upsertFields',
    [empty, [TEXT]],
    'No snapshot means no field can be confirmed',
  ),
  fc(
    'update/write-done-unknown-key',
    'predictFormOp',
    [writing, { type: 'forms.setValue', field: ref(404), value: { value: 'abc' } }],
    'A missing target leaves the current index unchanged; removed writeDone key messages are not rebuilt',
  ),
  fc(
    'update/write-done-past-the-end-pads-with-holes',
    'fieldByKey',
    [pastEnd, 'obj:77'],
    'Explicit inconsistent-index negative: current lookup returns null instead of producing sparse fields',
  ),
  fc(
    'update/write-failed',
    'undoLookOf',
    [withGeom, [setValue]],
    'Current undo prediction restores field and rows; live refusal rollback is independently tested',
  ),
  fc(
    'update/write-failed-not-writing',
    'fieldsNamedBy',
    [loaded, [{ type: 'forms.setValue', field: ref(404), value: { value: 'x' } }]],
    'A missing field names no fill target',
  ),
  fc(
    'update/page-geom',
    'upsertWidgets',
    [loaded, [row(104, ref(4), 3, box(1, 2))]],
    'Geometry is a complete widget row in the field mirror',
  ),
  fc(
    'update/page-geom-replaces-a-page',
    'pageWidgetsOf',
    [indexFields(snapshot(ALL_FIELDS)), 3],
    'A fresh snapshot without rows replaces prior geometry',
  ),
  fc(
    'update/clear-geom-all',
    'pageWidgetsOf',
    [indexFields(snapshot(ALL_FIELDS)), 3],
    'There is no independent geometry cache; rows own geometry',
  ),
  fc(
    'update/clear-geom-one-page',
    'removeWidgets',
    [
      indexFields(snapshot(ALL_FIELDS, [...ROWS, row(118, FQN.ref, 4)])),
      ALL_FIELDS.flatMap((f) => f.widgets).filter((w) => w.page?.objectNumber === 3),
    ],
    'Remove only named widget rows; another page remains',
  ),
  fc(
    'update/clear-geom-absent-page',
    'removeWidgets',
    [withGeom, [placed(999, 9)]],
    'Removing absent rows preserves identity',
  ),
  fc('fieldByKey/found', 'fieldByKey', [loaded, 'obj:4']),
  fc('fieldByKey/fqn', 'fieldByKey', [loaded, 'fqn:loose.field']),
  fc('fieldByKey/unknown', 'fieldByKey', [loaded, 'obj:404']),
  fc('fieldByKey/no-snapshot', 'fieldByKey', [empty, 'obj:4']),
  fc('fieldByKey/index-past-the-end', 'fieldByKey', [pastEnd, 'obj:77']),
  fc('fieldForWidget/found', 'fieldForWidget', [loaded, 110]),
  fc('fieldForWidget/direct-widget-is-not-indexed', 'fieldForWidget', [loaded, 0]),
  fc('fieldForWidget/unknown', 'fieldForWidget', [loaded, 404]),
  fc('fieldForWidget/index-past-the-end', 'fieldForWidget', [pastEnd, 177]),
  fc('projectWidget/text', 'projectWidget', [TEXT, 104, picture, true]),
  fc(
    'projectWidget/text-default-box',
    'projectWidget',
    [TEXT, 104, { ...picture, box: box(0, 0) }, true],
    'PageWidget is now mandatory; explicit default page-space box is recorded',
  ),
  fc('projectWidget/text-flags', 'projectWidget', [
    MULTI,
    106,
    { ...picture, box: box(0, 0) },
    true,
  ]),
  fc('projectWidget/read-only-is-disabled', 'projectWidget', [RO, 105, picture, true]),
  fc(
    'projectWidget/writing-is-disabled',
    'projectWidget',
    [TEXT, 104, picture, false],
    'Current disable input is explicit fill permission; a pending write alone stays fillable, independently tested live',
  ),
  fc('projectWidget/checkbox-on-state', 'projectWidget', [CHECK, 107, picture, true]),
  fc('projectWidget/checkbox-default-on-state', 'projectWidget', [CHECK_BARE, 108, picture, true]),
  fc('projectWidget/checkbox-unknown-widget', 'projectWidget', [CHECK, 999, picture, true]),
  fc('projectWidget/radio-checked', 'projectWidget', [RADIO, 110, picture, true]),
  fc('projectWidget/radio-unchecked', 'projectWidget', [RADIO, 109, picture, true]),
  fc('projectWidget/radio-bare-widget', 'projectWidget', [RADIO, 111, picture, true]),
  fc('projectWidget/radio-unknown-widget', 'projectWidget', [RADIO, 999, picture, true]),
  fc('projectWidget/combobox', 'projectWidget', [COMBO, 112, picture, true]),
  fc('projectWidget/combobox-empty', 'projectWidget', [COMBO_EMPTY, 113, picture, true]),
  fc('projectWidget/listbox', 'projectWidget', [LIST, 114, picture, true]),
  fc('projectWidget/listbox-single', 'projectWidget', [
    { ...LIST, multiSelect: false },
    114,
    picture,
    true,
  ]),
  fc('projectWidget/pushbutton', 'projectWidget', [BUTTON, 115, picture, true]),
  fc(
    'projectWidget/signature-has-no-control',
    'projectWidget',
    [SIG, 116, picture, true],
    'Current signatures have an explicit signature control and signed state',
  ),
  fc('projectWidget/unknown-has-no-control', 'projectWidget', [UNKNOWN, 117, picture, true]),
  fc('projectWidget/fqn-key', 'projectWidget', [FQN, 118, picture, true]),
  fc(
    'fillItems/page-with-geometry',
    'fillItemsAllowed',
    [withGeom, 3, geom],
    'Fixed allowed predicate; pending values and complete page widgets are explicit current arguments',
  ),
  fc(
    'fillItems/writing',
    'fillItemsAllowed',
    [writing, 3, pageWidgetsOf(writing, 3)],
    'Fixed allowed predicate; pending values and complete page widgets are explicit current arguments',
  ),
  fc(
    'fillItems/page-without-geometry',
    'fillItemsAllowed',
    [withGeom, 4, undefined],
    'Fixed allowed predicate; pending values and complete page widgets are explicit current arguments',
  ),
  fc(
    'fillItems/no-snapshot',
    'fillItemsAllowed',
    [empty, 3, geom],
    'Fixed allowed predicate; pending values and complete page widgets are explicit current arguments',
  ),
  fc(
    'fillItems/partial-geometry',
    'fillItemsAllowed',
    [loaded, 3, { 110: { ...picture, box: box(5, 5) } }],
    'Fixed allowed predicate; pending values and complete page widgets are explicit current arguments',
  ),
  fc(
    'fillItemForWidget/with-geometry',
    'widgetRowOf',
    [withGeom, 104],
    'Current row lookup replaces removed fused fillItemForWidget; projection is covered independently',
  ),
  fc(
    'fillItemForWidget/without-geometry',
    'widgetRowOf',
    [loaded, 104],
    'Current row lookup replaces removed fused fillItemForWidget; projection is covered independently',
  ),
  fc(
    'fillItemForWidget/other-page-geometry',
    'widgetRowOf',
    [indexFields(snapshot(ALL_FIELDS, [row(118, FQN.ref, 4)])), 104],
    'Current row lookup replaces removed fused fillItemForWidget; projection is covered independently',
  ),
  fc(
    'fillItemForWidget/unknown',
    'widgetRowOf',
    [withGeom, 404],
    'Current row lookup replaces removed fused fillItemForWidget; projection is covered independently',
  ),
  fc(
    'fillItemForWidget/no-control',
    'projectWidget',
    [SIG, 116, picture, true],
    'Signatures now project an explicit signature control',
  ),
  fc(
    'fillItemForWidget/index-without-the-widget',
    'widgetRowOf',
    [inconsistent, 999],
    'Current row lookup replaces removed fused fillItemForWidget; projection is covered independently',
  ),
  fc(
    'fillItemForWidget/an-unplaced-widget-has-no-box',
    'widgetRowOf',
    [indexFields(snapshot([text(30, 'loose', 'l', {}, [placed(130, null)])])), 130],
    'Current row lookup replaces removed fused fillItemForWidget; projection is covered independently',
  ),
  fc(
    'projectWidget/signature-unsigned',
    'projectWidget',
    [SIG, 116, picture, true],
    'Current signatures have an explicit signature control and signed state',
  ),
  fc(
    'projectWidget/signature-signed',
    'projectWidget',
    [{ ...SIG, valueEntry: { kind: 'scalar', value: 'sig-dict' } }, 116, picture, true],
    'Current signatures have an explicit signature control and signed state',
  ),
  fc('widgetAt/no-geometry-for-the-page', 'widgetAt', [loaded, undefined, { x: 1, y: 1 }]),
  fc('widgetAt/nothing-under-the-point', 'widgetAt', [withGeom, geom, { x: 500, y: 500 }]),
  fc('widgetAt/the-smallest-containing-box-wins', 'widgetAt', [
    loaded,
    {
      104: { ...picture, box: box(0, 0, 100, 100) },
      105: { ...picture, box: box(10, 10, 20, 20) },
      106: { ...picture, box: box(5, 5, 50, 50) },
    },
    { x: 15, y: 15 },
  ]),
  fc('widgetAt/edges-are-inside', 'widgetAt', [
    loaded,
    { 104: { ...picture, box: box(10, 10, 20, 20) } },
    { x: 30, y: 30 },
  ]),
  fc('widgetAt/a-box-without-a-field-is-skipped', 'widgetAt', [
    loaded,
    { 999: { ...picture, box: box(0, 0, 5, 5) }, 104: { ...picture, box: box(0, 0, 50, 50) } },
    { x: 2, y: 2 },
  ]),
  fc('widgetAt/equal-areas-keep-the-lower-object-number', 'widgetAt', [
    loaded,
    { 104: { ...picture, box: box(0, 0, 20, 20) }, 105: { ...picture, box: box(10, 10, 20, 20) } },
    { x: 15, y: 15 },
  ]),
  fc('widgetAt/a-larger-box-later-loses', 'widgetAt', [
    loaded,
    { 105: { ...picture, box: box(10, 10, 20, 20) }, 106: { ...picture, box: box(0, 0, 50, 50) } },
    { x: 15, y: 15 },
  ]),
  fc(
    'initialFormState/empty',
    'initialFormState',
    [],
    'Current session state is empty; fields and status live in the mirror',
  ),
  fc(
    'formReducer/set-model',
    'indexFields',
    [snapshot(ALL_FIELDS)],
    'Mirror indexing replaces removed SET_MODEL dispatch',
  ),
  fc(
    'formReducer/other-action-is-no-change',
    'foldFormEvent',
    [loaded, unrelated],
    'An unrelated confirmed event leaves the index identity unchanged',
  ),
  fc('fieldRefByName/fqn', 'toFieldRef', ['parent.child']),
  fc(
    'fieldRefByObjectNumber/object-number',
    'encodeFieldRefKey',
    [ref(12)],
    'Removed numeric constructor becomes a direct ref input to the shipping durable-ref codec',
  ),
  fc('formTools/the-palette', 'formTools', []),
  fc('formToolById/text', 'formToolById', ['form-text']),
  fc('formToolById/listbox', 'formToolById', ['form-listbox']),
  fc('formToolById/unknown', 'formToolById', ['form-signature']),
  fc('resolveFieldSelection/null-selects-all', 'resolveFieldSelection', [TREE, null, false]),
  fc('resolveFieldSelection/null-ignores-exclude', 'resolveFieldSelection', [TREE, null, true]),
  fc('resolveFieldSelection/empty-include-selects-nothing', 'resolveFieldSelection', [
    TREE,
    [],
    false,
  ]),
  fc('resolveFieldSelection/empty-exclude-selects-all', 'resolveFieldSelection', [TREE, [], true]),
  fc('resolveFieldSelection/a-parent-name-selects-descendants', 'resolveFieldSelection', [
    TREE,
    [n('parent')],
    false,
  ]),
  fc('resolveFieldSelection/exact-name', 'resolveFieldSelection', [TREE, [n('other')], false]),
  fc('resolveFieldSelection/exclude-a-subtree', 'resolveFieldSelection', [
    TREE,
    [n('parent')],
    true,
  ]),
  fc('resolveFieldSelection/object-number', 'resolveFieldSelection', [TREE, [o(42)], false]),
  fc('resolveFieldSelection/object-number-never-matches-an-fqn-ref', 'resolveFieldSelection', [
    TREE,
    [o(0)],
    false,
  ]),
  negative(
    'resolveFieldSelection/a-numberless-target-finds-a-numberless-ref',
    'resolveFieldSelection',
    [
      [...TREE, { ...sel('bare'), ref: { kind: 'objectNumber' } }],
      [{ kind: 'objectNumber' }],
      false,
    ],
    'Deliberate missing objectNumber on both field ref and target: current predicate compares undefined; normal records remain valid',
  ),
  negative(
    'resolveFieldSelection/mixed-targets',
    'resolveFieldSelection',
    [TREE, [o(42), n('parentheses'), { kind: 'other' }], false],
    'Deliberate unknown target discriminant is ignored; valid name and number still select',
  ),
  fc('buildSubmitEntries/fields-absent-silently-drops-ineligible', 'buildSubmitEntries', [
    [
      sel('plain'),
      sel('secret', 0, { noExport: true }),
      pushbutton(0, 'button'),
      signature(0, 'sig'),
    ],
    intent(),
  ]),
  fc('buildSubmitEntries/explicit-no-export-is-diagnosed', 'buildSubmitEntries', [
    [sel('secret', 0, { noExport: true }), sel('plain')],
    intent({ fields: [n('secret'), n('plain')] }),
  ]),
  fc('buildSubmitEntries/explicit-button-and-signature-are-diagnosed', 'buildSubmitEntries', [
    [pushbutton(0, 'button'), signature(0, 'sig')],
    intent({ fields: [n('button'), n('sig')] }),
  ]),
  fc('buildSubmitEntries/exclude-mode-is-never-explicit', 'buildSubmitEntries', [
    [pushbutton(0, 'button'), sel('secret', 0, { noExport: true }), sel('plain')],
    intent({ fields: [n('plain')], exclude: true }),
  ]),
  fc('buildSubmitEntries/unsupported-value-shape-is-always-diagnosed', 'buildSubmitEntries', [
    [sel('weird', 0, { valueEntry: { kind: 'unsupported' } }), sel('plain')],
    intent(),
  ]),
  negative(
    'buildSubmitEntries/unknown-value-kind-is-diagnosed',
    'buildSubmitEntries',
    [[{ ...sel('odd'), valueEntry: { kind: 'future' } }], intent()],
    'Deliberate unknown /V discriminant is diagnosed and skipped without throwing',
  ),
  fc('buildSubmitEntries/valueless-skipped-by-default', 'buildSubmitEntries', [
    [sel('empty', 0, { valueEntry: { kind: 'none' } })],
    intent(),
  ]),
  fc('buildSubmitEntries/valueless-name-only-when-included', 'buildSubmitEntries', [
    [sel('empty', 0, { valueEntry: { kind: 'none' } })],
    intent({ includeNoValueFields: true }),
  ]),
  fc('buildSubmitEntries/descendants-and-arrays', 'buildSubmitEntries', [
    [
      sel('parent.c1'),
      sel('parent.c2', 0, { valueEntry: { kind: 'array', values: ['a', 'b'] } }),
      sel('other'),
    ],
    intent({ fields: [n('parent')] }),
  ]),
];
export const suites: FunctionSuite[] = [
  {
    kind: 'function',
    file: 'plugins/form-core.json',
    describes: 'Current form index, prediction, widget projection, tools and ISO selection.',
    functions,
    cases,
  },
];
