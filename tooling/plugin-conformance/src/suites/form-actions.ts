/** Current form ownership of action reset, submit, script effects and widget events. */
import type { ScenarioCase, Step } from '../scenario';
import type { Suite } from '../vectors';
import type { SubmitIntent, ActionContext } from '../../../../packages/plugin/actions/src/contract';
import type { PdfActionNode, PdfActionTargetRef } from '@embedpdf/engine-core/runtime';
import {
  formScenario,
  text,
  checkbox,
  pushbutton,
  signature,
  ref,
  placed,
  row,
  snapshot,
  call,
  act,
  fnSpec,
  tree,
  js,
  resetNode,
  byName,
  byNumber,
  objRef,
  pr,
  resetAnswer,
  valuesAnswer,
  apply,
  refused,
  FORM_ONLY,
  boot,
  doc,
  list,
} from './form-script-fixtures';
const USER = {
  origin: 'user',
  source: { kind: 'api' },
  event: { scope: 'activate' },
} satisfies ActionContext;
const LIFECYCLE = {
  origin: 'lifecycle',
  source: { kind: 'api' },
  event: { scope: 'document', name: 'open' },
} satisfies ActionContext;
const C1 = text(1, 'parent.c1', 'one'),
  C2 = text(2, 'parent.c2', 'two'),
  OTHER = text(3, 'other', 'three');
const BUTTON = pushbutton(4, 'button'),
  SIG = signature(5, 'sig'),
  AGREE = checkbox(6, 'agree', true);
const FIELDS = [C1, C2, OTHER, BUTTON, SIG, AGREE];
const ROWS = FIELDS.map((field) =>
  row(field.widgets[0]!.objectNumber, field.ref, 1, undefined, { fieldFamily: field.family }),
);
const FORM = snapshot(FIELDS, ROWS);
const cleared = [
  text(1, 'parent.c1', ''),
  text(2, 'parent.c2', ''),
  text(3, 'other', ''),
  checkbox(6, 'agree', false, { valueEntry: { kind: 'scalar', value: 'Off' } }),
];
const reset = (n: number, numbers: number[]) =>
  apply(
    resetAnswer(
      n,
      cleared.filter(
        (field) => field.ref.kind === 'objectNumber' && numbers.includes(field.ref.objectNumber),
      ),
      ROWS.filter(
        (widget) =>
          widget.field?.kind === 'objectNumber' && numbers.includes(widget.field.objectNumber),
      ),
    ),
  );
const execute = (node: PdfActionNode, context: ActionContext = USER): Step =>
  act('execute', [tree(node), context]);
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
const submit = (fields: PdfActionTargetRef[] | null = null, charSet?: string): PdfActionNode => ({
  type: 'submit-form',
  subtype: 'SubmitForm',
  payload: {
    url: 'https://home.test/submit',
    fields,
    flags: {
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
    },
    ...(charSet ? { charSet } : {}),
  },
  next: [],
});
const observe = [
  act('onDiagnosticReported', [fnSpec('onDiagnostic')]),
  act('onExecuted', [fnSpec('onAction')]),
];
const W1 = objRef(101);
const withDeniedDocument = (input: ScenarioCase): ScenarioCase => ({
  ...input,
  engine: {
    ...input.engine,
    documents: {
      ...input.engine?.documents,
      b: doc(
        [
          { path: 'forms.list', value: FORM },
          { path: 'annotations.list', value: { annotations: [] } },
        ],
        {
          security: {
            allows: ['doc.forms.read', 'doc.annotate.read'],
            allowsField: { fill: false },
          },
        },
      ),
    },
  },
});
const cases: ScenarioCase[] = [
  formScenario(
    'the-reset-form-executor',
    'The real reset executor selects a name subtree, an exclusion complement, all resettable fields and an object-number field. Each reset is one current atomic op.',
    FORM,
    [
      ...observe,
      execute(resetNode([byName('parent')])),
      call('getValue', [ref(1)]),
      call('getValue', [ref(2)]),
      execute(resetNode([byName('parent'), byNumber(6)], true)),
      call('getValue', [ref(3)]),
      execute(resetNode(null)),
      call('getValue', [ref(6)]),
      execute(resetNode([byNumber(3)])),
      call('getSnapshot'),
    ],
    [reset(1, [1, 2]), reset(2, [3]), reset(3, [6]), apply(resetAnswer(4, [], [], false))],
  ),
  formScenario(
    'an-inert-reset',
    'Empty or valueless selections are inert. A real atomic engine refusal fails a selected reset and leaves every public value unchanged.',
    FORM,
    [
      ...observe,
      execute(resetNode([])),
      execute(resetNode([byName('button'), byName('sig')])),
      execute(resetNode(null)),
      call('exportValues'),
    ],
    [refused('reset unavailable', 'Unsupported')],
  ),
  formScenario(
    'a-failed-reset-fails-the-node',
    'Atomic reset refusal fails the action node with the engine message; no selected field changes and a later refused reset remains observable.',
    FORM,
    [
      ...observe,
      execute(resetNode(null)),
      call('exportValues'),
      execute(resetNode(null)),
      call('exportValues'),
    ],
    [refused('reset rejected', 'Refused'), refused('batch refused', 'InvalidArg')],
  ),
  formScenario(
    'reset-form-action-door',
    'The host reset uses the current mirrored form, defaults origin to user and does not force engine reads. Failed atomic reset rejects and preserves confirmed fields.',
    FORM,
    [
      call('resetFormAction', [[byName('other')], false]),
      call('getValue', [ref(3)]),
      call('resetFormAction', [null, false, 'lifecycle']),
      call('exportValues'),
      call('resetFormAction', [[], true, 'hover']),
      call('exportValues'),
    ],
    [reset(1, [3]), refused('gone', 'Io'), reset(3, [1, 2, 6])],
  ),
  formScenario(
    'the-submit-resolver',
    'SubmitForm resolves its post-fill dataset through the registered form reader. Unsupported explicit buttons and NoExport fields are diagnosed; charSet is forwarded.',
    snapshot([...FIELDS, text(7, 'secret', 's', { noExport: true })], ROWS),
    [
      ...observe,
      act('setSubmitHandler', [fnSpec('submitHandler')]),
      execute(submit([byName('parent'), byName('button'), byName('secret')])),
      execute(submit(null, 'utf-8')),
    ],
  ),
  formScenario(
    'resolve-submit-dataset-door',
    'The host dataset uses current values and provenance, forwards charSet and explicit diagnostics, and reads no new engine snapshot after hydration.',
    FORM,
    [
      call('resolveSubmitDataset', [
        intent({ fields: [byName('button')] }),
        USER,
        fnSpec('diagnose'),
      ]),
      call('resolveSubmitDataset', [
        intent({ charSet: 'utf-16', includeNoValueFields: true }),
        LIFECYCLE,
        fnSpec('diagnose'),
      ]),
      call('resolveSubmitDataset', [intent(), USER, fnSpec('diagnose')]),
    ],
  ),
  withDeniedDocument(
    formScenario(
      'commit-script-form-effects',
      'The script sink stages current ops immediately. Native refusal is reported by the queue; per-field authority refuses staging synchronously with a failed sink result.',
      snapshot([C1, { ...C2, readOnly: true }], ROWS.slice(0, 2)),
      [
        ...observe,
        {
          op: 'call',
          target: 'documents',
          method: 'open',
          args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }],
        },
        call('commitScriptFormEffects', [
          [{ kind: 'setValue', ref: ref(1), value: { value: 'x' } }],
        ]),
        call('getValue', [ref(1)]),
        call('commitScriptFormEffects', [
          [{ kind: 'setValue', ref: ref(1), value: { value: 'y' } }],
        ]),
        call('getValue', [ref(1)]),
        call(
          'commitScriptFormEffects',
          [
            [
              { kind: 'setDisplay', ref: ref(2), display: 'hidden' },
              { kind: 'reset', refs: [ref(1)] },
            ],
          ],
          'b',
        ),
        call('getValue', [ref(1)]),
      ],
      [
        apply(valuesAnswer(1, [text(1, 'parent.c1', 'x')], [ROWS[0]!])),
        refused('script write refused', 'Refused'),
      ],
      undefined,
      true,
    ),
  ),
  formScenario(
    'commit-sink-without-annotation',
    'The same native sink confirms through form events without annotation and never reloads annotation pages.',
    FORM,
    [
      call('commitScriptFormEffects', [[{ kind: 'setValue', ref: ref(1), value: { value: 'x' } }]]),
      call('getValue', [ref(1)]),
      call('getSnapshot'),
    ],
    [apply(valuesAnswer(1, [text(1, 'parent.c1', 'x')], [ROWS[0]!]))],
  ),
  {
    name: 'activation-without-actions',
    covers:
      'Without actions the current activateWidget door resolves its field from the object-number relationship and returns an unscripted unchanged form result; DOM events are inert.',
    plugins: FORM_ONLY,
    engine: { documents: { a: doc([{ path: 'forms.list', value: FORM }]) } },
    snapshot: ['form::a'],
    steps: [
      ...boot,
      call('activateWidget', [W1]),
      call('notifyWidgetEvent', [ref(1), W1, 'mouseDown']),
    ],
  },
  formScenario(
    'activation-is-delegated',
    'Activation dispatches the current form row action; absent or rootless actions fall back to the form owner, without extra page reads.',
    snapshot(FIELDS, [
      row(101, ref(1), 1, undefined, { actions: { activate: tree(resetNode([byName('other')])) } }),
      row(102, ref(2)),
      row(103, ref(3), 1, undefined, { actions: { activate: tree(null) } }),
      ...ROWS.slice(3),
    ]),
    [
      ...observe,
      call('activateWidget', [W1]),
      call('activateWidget', [objRef(102)]),
      call('activateWidget', [objRef(103)]),
      call('getValue', [ref(3)]),
    ],
    [reset(1, [3])],
  ),
  formScenario(
    'a-refused-activation-is-dispatched',
    'An incomplete current widget action is refused by the dispatcher and still returns kind dispatched; the form fallback never runs.',
    snapshot(FIELDS, [
      row(101, ref(1), 1, undefined, { actions: { activate: tree(js('x()'), true) } }),
      ...ROWS.slice(1),
    ]),
    [call('activateWidget', [W1])],
  ),
  formScenario(
    'the-widget-event-feed',
    'Current form rows supply hover flags and action trees. Enter, exit, down, up, focus and blur carry the real widget source through the actions queue.',
    snapshot(FIELDS, [
      row(101, ref(1), 1, undefined, {
        actions: {
          cursorEnter: tree(resetNode([byName('other')])),
          mouseDown: tree(resetNode([byName('other')])),
          focus: tree(resetNode([byName('other')])),
        },
      }),
      ...ROWS.slice(1),
    ]),
    [
      ...observe,
      ...(['cursorEnter', 'cursorExit', 'mouseDown', 'mouseUp', 'focus', 'blur'] as const).map(
        (event) => call('notifyWidgetEvent', [ref(1), W1, event]),
      ),
      call('getValue', [ref(3)]),
    ],
    [reset(1, [3]), apply(resetAnswer(2, [], [], false)), apply(resetAnswer(3, [], [], false))],
  ),
  formScenario(
    'hover-pre-checks-the-folded-annotation',
    'The current form-row mirror owns hover flags even with annotation lending. Rows without trees skip dispatch; an exit tree dispatches on hover exit; unknown rows read the page.',
    snapshot(FIELDS, [
      row(101, ref(1), 1, undefined, {
        actions: { cursorExit: tree(resetNode([byName('other')])) },
      }),
      ...ROWS.slice(1),
    ]),
    [
      ...observe,
      call('notifyWidgetEvent', [ref(2), objRef(102), 'cursorEnter']),
      call('notifyWidgetEvent', [ref(1), W1, 'cursorEnter']),
      call('notifyWidgetEvent', [ref(1), W1, 'cursorExit']),
      call('notifyWidgetEvent', [ref(3), objRef(103), 'cursorEnter']),
      call('notifyWidgetEvent', [ref(3), objRef(404), 'cursorEnter']),
      call('notifyWidgetEvent', [ref(3), objRef(404), 'cursorExit']),
      call('getValue', [ref(3)]),
    ],
    [reset(1, [3])],
    undefined,
    true,
  ),
];
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/form-actions.json',
    describes: 'Current native form/action integration and exact public effects.',
    cases,
  },
];
