/** Current form design: direct drafts/ops, form-owned widgets lent to annotation, and real pointer placement. */
import type {
  FormFieldDraft,
  FormFieldPatch,
  WidgetPlacement,
  WidgetAppearance,
} from '@embedpdf/engine-core/runtime';
import type { ScenarioCase, Step } from '../scenario';
import type { ScenarioSuite } from '../vectors';
import {
  scenario,
  call,
  text,
  checkbox,
  radio,
  combobox,
  listbox,
  signature,
  row,
  placed,
  ref,
  objRef,
  pr,
  snapshot,
  list,
  apply,
  fieldAnswer,
  deleteAnswer,
  refused,
  FORM_ONLY,
  WITH_ANNOTATION,
  doc,
  boot,
  settle,
  ix,
  anno,
  tool,
  sample,
  open,
  fnSpec,
} from './form-live-fixtures';
const BOX = { x: 10, y: 20, width: 100, height: 24 };
const place = (page = 1, rect = BOX): WidgetPlacement => ({ page: pr(page), rect });
const draft = (name = 'text_1', widgets = [place()]): FormFieldDraft => ({
  family: 'text',
  name,
  widgets,
});
const NEW = text(10001, 'text_1', '', {}, [placed(10002)]);
const NEW_ROW = row(10002, NEW.ref, 1, BOX);
const NEW2 = text(10003, 'text_2', '', {}, [placed(10004)]);
const NEW_ROW2 = row(10004, NEW2.ref, 1, BOX);
const PALETTE_CHROME: WidgetAppearance = {
  color: '#6b7280',
  interiorColor: '#ffffff',
  strokeWidth: 1,
};
const PALETTE_TEXT: WidgetAppearance = { ...PALETTE_CHROME, fontSize: 12 };
const EXISTING = text(1, 'text_1', 'taken');
const PATCH: FormFieldPatch = { name: 'renamed' };
const pointer = (
  phase: string,
  x: number,
  y: number,
  over: Record<string, unknown> = {},
  page = 1,
): Step => ix('dispatchPointer', [sample(phase, x, y, over, page)]);
const gap = (phase: string): Step =>
  ix('dispatchPointer', [
    {
      phase,
      viewport: { x: 900, y: 900 },
      modifiers: { shift: false, alt: false, ctrl: false, meta: false },
    },
  ]);
const click = (x = 100, y = 100): Step[] => [pointer('down', x, y), pointer('up', x, y)];
const HIT_FIELDS = [
  text(1, 'big', '', {}, [placed(101)]),
  text(2, 'small', '', {}, [placed(102)]),
  text(3, 'unplaced', '', {}, [placed(103, null)]),
];
const HIT_ROWS = [
  row(101, ref(1), 1, { x: 0, y: 0, width: 300, height: 300 }),
  row(102, ref(2), 1, { x: 10, y: 10, width: 50, height: 90 }),
  row(150, null, 1, { x: 400, y: 600, width: 200, height: 200 }),
];
const LIVE_FIELDS = [201, 202, 203, 204, 205, 206].map((n) =>
  text(n - 200, 'live' + n, '', {}, [placed(n)]),
);
const LIVE_ROWS = [
  row(201, ref(1), 1, { x: 99, y: 300, width: 2, height: 200 }),
  row(202, ref(2), 1, { x: 85, y: 385, width: 30, height: 30 }),
  row(203, ref(3), 1, { x: 280, y: 395, width: 40, height: 10 }),
  row(204, ref(4), 1, { x: 285, y: 385, width: 30, height: 30 }),
  row(205, ref(5), 1, { x: 490, y: 390, width: 20, height: 20 }),
  row(206, ref(6), 1, { x: 495, y: 395, width: 20, height: 20 }),
];
const SPREAD = text(7, 'spread', 'v', {}, [
  placed(107),
  placed(108, 2),
  placed(109),
  placed(110, null),
]);
const SPREAD_ROWS = [row(107, SPREAD.ref), row(108, SPREAD.ref, 2), row(109, SPREAD.ref)];
const PAIR = text(7, 'pair', 'v', {}, [placed(107), placed(108, null)]);
const DETACHED = text(7, 'pair', 'v', {}, [placed(108, null)]);
const RADIO = radio(10001, 'radio_1', [
  { ...placed(10002), onState: 'Choice1', exportValue: 'Choice1', checked: false },
]);
const COMBO = combobox(10003, 'combobox_1', '', {
  widgets: [placed(10004)],
  options: [
    { label: 'Option 1', value: 'Option 1', selected: false },
    { label: 'Option 2', value: 'Option 2', selected: false },
  ],
});
const LIST = listbox(10005, 'listbox_1', [], { widgets: [placed(10006)], options: COMBO.options });
const CHECK = checkbox(10007, 'checkbox_1', false, {
  widgets: [{ ...placed(10008), onState: 'Yes', exportValue: 'Yes', checked: false }],
});
const cases: ScenarioCase[] = [
  scenario(
    'init-without-annotation',
    'Current connect registers fill/edit and all seven palette tools on the actual interaction hub.',
    [ix('listTools')],
  ),
  scenario(
    'init-with-annotation',
    'Palette tools join annotation.tools; widgets lend in design mode and return to form fill mode.',
    [
      ix('listTools'),
      anno('tools.get', ['form-text']),
      anno('tools.get', ['form-listbox']),
      anno('tools.getDefaults', ['form-checkbox']),
      anno('listPageItems', [pr(1)]),
      tool('form-edit'),
      anno('listPageItems', [pr(1)]),
      call('getSelectedField'),
      tool('form-fill'),
      anno('listPageItems', [pr(1)]),
      tool('pan'),
      anno('listPageItems', [pr(1)]),
    ],
    snapshot([EXISTING], [row(101, ref(1))]),
    [],
    {},
    true,
  ),
  scenario(
    'place-a-text-field',
    'Current create accepts page-space drafts and explicit names. The handler owns auto-naming; engine answers fold without geometry reloads.',
    [
      call('create', [draft('text_3')]),
      call('get', [ref(10001)]),
      call('create', [
        draft('text_4', [{ page: pr(2), rect: BOX, interiorColor: '#ffffff', strokeWidth: 1 }]),
      ]),
      call('getWidgets', [ref(10003)]),
    ],
    snapshot([EXISTING, text(2, 'text_2')]),
    [
      apply(fieldAnswer('forms.create', 1, { ...NEW, name: 'text_3' }, [NEW_ROW])),
      apply(
        fieldAnswer('forms.create', 2, { ...NEW2, name: 'text_4', widgets: [placed(10004, 2)] }, [
          row(10004, ref(10003), 2, BOX, { interiorColor: '#ffffff', strokeWidth: 1 }),
        ]),
      ),
    ],
  ),
  scenario(
    'place-each-family',
    'Current direct drafts retain radio exportValue and complete choice options, and return complete family-specific fields/widgets.',
    [
      call('create', [
        {
          family: 'radio',
          name: 'radio_1',
          widgets: [{ ...place(), exportValue: 'Choice1' }],
        } satisfies FormFieldDraft,
      ]),
      call('create', [
        {
          family: 'combobox',
          name: 'combobox_1',
          widgets: [place()],
          options: [
            { label: 'Option 1', value: 'Option 1' },
            { label: 'Option 2', value: 'Option 2' },
          ],
        } satisfies FormFieldDraft,
      ]),
      call('create', [
        {
          family: 'listbox',
          name: 'listbox_1',
          widgets: [place()],
          options: [
            { label: 'Option 1', value: 'Option 1' },
            { label: 'Option 2', value: 'Option 2' },
          ],
        } satisfies FormFieldDraft,
      ]),
      call('create', [
        { family: 'checkbox', name: 'checkbox_1', widgets: [place()] } satisfies FormFieldDraft,
      ]),
      call('list'),
    ],
    snapshot([]),
    [
      apply(
        fieldAnswer('forms.create', 1, RADIO, [
          row(10002, RADIO.ref, 1, BOX, { fieldFamily: 'radio' }),
        ]),
      ),
      apply(
        fieldAnswer('forms.create', 2, COMBO, [
          row(10004, COMBO.ref, 1, BOX, { fieldFamily: 'combobox' }),
        ]),
      ),
      apply(
        fieldAnswer('forms.create', 3, LIST, [
          row(10006, LIST.ref, 1, BOX, { fieldFamily: 'listbox' }),
        ]),
      ),
      apply(
        fieldAnswer('forms.create', 4, CHECK, [
          row(10008, CHECK.ref, 1, BOX, { fieldFamily: 'checkbox' }),
        ]),
      ),
    ],
  ),
  {
    name: 'placement-before-the-snapshot',
    covers:
      'The actual handler names against the current view while the first list is pending; create shows after confirmation and remains after the snapshot loads.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([
          list(snapshot([EXISTING]), { defer: 'list' }),
          apply(
            fieldAnswer('forms.create', 1, NEW, [
              row(10002, NEW.ref, 1, { x: 20, y: 88, width: 160, height: 24 }, PALETTE_TEXT),
            ]),
          ),
        ]),
      },
    },
    snapshot: ['form::a'],
    steps: [
      ...boot,
      tool('form-text'),
      ...click(),
      call('get', [ref(10001)]),
      settle(
        'list',
        snapshot(
          [EXISTING, NEW],
          [row(10002, NEW.ref, 1, { x: 20, y: 88, width: 160, height: 24 }, PALETTE_TEXT)],
        ),
      ),
      call('list'),
    ],
  },
  scenario(
    'placement-is-page-bound',
    'Actual gesture placement clamps overshoots, ignores degenerate drags and unknown pages, and permits one-point geometry.',
    [
      tool('form-text'),
      pointer('down', -30, 790),
      pointer('up', 700, 830),
      pointer('down', 10, 10),
      pointer('up', 100, 10.5),
      pointer('down', 700, 10),
      pointer('up', 730, 40),
      pointer('down', 10, 10, {}, 99),
      pointer('up', 100, 40, {}, 99),
      pointer('down', 599, 799),
      pointer('up', 604, 804),
      call('list'),
    ],
    snapshot([]),
    [
      apply(
        fieldAnswer('forms.create', 1, NEW, [
          row(10002, NEW.ref, 1, { x: 0, y: 790, width: 600, height: 10 }, PALETTE_TEXT),
        ]),
      ),
      apply(
        fieldAnswer('forms.create', 2, NEW2, [
          row(10004, NEW2.ref, 1, { x: 599, y: 799, width: 1, height: 1 }, PALETTE_TEXT),
        ]),
      ),
    ],
  ),
  scenario(
    'placement-refused-by-the-engine',
    'A current create refusal rejects without confirmation; the next create succeeds with its own allocated refs.',
    [call('create', [draft()]), call('list'), call('create', [draft()]), call('list')],
    snapshot([]),
    [
      {
        path: 'apply',
        times: 1,
        error: {
          name: 'PermissionDenied',
          code: 'Forbidden',
          message: 'permission denied: doc.forms.modify',
          details: { required: 'doc.forms.modify' },
        },
      },
      apply(fieldAnswer('forms.create', 2, { ...NEW2, name: 'text_1' }, [NEW_ROW2])),
    ],
  ),
  scenario(
    'placement-reloads-the-annotation-page',
    'Current confirmed widget rows are lent directly to annotation in design mode; selection needs no extra annotation read.',
    [
      tool('form-edit'),
      call('create', [draft()]),
      anno('get', [objRef(10002)]),
      anno('selection.set', [[objRef(10002)]]),
      anno('selection.list'),
      call('getSelectedField'),
    ],
    snapshot([]),
    [apply(fieldAnswer('forms.create', 1, NEW, [NEW_ROW]))],
    {},
    true,
  ),
  scenario(
    'page-box',
    'The host page box is in current page-space coordinates; unknown pages return null.',
    [call('getPageBox', [pr(1)]), call('getPageBox', [pr(2)]), call('getPageBox', [pr(99)])],
  ),
  scenario(
    'update-field',
    'Current update stages a patch against an actual field ref and folds the answer; a named engine family refusal rejects.',
    [
      call('update', [ref(1), PATCH]),
      call('get', [ref(1)]),
      call('update', [{ kind: 'fqn', name: 'a.b' }, { readOnly: true } satisfies FormFieldPatch]),
    ],
    snapshot([EXISTING]),
    [
      apply(fieldAnswer('forms.update', 1, { ...EXISTING, name: 'renamed' })),
      refused('family mismatch', 'InvalidArg'),
    ],
  ),
  scenario(
    'delete-field',
    'Deleting a spread field removes its widgets on both pages from form and annotation lending. Unknown ref deletion is an explicit engine not-found refusal.',
    [
      tool('form-edit'),
      anno('listPageItems', [pr(1)]),
      call('delete', [ref(7)]),
      call('get', [ref(7)]),
      call('listWidgets', [pr(1)]),
      call('listWidgets', [pr(2)]),
      anno('listPageItems', [pr(1)]),
      call('delete', [ref(404)]),
    ],
    snapshot([SPREAD], SPREAD_ROWS),
    [apply(deleteAnswer(1, SPREAD, SPREAD_ROWS)), refused('unknown field', 'NotFound')],
    {},
    true,
  ),
  scenario(
    'queued-verbs-interleave-with-page-reloads',
    'Current structural ops overlap without old page reloads; a confirmed update remains while a prior delete awaits its answer.',
    [
      tool('form-edit'),
      call('delete', [ref(7)]),
      call('update', [ref(8), PATCH]),
      call('get', [ref(8)]),
      settle('delete', deleteAnswer(1, SPREAD, SPREAD_ROWS).value),
      call('get', [ref(7)]),
      anno('listPageItems', [pr(1)]),
    ],
    snapshot([SPREAD, text(8, 'other', 'o')], SPREAD_ROWS),
    [
      apply(deleteAnswer(1, SPREAD, SPREAD_ROWS), { defer: 'delete' }),
      apply(fieldAnswer('forms.update', 2, text(8, 'renamed', 'o'))),
    ],
    {},
    true,
  ),
  scenario(
    'delete-field-without-annotation',
    'Direct delete works with no annotation plugin; a later engine not-found refusal is preserved.',
    [call('delete', [ref(7)]), call('list'), call('delete', [ref(7)])],
    snapshot([SPREAD], SPREAD_ROWS),
    [apply(deleteAnswer(1, SPREAD, SPREAD_ROWS)), refused('gone', 'NotFound')],
  ),
  scenario(
    'detach-widget',
    'Current removeWidget takes field and annotation refs; detached rows remain on the page with field:null and are no longer projected as fields.',
    [
      tool('form-edit'),
      call('removeWidget', [ref(7), objRef(107)]),
      call('get', [ref(7)]),
      call('getFieldForWidget', [objRef(107)]),
      call('getWidget', [objRef(107)]),
      call('removeWidget', [ref(7), objRef(999)]),
      call('removeWidget', [ref(404), objRef(1)]),
    ],
    snapshot([PAIR], [row(107, PAIR.ref)]),
    [
      apply(fieldAnswer('forms.removeWidget', 1, DETACHED, [row(107, null)])),
      refused('not attached', 'InvalidArg'),
      refused('unknown field', 'NotFound'),
    ],
    {},
    true,
  ),
  scenario(
    'widget-hit-test',
    'Current model hit test returns the smallest owned widget; unowned widgets and unknown pages return null before and after ensureLoaded.',
    [
      call('getWidgetAt', [pr(1), { x: 20, y: 20 }]),
      call('getWidgetAt', [pr(1), { x: 200, y: 200 }]),
      call('getWidgetAt', [pr(1), { x: 500, y: 700 }]),
      call('getWidgetAt', [pr(1), { x: 590, y: 20 }]),
      call('ensureLoaded', [pr(1)]),
      call('getWidgetAt', [pr(1), { x: 20, y: 20 }]),
      call('getWidgetAt', [pr(2), { x: 1, y: 1 }]),
    ],
    snapshot(HIT_FIELDS, HIT_ROWS),
    [],
    {},
    true,
  ),
  scenario(
    'widget-hit-test-live-boxes-compare-areas',
    'Current snapshot hit test compares area and preserves first-listed ties; removed live fallback has the same surviving geometric owner.',
    [
      call('getWidgetAt', [pr(1), { x: 100, y: 400 }]),
      call('getWidgetAt', [pr(1), { x: 300, y: 400 }]),
      call('getWidgetAt', [pr(1), { x: 500, y: 400 }]),
    ],
    snapshot(LIVE_FIELDS, LIVE_ROWS),
    [],
    {},
    true,
  ),
  {
    name: 'widget-hit-test-without-annotation',
    covers:
      'Before the current form snapshot lands no widget can hit; after it lands the model hits without annotation.',
    plugins: FORM_ONLY,
    engine: { documents: { a: doc([list(snapshot(HIT_FIELDS, HIT_ROWS), { defer: 'hit' })]) } },
    snapshot: ['form::a'],
    steps: [
      ...boot,
      call('getWidgetAt', [pr(1), { x: 20, y: 20 }]),
      settle('hit', snapshot(HIT_FIELDS, HIT_ROWS)),
      call('getWidgetAt', [pr(1), { x: 20, y: 20 }]),
    ],
  },
  scenario(
    'attach-widget',
    'Current addWidget creates another placement on the field, allocates its real ref and inherits look; a refused follow-up is explicit.',
    [
      tool('form-edit'),
      call('addWidget', [ref(7), { page: pr(1), rect: BOX } satisfies WidgetPlacement]),
      call('getWidgets', [ref(7)]),
      anno('get', [objRef(10001)]),
      call('addWidget', [ref(7), { page: pr(1), rect: BOX } satisfies WidgetPlacement]),
    ],
    snapshot([PAIR], [row(107, PAIR.ref)]),
    [
      apply(
        fieldAnswer('forms.addWidget', 1, { ...PAIR, widgets: [...PAIR.widgets, placed(10001)] }, [
          row(10001, PAIR.ref, 1, BOX),
        ]),
      ),
      refused('cannot add widget', 'InvalidArg'),
    ],
    {},
    true,
  ),
  scenario(
    'place-with-a-name-and-options',
    'Current explicit create supports custom choice names/options and signature fields without a stale placeField door.',
    [
      call('create', [
        {
          family: 'combobox',
          name: 'my.choice',
          widgets: [place()],
          options: [{ label: 'A', value: 'a' }],
        } satisfies FormFieldDraft,
      ]),
      call('create', [
        { family: 'signature', name: 'signature_1', widgets: [place()] } satisfies FormFieldDraft,
      ]),
      call('list'),
    ],
    snapshot([]),
    [
      apply(
        fieldAnswer(
          'forms.create',
          1,
          combobox(10001, 'my.choice', '', {
            widgets: [placed(10002)],
            options: [{ label: 'A', value: 'a', selected: false }],
          }),
          [row(10002, ref(10001), 1, BOX, { fieldFamily: 'combobox' })],
        ),
      ),
      apply(
        fieldAnswer(
          'forms.create',
          2,
          signature(10003, 'signature_1', { widgets: [placed(10004)] }),
          [row(10004, ref(10003), 1, BOX, { fieldFamily: 'signature' })],
        ),
      ),
    ],
  ),
  scenario(
    'a-click-places-the-tool-default',
    'A real pointer click creates the default text tool draft and selects the final allocated widget in design mode.',
    [
      tool('form-text'),
      pointer('down', 100, 100),
      pointer('up', 101, 101),
      anno('selection.list'),
      call('getSelectedField'),
      ix('getActiveToolId'),
    ],
    snapshot([]),
    [
      apply(
        fieldAnswer('forms.create', 1, NEW, [
          row(10002, NEW.ref, 1, { x: 20, y: 88, width: 160, height: 24 }, PALETTE_TEXT),
        ]),
      ),
    ],
    {},
    true,
  ),
  scenario(
    'a-click-near-the-edge-is-clamped',
    'The real listbox tool click clamps its standard size to the page edge.',
    [tool('form-listbox'), ...click(595, 5), call('list')],
    snapshot([]),
    [
      apply(
        fieldAnswer(
          'forms.create',
          1,
          listbox(10001, 'listbox_1', [], { widgets: [placed(10002)], options: COMBO.options }),
          [
            row(
              10002,
              ref(10001),
              1,
              { x: 460, y: 0, width: 140, height: 72 },
              { ...PALETTE_TEXT, fieldFamily: 'listbox' },
            ),
          ],
        ),
      ),
    ],
    {},
    true,
  ),
  scenario(
    'a-thin-drag-is-a-drag',
    'Without annotation a 100 by 2 drag commits its exact page-space rectangle and the tool static look.',
    [
      tool('form-text'),
      pointer('down', 50, 50),
      pointer('move', 150, 52),
      pointer('up', 150, 52),
      call('list'),
    ],
    snapshot([]),
    [
      apply(
        fieldAnswer('forms.create', 1, NEW, [
          row(10002, NEW.ref, 1, { x: 50, y: 50, width: 100, height: 2 }, PALETTE_TEXT),
        ]),
      ),
    ],
  ),
  scenario(
    'the-home-page-tracks-through-the-projection',
    'Cross-page moves use the serialized projection callable, null projections are ignored, and gap releases use the last resolved home-page point.',
    [
      tool('form-checkbox'),
      pointer('down', 10, 10),
      ix('dispatchPointer', [
        sample('move', 1, 1, { project: { $fn: 'project', returns: { x: 90, y: 40 } } }, 2),
      ]),
      ix('dispatchPointer', [
        sample('move', 1, 1, { project: { $fn: 'project', returns: null } }, 2),
      ]),
      ix('dispatchPointer', [
        {
          ...sample('up', 0, 0),
          page: undefined,
          project: { $fn: 'project', returns: { x: 110, y: 60 } },
        },
      ]),
      pointer('down', 10, 10),
      pointer('move', 60, 40),
      gap('up'),
      call('list'),
    ],
    snapshot([]),
    [
      apply(
        fieldAnswer(
          'forms.create',
          1,
          checkbox(10001, 'checkbox_1', false, {
            widgets: [{ ...placed(10002), onState: 'Yes', exportValue: 'Yes', checked: false }],
          }),
          [
            row(
              10002,
              ref(10001),
              1,
              { x: 10, y: 10, width: 100, height: 50 },
              { ...PALETTE_CHROME, fieldFamily: 'checkbox' },
            ),
          ],
        ),
      ),
      apply(
        fieldAnswer(
          'forms.create',
          2,
          checkbox(10003, 'checkbox_2', false, {
            widgets: [{ ...placed(10004), onState: 'Yes', exportValue: 'Yes', checked: false }],
          }),
          [
            row(
              10004,
              ref(10003),
              1,
              { x: 10, y: 10, width: 50, height: 30 },
              { ...PALETTE_CHROME, fieldFamily: 'checkbox' },
            ),
          ],
        ),
      ),
    ],
  ),
  scenario(
    'the-live-preview',
    'The real form placement handler updates annotation page items past the shared drag threshold and clears its preview on release.',
    [
      tool('form-checkbox'),
      pointer('down', 10, 10),
      pointer('move', 11, 11),
      anno('listPageItems', [pr(1)]),
      pointer('move', 60, 40),
      anno('listPageItems', [pr(1)]),
      pointer('move', 12, 12),
      anno('listPageItems', [pr(1)]),
      pointer('move', 60, 40),
      pointer('up', 60, 40),
      anno('listPageItems', [pr(1)]),
    ],
    snapshot([]),
    [
      apply(
        fieldAnswer(
          'forms.create',
          1,
          checkbox(10001, 'checkbox_1', false, {
            widgets: [{ ...placed(10002), onState: 'Yes', exportValue: 'Yes', checked: false }],
          }),
          [
            row(
              10002,
              ref(10001),
              1,
              { x: 10, y: 10, width: 50, height: 30 },
              { ...PALETTE_CHROME, fieldFamily: 'checkbox' },
            ),
          ],
        ),
      ),
    ],
    {},
    true,
  ),
  scenario(
    'the-click-threshold-is-strict',
    'Exactly three points is a drag; degenerate dimensions produce no placement or engine call.',
    [
      tool('form-text'),
      pointer('down', 50, 50),
      pointer('up', 53, 50),
      pointer('down', 50, 50),
      pointer('up', 50, 53),
      call('list'),
    ],
  ),
  scenario(
    'the-handler-outranks-lower-priorities',
    'The actual priority95 form handler captures before a priority50 observer and reports form-place in gesture events.',
    [
      ix('registerHandler', [
        {
          id: 'low',
          priority: 50,
          enabledFor: { $fn: 'low.enabledFor', returns: true },
          onDown: { $fn: 'low.onDown', returns: true },
          onUp: { $fn: 'low.onUp' },
        },
      ]),
      ix('onGestureStarted', [fnSpec('gestureStarted')]),
      tool('form-text'),
      ...click(),
      ix('getActiveToolId'),
    ],
    snapshot([]),
    [
      apply(
        fieldAnswer('forms.create', 1, NEW, [
          row(10002, NEW.ref, 1, { x: 20, y: 88, width: 160, height: 24 }, PALETTE_TEXT),
        ]),
      ),
    ],
  ),
  {
    name: 'the-handler-declines',
    covers:
      'The real handler declines gaps, unknown palette tools, missing design authority and a replacement tool without form-place.',
    plugins: FORM_ONLY,
    engine: {
      documents: {
        a: doc([list(snapshot([]))]),
        b: doc([list(snapshot([]))], {
          security: { allows: ['doc.forms.read', 'doc.forms.fill'] },
        }),
      },
    },
    snapshot: ['form::a', 'form::b'],
    steps: [
      ...boot,
      open('b'),
      tool('form-text'),
      gap('down'),
      gap('up'),
      ix('registerTool', [
        { id: 'custom-place', cursor: 'crosshair', enables: { $set: ['form-place'] } },
      ]),
      tool('custom-place'),
      ...click(10, 10),
      ix('activateTool', ['form-text'], 'b'),
      ix('dispatchPointer', [sample('down', 10, 10)], 'b'),
      ix('dispatchPointer', [sample('up', 10, 10)], 'b'),
      ix('registerTool', [
        { id: 'form-text', cursor: 'crosshair', enables: { $set: [] } },
        { replace: true },
      ]),
      tool('form-text'),
      ...click(10, 10),
    ],
  },
  scenario(
    'auto-select-waits-for-a-stable-world',
    'Reserved object numbers make pending selection immediate; switching tools clears it. The next two immediate engine answers publish another-page/no-widget facts synchronously during stage/apply before form.get, so those authored outcomes stay unselected.',
    [
      tool('form-text'),
      ...click(),
      tool('pointer'),
      settle(
        'slow',
        fieldAnswer('forms.create', 1, NEW, [
          row(10002, NEW.ref, 1, { x: 20, y: 88, width: 160, height: 24 }, PALETTE_TEXT),
        ]).value,
      ),
      anno('selection.list'),
      tool('form-text'),
      ...click(300, 300),
      anno('selection.list'),
      ...click(500, 500),
      anno('selection.list'),
    ],
    snapshot([]),
    [
      apply(
        fieldAnswer('forms.create', 1, NEW, [
          row(10002, NEW.ref, 1, { x: 20, y: 88, width: 160, height: 24 }, PALETTE_TEXT),
        ]),
        { defer: 'slow' },
      ),
      apply(
        fieldAnswer('forms.create', 2, { ...NEW2, widgets: [placed(10004, 2)] }, [
          row(10004, NEW2.ref, 2, BOX, PALETTE_TEXT),
        ]),
      ),
      apply(fieldAnswer('forms.create', 3, text(10005, 'text_3', '', {}, []))),
    ],
    {},
    true,
  ),
  scenario(
    'a-failed-placement-is-logged',
    'The current handler silently skips too-thin clamped geometry and unknown pages, and contains an exact engine refusal via console.error.',
    [
      tool('form-text'),
      pointer('down', 50, 50),
      pointer('up', 150, 50.5),
      pointer('down', 50, 50, {}, 99),
      pointer('up', 50, 50, {}, 99),
      ...click(50, 50),
      call('list'),
    ],
    snapshot([]),
    [refused('intended placement refusal')],
    {},
    true,
  ),
];
export const suites: ScenarioSuite[] = [
  {
    kind: 'scenario',
    file: 'plugins/form-design.json',
    describes:
      'Current form tools, direct design verbs, widget lending, page-space hits and pointer placement.',
    cases,
  },
];
