/** Current form fill boundary: snapshots, pending writes, confirmations, authority and lifecycle. */
import type {
  FormBundle,
  FormRepairResult,
  FormFieldValue,
  FormImportResult,
} from '@embedpdf/engine-core/runtime';
import type { ScenarioCase, Step } from '../scenario';
import type { ScenarioSuite } from '../vectors';
import {
  call,
  text,
  checkbox,
  combobox,
  listbox,
  pushbutton,
  row,
  ref,
  fqn,
  placed,
  snapshot,
  list,
  valueSet,
  apply,
  valuesAnswer,
  resetAnswer,
  mutationMeta,
  valueFact,
  scenario,
  refused,
  FORM_ONLY,
  doc,
  boot,
  open,
  settle,
  emit,
  close,
  fnSpec,
  REMOTE,
  LOCAL,
  objRef,
  pr,
  fieldAnswer,
  deleteAnswer,
} from './form-live-fixtures';
const NAME = text(1, 'name', 'Ada');
const AGREE = checkbox(2, 'agree', false, { valueEntry: { kind: 'scalar', value: 'Off' } });
const COLOUR = combobox(3, 'colour', 'r');
const COLOURS = listbox(4, 'colours', ['g']);
const FIELDS = [NAME, AGREE, COLOUR, COLOURS];
const ROWS = [
  row(101, ref(1), 1, { x: 10, y: 20, width: 100, height: 24 }),
  row(102, ref(2), 1, { x: 50, y: 82, width: 18, height: 18 }, { fieldFamily: 'checkbox' }),
  row(103, ref(3), 1, undefined, { fieldFamily: 'combobox' }),
  row(104, ref(4), 1, undefined, { fieldFamily: 'listbox' }),
];
const FORM = snapshot(FIELDS, ROWS);
const GRACE = text(1, 'name', 'Grace');
const EMPTY = text(1, 'name', '');
const RESET_FIELDS = [EMPTY, combobox(3, 'colour', ''), listbox(4, 'colours', [])];
const RESET_ROWS = [ROWS[0]!, ROWS[2]!, ROWS[3]!];
const ro = text(6, 'ro', 'r', { readOnly: true });
const v = (value: string): FormFieldValue => ({ value });
const set = (n: number, value: string): Step => call('setValue', [ref(n), v(value)]);
const gate = { security: { allows: ['doc.forms.read'], allowsField: { fill: false } } };
const BUNDLE: FormBundle = {
  format: 'embedpdf/form',
  version: 1,
  pages: [{ page: pr(1), position: 0, size: { width: 600, height: 800 } }],
  fields: FIELDS.map((data) => ({ data })),
  widgets: ROWS.map((data) => ({ data, resources: {} })),
  calculationOrder: [],
  resources: {},
};
const IMPORT_BUNDLE: FormBundle = {
  format: 'embedpdf/form',
  version: 1,
  pages: [],
  fields: [{ data: text(40, 'imported', 'Imported', {}, []) }],
  widgets: [],
  calculationOrder: [],
  resources: {},
};
const IMPORTED_FIELD = text(50, 'imported', 'Imported', {}, []);
const IMPORTED: FormImportResult = {
  fields: [IMPORTED_FIELD],
  widgets: [],
  refMap: [{ from: ref(40), to: ref(50) }],
  dropped: [],
  calculationOrder: [],
  meta: mutationMeta(1, [IMPORTED_FIELD]),
};
const REPAIRED: FormRepairResult = {
  acroformCreated: false,
  fieldsLinked: 0,
  widgetsLinked: 0,
  fieldsUnrepairable: 0,
  appearancesBaked: 0,
  needsAppearancesCleared: false,
  meta: { affectedPages: [], cacheDelta: null, opId: 'repair', undoable: false },
};
const INTEGER_FIELDS = [
  text(1, 'b', 'x'),
  text(2, '10', 'y'),
  text(3, '2', 'z'),
  text(4, 'a', 'w'),
  text(5, '02', 'v'),
];
const READ_FIELDS = [
  ...FIELDS,
  text(7, 'lines', 'l1\nl2', {
    multiline: true,
    valueEntry: { kind: 'array', values: ['l1', 'l2'] },
  }),
  checkbox(8, 'multi', false, {
    valueEntry: { kind: 'array', values: [] },
    widgets: [{ ...placed(108, 2), onState: 'Yes', exportValue: 'Yes', checked: false }],
  }),
  checkbox(9, 'multi2', false, { valueEntry: { kind: 'array', values: ['On'] } }),
  combobox(10, 'pick', 'r', { valueEntry: { kind: 'array', values: ['r', 'g'] } }),
  listbox(11, 'one', ['r']),
  pushbutton(12, 'go'),
  text(13, 'odd', '', { valueEntry: { kind: 'unsupported' } }),
  text(14, 'loose', 'l', {}, [placed(114, null)]),
  checkbox(15, 'ticked', true),
];
const EVENT_TYPES = [
  'forms.create',
  'forms.update',
  'forms.addWidget',
  'forms.removeWidget',
] as const;
const cases: ScenarioCase[] = [
  {
    name: 'hydrates-on-open',
    covers:
      'One deferred complete snapshot; reads and widget joins are empty until it lands. Authority is per field.',
    plugins: FORM_ONLY,
    engine: { documents: { a: doc([list(FORM, { defer: 'list' })]) } },
    snapshot: ['form::a'],
    steps: [
      ...boot,
      call('getSnapshot'),
      call('listWidgets', [pr(1)]),
      call('get', [ref(1)]),
      call('canRead'),
      call('canFill', [ref(1)]),
      call('canDesign'),
      settle('list', FORM),
      call('getSnapshot'),
      call('get', [ref(1)]),
      call('get', [ref(404)]),
      call('getFieldForWidget', [placed(102)]),
      call('getFieldForWidget', [placed(404)]),
      call('getWidget', [objRef(101)]),
      call('getWidget', [objRef(404)]),
    ],
  },
  scenario(
    'no-read-grant-never-lists',
    'The readable mirror stays forbidden and never calls forms.list, including refresh.',
    [
      call('refresh'),
      call('getSnapshot'),
      call('canRead'),
      call('canFill', [ref(1)]),
      call('canDesign'),
    ],
    FORM,
    [],
    { security: { allows: ['doc.forms.fill', 'doc.forms.modify'] } },
  ),
  {
    name: 'refresh-coalesces-and-forces',
    covers:
      'Current mirror coalesces concurrent refreshes; a subsequent explicit refresh starts a fresh load. No force flag.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list(snapshot([NAME]), { defer: 'first', times: 1 }),
          list(snapshot([GRACE]), { defer: 'second', times: 1 }),
          list(snapshot([text(1, 'name', 'Hopper')]), { defer: 'third' }),
        ]),
      },
    },
    snapshot: ['form::a'],
    steps: [
      ...boot,
      call('refresh'),
      call('refresh'),
      call('refresh'),
      settle('first', snapshot([NAME])),
      settle('second', snapshot([GRACE])),
      call('getSnapshot'),
      call('refresh'),
      settle('third', snapshot([text(1, 'name', 'Hopper')])),
    ],
  },
  {
    name: 'a-failed-load-warns',
    covers:
      'Current mirror exposes a failed read through error status, without console or kernel reports; refresh recovers.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          {
            path: 'forms.list',
            times: 1,
            error: { name: 'EngineError', message: 'intended form load refusal', code: 'Io' },
          },
          list(FORM),
        ]),
      },
    },
    snapshot: ['form::a'],
    steps: [
      ...boot,
      call('getStatus'),
      call('getSnapshot'),
      call('refresh'),
      call('getStatus'),
      call('getSnapshot'),
    ],
  },
  {
    name: 'widget-geometry-loads-once-per-page',
    covers:
      'Complete page-space widget rows arrive with the one form snapshot. ensureLoaded shares that load; no annotation geometry read exists.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list(
            snapshot(
              [...FIELDS, text(5, 'far', 'f', {}, [placed(105, 2)])],
              [...ROWS, row(105, ref(5), 2, { x: 30, y: 100, width: 100, height: 20 })],
            ),
            { defer: 'rows' },
          ),
        ]),
      },
    },
    snapshot: ['form::a'],
    steps: [
      ...boot,
      call('listWidgets', [pr(1)]),
      call('ensureLoaded', [pr(1)]),
      call('ensureLoaded', [pr(1)]),
      settle(
        'rows',
        snapshot(
          [...FIELDS, text(5, 'far', 'f', {}, [placed(105, 2)])],
          [...ROWS, row(105, ref(5), 2, { x: 30, y: 100, width: 100, height: 20 })],
        ),
      ),
      call('ensureLoaded', [pr(1)]),
      call('listWidgets', [pr(1)]),
      call('getWidget', [objRef(102)]),
      call('listWidgets', [pr(99)]),
      call('ensureLoaded', [pr(2)]),
      call('listWidgets', [pr(2)]),
      call('listWidgets', [pr(1)]),
    ],
  },
  {
    name: 'a-failed-geometry-read-can-retry',
    covers:
      'Geometry is part of the fields load now; a refused snapshot reports once and explicit refresh retries the full current snapshot.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          {
            path: 'forms.list',
            times: 1,
            error: { name: 'EngineError', message: 'intended widget snapshot refusal', code: 'Io' },
          },
          list(FORM),
        ]),
      },
    },
    snapshot: ['form::a'],
    steps: [
      ...boot,
      call('getStatus'),
      call('listWidgets', [pr(1)]),
      call('refresh'),
      call('ensureLoaded', [pr(1)]),
      call('listWidgets', [pr(1)]),
    ],
  },
  {
    name: 'fill-authority-fuses-into-disabled',
    covers:
      'Per-field fill authority and readOnly determine disabled; ordinary and readonly widgets are independently projected in two sessions.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([list(snapshot([NAME, ro], [ROWS[0]!, row(106, ref(6))]))], gate),
        b: doc([list(snapshot([NAME, ro], [ROWS[0]!, row(106, ref(6))]))]),
      },
    },
    snapshot: ['form::a', 'form::b'],
    steps: [
      ...boot,
      open('b'),
      call('listWidgets', [pr(1)]),
      call('listWidgets', [pr(1)]),
      call('canFill', [ref(1)]),
      call('listWidgets', [pr(1)], 'b'),
      call('canFill', [ref(1)], 'b'),
    ],
  },
  scenario(
    'typed-writes',
    'Current family writes use value/checked/selectedValues and current refs; null clears a checkbox via its value vocabulary.',
    [
      set(1, 'Grace'),
      call('setValue', [ref(2), { checked: true } satisfies FormFieldValue]),
      call('setValue', [ref(2), { value: null } satisfies FormFieldValue]),
      call('setValue', [ref(3), { value: 'g' } satisfies FormFieldValue]),
      call('getValue', [ref(1)]),
      call('getValue', [ref(2)]),
      call('getValue', [ref(3)]),
    ],
    FORM,
    [
      apply(valueSet(1, GRACE, [ROWS[0]!])),
      apply(valuesAnswer(2, [checkbox(2, 'agree', true)], [ROWS[1]!], [])),
      apply(valuesAnswer(3, [AGREE], [ROWS[1]!], [])),
      apply(valueSet(4, combobox(3, 'colour', 'g'), [ROWS[2]!])),
    ],
  ),
  scenario(
    'writes-serialize',
    'Current fills send in order without waiting for prior answers; pending reads see the later fill until both confirmations land.',
    [
      set(1, 'a'),
      set(1, 'b'),
      call('getValue', [ref(1)]),
      settle('first', valueSet(1, text(1, 'name', 'a'), [ROWS[0]!]).value),
      settle('second', valueSet(2, text(1, 'name', 'b'), [ROWS[0]!]).value),
      call('getValue', [ref(1)]),
    ],
    FORM,
    [
      apply(valueSet(1, text(1, 'name', 'a'), [ROWS[0]!]), { defer: 'first' }),
      apply(valueSet(2, text(1, 'name', 'b'), [ROWS[0]!]), { defer: 'second' }),
    ],
  ),
  scenario(
    'a-failed-write-reverts',
    'An engine refusal rolls back the pending value and rejects; the next fill confirms normally.',
    [set(1, 'bad'), call('getValue', [ref(1)]), set(1, 'good'), call('getValue', [ref(1)])],
    FORM,
    [
      refused('invalid value', 'InvalidArg'),
      apply(valueSet(2, text(1, 'name', 'good'), [ROWS[0]!])),
    ],
  ),
  scenario(
    'the-write-gate',
    'Known fields denied fill reject value/checkbox/choice/reset locally before any apply.',
    [
      set(1, 'x'),
      call('setValue', [ref(2), { checked: true } satisfies FormFieldValue]),
      call('setValue', [ref(3), { value: null } satisfies FormFieldValue]),
      call('reset', [[ref(1)]]),
      call('getValue', [ref(1)]),
    ],
    FORM,
    [],
    gate,
  ),
  scenario(
    'commit-value-without-scripting',
    'The public validated setValue replaces commitValue: applied or unchanged comes from result.meta.undoable; a named engine refusal rejects.',
    [set(1, 'x'), set(1, 'x'), set(1, 'y'), call('getValue', [ref(1)])],
    FORM,
    [
      apply(valueSet(1, text(1, 'name', 'x'), [ROWS[0]!])),
      apply({
        value: {
          ...valueSet(2, text(1, 'name', 'x')).value,
          meta: { ...valueSet(2, text(1, 'name', 'x')).value.meta, undoable: false },
        },
        events: [],
      }),
      refused('nope', 'InvalidArg'),
    ],
  ),
  scenario(
    'reset-rides-the-batch',
    'Current reset is one forms.reset ChangeOp and confirmed value facts, with no re-read. Unknown refs select nothing.',
    [call('reset', [[ref(1)]]), call('getValue', [ref(1)]), call('reset', [[ref(404)]])],
    FORM,
    [apply(resetAnswer(1, [EMPTY], [ROWS[0]!]))],
  ),
  scenario(
    'a-failed-reset-batch-rejects',
    'Current resets are atomic apply changes; exact named engine refusals reject without stale per-entry batch statuses.',
    [
      call('reset', [[ref(1)]]),
      call('getValue', [ref(1)]),
      call('reset', [[ref(1)]]),
      call('reset', [[ref(1)]]),
    ],
    FORM,
    [
      refused('disk full', 'Io'),
      refused('reset refused', 'Refused'),
      refused('batch refused', 'InvalidArg'),
    ],
  ),
  scenario(
    'reset-without-the-batch',
    'Removed applyEffects/reset fallback is replaced by the sole current change queue; successful and refused reset preserve defaults/rollback.',
    [call('reset', [[ref(1)]]), call('getValue', [ref(1)]), call('reset', [[ref(1)]])],
    FORM,
    [apply(resetAnswer(1, [EMPTY], [ROWS[0]!])), refused('no default', 'InvalidArg')],
  ),
  scenario(
    'set-value-passthrough',
    'Current setValue returns the confirmed field and status; the next refused value leaves it intact.',
    [set(1, 'x'), set(1, 'y'), call('get', [ref(1)])],
    FORM,
    [apply(valueSet(1, text(1, 'name', 'x'), [ROWS[0]!])), refused('nope', 'InvalidArg')],
  ),
  scenario(
    'interchange',
    'Current bundles replace FDF/XFDF data doors; export/import/repair use current direct engine services and exact refusals.',
    [
      call('export'),
      call('export', [{ fields: [ref(1)] }]),
      call('import', [IMPORT_BUNDLE, { attribution: 'stamp' }]),
      call('get', [ref(50)]),
      call('import', [IMPORT_BUNDLE, { attribution: 'stamp' }]),
      call('repair'),
      call('repair', [{ bakeAppearances: true }]),
    ],
    FORM,
    [
      { path: 'forms.export', times: 1, value: BUNDLE },
      {
        path: 'forms.export',
        error: { name: 'EngineError', message: 'export refused', code: 'NotImplemented' },
      },
      {
        path: 'forms.import',
        times: 1,
        value: IMPORTED,
        events: fieldAnswer('forms.create', 1, IMPORTED_FIELD).events,
      },
      {
        path: 'forms.import',
        error: { name: 'EngineError', message: 'garbled bundle', code: 'InvalidArg' },
      },
      { path: 'forms.repair', times: 1, value: REPAIRED },
      {
        path: 'forms.repair',
        error: { name: 'EngineError', message: 'repair refused', code: 'Refused' },
      },
    ],
  ),
  {
    name: 'the-twins',
    covers:
      'Read/design grants and per-field fill authority are independently exposed by the current twins.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([list(FORM)], {
          security: { allows: ['doc.forms.read', 'doc.forms.fill'], allowsField: { fill: true } },
        }),
        b: doc([list(FORM)], {
          security: { allows: ['doc.forms.modify'], allowsField: { fill: false } },
        }),
      },
    },
    snapshot: ['form::a', 'form::b'],
    steps: [
      ...boot,
      open('b'),
      call('canRead'),
      call('canFill', [ref(1)]),
      call('canDesign'),
      call('canRead', [], 'b'),
      call('canFill', [ref(1)], 'b'),
      call('canDesign', [], 'b'),
    ],
  },
  scenario(
    'the-effect-folds-foreign-events',
    'Current remote and local facts fold directly without reads; deletion removes the field/rows. Desynchronization reloads the complete form.',
    [
      call('onValueChanged', [fnSpec('valueChanged')]),
      emit(valueFact(GRACE, [ROWS[0]!])),
      call('getValue', [ref(1)]),
      emit({ ...valueFact(text(1, 'name', 'Local'), [ROWS[0]!]), origin: LOCAL }),
      call('getValue', [ref(1)]),
      emit({ ...deleteAnswer(1, NAME, [ROWS[0]!]).events[0]! }),
      call('get', [ref(1)]),
      call('getWidget', [objRef(101)]),
      emit({ type: 'stream.desynced', reason: 'backlog-overflow', ts: 0 }),
      call('getSnapshot'),
    ],
    FORM,
  ),
  scenario(
    'the-effect-refresh-is-latched',
    'While refresh is in flight current mirrors buffer facts, then replay them over the loaded snapshot. The last buffered fact wins, and later facts fold normally.',
    [
      call('refresh'),
      emit(valueFact(GRACE, [ROWS[0]!])),
      call('getValue', [ref(1)]),
      emit(valueFact(text(1, 'name', 'Second'), [ROWS[0]!])),
      call('getValue', [ref(1)]),
      settle('remote', snapshot([NAME], [ROWS[0]!])),
      call('getValue', [ref(1)]),
      emit(valueFact(text(1, 'name', 'After'), [ROWS[0]!])),
      call('getValue', [ref(1)]),
    ],
    FORM,
    [list(snapshot([NAME], [ROWS[0]!]), { defer: 'remote' })],
  ),
  scenario(
    'a-reset-action-without-a-batch-door',
    'The host reset action uses the same current forms.reset operation without an actions plugin or any removed batch API.',
    [
      call('resetFormAction', [null, false]),
      call('getValue', [ref(1)]),
      call('resetFormAction', [[], false]),
    ],
    FORM,
    [apply(resetAnswer(1, RESET_FIELDS, RESET_ROWS))],
  ),
  scenario(
    'close-unsubscribes',
    'Closing releases the form instance, its mirrors and hooks; reopening constructs a fresh model.',
    [
      call('onValueChanged', [fnSpec('closed.valueChanged')]),
      close(),
      open('a'),
      emit(valueFact(GRACE, [ROWS[0]!])),
      call('getValue', [ref(1)]),
    ],
    FORM,
  ),
  scenario(
    'geometry-on-an-offset-crop',
    'Current widget rows already carry crop-relative page coordinates, including fractional dimensions; reads preserve them.',
    [
      call('ensureLoaded', [pr(2)]),
      call('listWidgets', [pr(2)]),
      call('getWidgetAt', [pr(2), { x: 35, y: 105 }]),
    ],
    snapshot(
      [text(5, 'far', 'f', {}, [placed(105, 2)])],
      [row(105, ref(5), 2, { x: 30, y: 99.5, width: 100, height: 20.5 })],
    ),
  ),
  {
    name: 'reads-over-the-snapshot',
    covers:
      'Current list filters, values, widget addresses and exportValues read complete flat field records and page rows; unplaced widgets are absent.',
    plugins: FORM_ONLY,
    engine: { documents: { a: doc([list(snapshot(READ_FIELDS, ROWS), { defer: 'list' })]) } },
    snapshot: ['form::a'],
    steps: [
      ...boot,
      call('getStatus'),
      call('list'),
      call('exportValues'),
      settle('list', snapshot(READ_FIELDS, ROWS)),
      call('getStatus'),
      call('list'),
      call('list', [{ family: 'checkbox' }]),
      call('list', [{ name: 'colour' }]),
      call('list', [{ name: 'nope' }]),
      call('list', [{ page: pr(2) }]),
      call('list', [{ family: 'text', page: pr(1) }]),
      call('list', [{}]),
      ...[1, 2, 3, 4, 7, 8, 9, 10, 11, 12, 13, 15, 404].map((n) => call('getValue', [ref(n)])),
      call('getValue', [fqn('loose')]),
      call('exportValues'),
      call('getFieldForWidget', [objRef(102)]),
      call('getFieldForWidget', [{ kind: 'baseIndex', page: pr(1), baseIndex: 3 }]),
      call('getFieldForWidget', [placed(104)]),
      call('listWidgets', [pr(1)]),
    ],
  },
  {
    name: 'status-without-read-and-after-a-failed-load',
    covers:
      'The current mirror returns forbidden without read permission and error after a named load refusal.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([list(FORM)], { security: { allows: ['doc.forms.fill'] } }),
        b: doc([
          {
            path: 'forms.list',
            error: { name: 'EngineError', message: 'intended status load refusal', code: 'Io' },
          },
        ]),
      },
    },
    snapshot: ['form::a', 'form::b'],
    steps: [...boot, open('b'), call('getStatus'), call('getStatus', [], 'b')],
  },
  {
    name: 'batch-writes',
    covers:
      'Current batch accepts well-typed fields and reports unknown refs independently; plain import skips missing, forbidden and incompatible values. Accepted fields share one apply.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list(FORM),
          apply(valueSet(1, GRACE, [ROWS[0]!])),
          apply(valuesAnswer(2, [NAME, combobox(3, 'colour', 'g')], [ROWS[0]!, ROWS[2]!])),
        ]),
        b: doc([list(FORM)], gate),
      },
    },
    snapshot: ['form::a', 'form::b'],
    steps: [
      ...boot,
      open('b'),
      call('setValues', [
        [
          { ref: ref(1), value: v('Grace') },
          { ref: ref(404), value: v('missing') },
        ],
      ]),
      call('importValues', [{ name: 'Ada', colour: 'g', absent: 'x', agree: ['bad'] }]),
      call('setValues', [[]]),
      call('setValues', [[{ ref: ref(1), value: v('x') }]], 'b'),
      call('importValues', [{ name: 'x' }], 'b'),
      call('exportValues'),
    ],
  },
  scenario(
    'values-keyed-by-integer-like-names',
    'Plain import/export follow JavaScript integer-like property ordering; the actual apply operation order records that ordering.',
    [
      call('exportValues'),
      call('importValues', [{ b: 'B', 10: 'Ten', 2: 'Two', a: 'A', '02': 'Zero-two' }]),
      call('exportValues'),
    ],
    snapshot(INTEGER_FIELDS),
    [
      apply(
        valuesAnswer(1, [
          text(3, '2', 'Two'),
          text(2, '10', 'Ten'),
          text(1, 'b', 'B'),
          text(4, 'a', 'A'),
          text(5, '02', 'Zero-two'),
        ]),
      ),
    ],
  ),
  scenario(
    'reset-all',
    'Current reset() selects every fillable resettable field, or explicit refs. Host action exclusion preserves ISO target selection; empty include selects none.',
    [
      call('reset'),
      call('reset', [[ref(1), fqn('agree')]]),
      call('resetFormAction', [[{ kind: 'objectNumber', objectNumber: 1 }], true]),
      call('reset', [[]]),
      call('getValue', [ref(1)]),
    ],
    snapshot([...FIELDS, pushbutton(12, 'go')], ROWS),
    [
      apply(resetAnswer(1, RESET_FIELDS, RESET_ROWS)),
      apply(resetAnswer(2, [], [], false)),
      apply(resetAnswer(3, [], [], false)),
    ],
  ),
  {
    name: 'reset-all-failures',
    covers:
      'Explicit denied reset rejects locally; reset waits for the snapshot. Current atomic engine refusal rejects the entire change and the next reset succeeds.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list(FORM, { defer: 'first' }),
          refused('reset atomic refusal'),
          apply(resetAnswer(2, RESET_FIELDS, RESET_ROWS)),
        ]),
        b: doc([list(FORM)], gate),
      },
    },
    snapshot: ['form::a', 'form::b'],
    steps: [
      ...boot,
      open('b'),
      call('reset', [[ref(1)]], 'b'),
      call('reset'),
      settle('first', FORM),
      call('getValue', [ref(1)]),
      call('reset'),
      call('getValue', [ref(1)]),
    ],
  },
  scenario(
    'confirmed-change-events',
    'Current facts announce value/create/update/widget/remove/delete with exact origins; refresh alone announces resync. Unrelated version facts do not synthesize form writes.',
    [
      call('onValueChanged', [{ $fn: 'valueChanged.bad', throws: { message: 'listener broke' } }]),
      call('onValueChanged', [fnSpec('valueChanged')]),
      call('onFieldCreated', [fnSpec('fieldCreated')]),
      call('onFieldUpdated', [fnSpec('fieldUpdated')]),
      call('onFieldDeleted', [fnSpec('fieldDeleted')]),
      call('onValidationRejected', [fnSpec('validationRejected')]),
      call('onResynced', [fnSpec('resynced')]),
      emit(valueFact(GRACE, [ROWS[0]!])),
      emit({ ...valueFact(AGREE, [ROWS[1]!]), origin: LOCAL }),
      ...EVENT_TYPES.map((type) => emit(fieldAnswer(type, 1, COLOUR, [ROWS[2]!]).events[0]!)),
      emit(deleteAnswer(1, COLOUR, [ROWS[2]!]).events[0]!),
      call('refresh'),
      call('getSnapshot'),
    ],
    FORM,
  ),
  scenario(
    'an-unsubscribed-listener-hears-nothing',
    'An event hook returns a callable unsubscribe; detaching it prevents the subsequent refreshed snapshot callback.',
    [call('onResynced', [fnSpec('resynced')]), { op: 'call', target: { ref: 0 } }, call('refresh')],
    FORM,
  ),
];
export const suites: ScenarioSuite[] = [
  {
    kind: 'scenario',
    file: 'plugins/form-fill.json',
    describes:
      'Current form fill, mirror geometry, values/defaults, authority, interchange, events and lifecycle.',
    cases,
  },
];
