/**
 * `packages/plugin/form` beside the actions plugin, scripting off: the
 * `reset-form` executor, the submit dataset resolver and the script commit
 * sink it registers, the host-lens doors (`resetFormAction`,
 * `resolveSubmitDataset`, `commitScriptFormEffects`), widget activation and
 * the widget DOM-event feed. Replayed by
 * `crates/plugins/form/tests/conformance.rs`.
 */
import type { ResponseSpec } from '../fake-engine';
import type { ScenarioCase, Step } from '../scenario';
import type { Suite } from '../vectors';

import {
  FORM_ONLY,
  SNAP,
  act,
  annotations,
  aref,
  boot,
  byName,
  byNumber,
  call,
  checkbox,
  doc,
  effectsOk,
  fnSpec,
  js,
  list,
  pushbutton,
  ref,
  resetNode,
  signature,
  text,
  tree,
  widgetAnnot,
} from './form-fixtures';

const OFF = { openSequence: 'off' };
const HYDRATE: ResponseSpec = { path: 'annotations.listRawAll', value: { pages: [] } };
const WITH_ACTIONS = [{ factory: 'interaction' }, { factory: 'actions', config: OFF }, { factory: 'form' }];
const WITH_BOTH = [{ factory: 'interaction' }, { factory: 'actions', config: OFF }, { factory: 'annotation' }, { factory: 'form' }];

const USER = { origin: 'user', source: { kind: 'api' }, event: { scope: 'activate' } };
const LIFECYCLE = { origin: 'lifecycle', source: { kind: 'api' }, event: { scope: 'document', name: 'open' } };

const C1 = text(1, 'parent.c1', 'one');
const C2 = text(2, 'parent.c2', 'two');
const OTHER = text(3, 'other', 'three');
const BUTTON = pushbutton(4, 'button');
const SIG = signature(5, 'sig');
const AGREE = checkbox(6, 'agree', true);
const FIELDS = [C1, C2, OTHER, BUTTON, SIG, AGREE];

const SUBMIT_FLAGS = {
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
const submitNode = (payload: Record<string, unknown>) => ({
  type: 'submit-form',
  subtype: 'SubmitForm',
  payload: { url: 'https://home.test/submit', fields: null, flags: SUBMIT_FLAGS, ...payload },
  next: [],
});
const intent = (over: Record<string, unknown> = {}) => ({
  url: 'https://home.test/x',
  fields: null,
  exclude: false,
  includeNoValueFields: false,
  format: 'fdf',
  method: 'post',
  flagsRaw: 0,
  ...over,
});

const execute = (node: Record<string, unknown>, ctx: unknown = USER): Step => act('execute', [tree(node), ctx]);

const executorCases: ScenarioCase[] = [
  {
    name: 'the-reset-form-executor',
    covers:
      'A ResetForm node resets through the form: a parent name selects its descendants, exclude takes the complement without buttons or signatures, null takes every resettable field, and the batch is one engine call followed by a re-read.',
    plugins: WITH_ACTIONS,
    engine: { documents: { a: doc([list(FIELDS), { path: 'forms.applyEffects', value: effectsOk(1) }]) } },
    snapshot: SNAP,
    steps: [
      ...boot,
      execute(resetNode([byName('parent')])),
      execute(resetNode([byName('parent'), byNumber(6)], true)),
      execute(resetNode(null)),
      execute(resetNode([byNumber(3)])),
    ],
  },
  {
    name: 'an-inert-reset',
    covers:
      "A selection with nothing resettable never reaches the engine and the node is inert with 'reset-form resolved no fields'; an engine with no batch door surfaces an unsupported-api diagnostic and the node is inert with its message.",
    plugins: WITH_ACTIONS,
    engine: {
      documents: {
        a: doc([list(FIELDS)]),
        b: doc([list(FIELDS)], { lacks: ['forms.applyEffects'] }),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }] },
      act('onScriptDiagnostic', [fnSpec('onScriptDiagnostic')], 'b'),
      execute(resetNode([])),
      execute(resetNode([byName('button'), byName('sig')])),
      act('execute', [tree(resetNode(null)), USER], 'b'),
    ],
  },
  {
    name: 'a-failed-reset-fails-the-node',
    covers: "A batch whose reset entry failed or was rejected fails the node with 'reset failed'; a thrown batch fails it with the engine error.",
    plugins: WITH_ACTIONS,
    engine: {
      documents: {
        a: doc([
          list(FIELDS),
          { path: 'forms.applyEffects', times: 1, value: { results: [{ index: 0, status: 'rejected', fields: [], changedWidgets: [] }], changedWidgets: [], meta: null } },
          { path: 'forms.applyEffects', error: { name: 'EngineError', message: 'batch refused', code: 'InvalidArg' } },
        ]),
      },
    },
    snapshot: 'none',
    steps: [...boot, act('onDiagnostic', [fnSpec('onDiagnostic')]), execute(resetNode(null)), execute(resetNode(null))],
  },
  {
    name: 'reset-form-action-door',
    covers:
      "resetFormAction reads the snapshot fresh, resets the selection as one batch and resolves the commit result; the origin defaults to 'user'. A failed list rejects.",
    plugins: WITH_ACTIONS,
    engine: {
      documents: {
        a: doc([
          list(FIELDS, { times: 5 }),
          { path: 'forms.list', times: 1, error: { name: 'EngineError', message: 'gone', code: 'Io' } },
          list(FIELDS),
          { path: 'forms.applyEffects', value: effectsOk(1) },
        ]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      call('resetFormAction', [[byName('other')], false]),
      call('resetFormAction', [null, false, 'lifecycle']),
      call('resetFormAction', [[], true, 'hover']),
    ],
  },
];

const submitCases: ScenarioCase[] = [
  {
    name: 'the-submit-resolver',
    covers:
      'A SubmitForm node resolves its dataset through the form: a fresh read, the ISO selection and value rules, and the explicit-listing diagnostics; charSet rides along only when the intent has one.',
    plugins: WITH_ACTIONS,
    engine: {
      documents: {
        a: doc([
          list([C1, C2, OTHER, BUTTON, { ...text(7, 'secret', 's'), flags: { readOnly: false, required: false, noExport: true, raw: 4 } }]),
        ]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      act('onDiagnostic', [fnSpec('onDiagnostic')]),
      act('setSubmitHandler', [fnSpec('submitHandler')]),
      execute(submitNode({ fields: [byName('parent'), byName('button'), byName('secret')] })),
      execute(submitNode({ charSet: 'utf-8' })),
    ],
  },
  {
    name: 'resolve-submit-dataset-door',
    covers:
      "resolveSubmitDataset builds the request from the intent and the action context's origin and event; diagnostics go to the given sink; a failed read rejects.",
    plugins: WITH_ACTIONS,
    engine: {
      documents: {
        a: doc([list([C1, BUTTON], { times: 3 }), { path: 'forms.list', error: { name: 'EngineError', message: 'gone', code: 'Io' } }]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      call('resolveSubmitDataset', [intent({ fields: [byName('button')] }), USER, fnSpec('diagnose')]),
      call('resolveSubmitDataset', [intent({ charSet: 'utf-16', includeNoValueFields: true }), LIFECYCLE, fnSpec('diagnose')]),
      call('resolveSubmitDataset', [intent(), USER, fnSpec('diagnose')]),
    ],
  },
];

const sinkCases: ScenarioCase[] = [
  {
    name: 'commit-script-form-effects',
    covers:
      "The script commit sink applies the batch, re-reads, and reloads each annotation page a changed widget is on (once, direct widgets skipped); a refused batch and an engine with no batch door both resolve as all-failed, never a throw.",
    plugins: WITH_BOTH,
    engine: {
      documents: {
        a: doc([
          list(FIELDS),
          HYDRATE,
          annotations(1, [widgetAnnot(1, 101)]),
          annotations(2, []),
          {
            path: 'forms.applyEffects',
            times: 1,
            value: effectsOk(2, [
              { annotObjectNumber: 101, pageObjectNumber: 1 },
              { annotObjectNumber: 102, pageObjectNumber: 2 },
              { annotObjectNumber: 103, pageObjectNumber: 1 },
              { annotObjectNumber: 0, pageObjectNumber: 0 },
            ]),
          },
          { path: 'forms.applyEffects', error: { name: 'PermissionDenied', message: 'permission denied: doc.forms.fill', required: 'doc.forms.fill' } },
        ]),
        b: doc([list(FIELDS), HYDRATE], { lacks: ['forms.applyEffects'] }),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }] },
      call('commitScriptFormEffects', [[{ kind: 'setValue', ref: ref(1), value: { type: 'text', value: 'x' } }, { kind: 'setDisplay', ref: ref(2), display: 'hidden' }]]),
      call('commitScriptFormEffects', [[{ kind: 'setValue', ref: ref(1), value: { type: 'text', value: 'y' } }]]),
      call('commitScriptFormEffects', [[{ kind: 'setDisplay', ref: ref(1), display: 'hidden' }, { kind: 'reset', refs: [ref(2)] }]], 'b'),
    ],
  },
  {
    name: 'commit-sink-without-annotation',
    covers: 'Without the annotation plugin the sink re-reads and has no plane to reload.',
    plugins: WITH_ACTIONS,
    engine: {
      documents: {
        a: doc([list(FIELDS), { path: 'forms.applyEffects', value: effectsOk(1, [{ annotObjectNumber: 101, pageObjectNumber: 1 }]) }]),
      },
    },
    snapshot: 'none',
    steps: [...boot, call('commitScriptFormEffects', [[{ kind: 'setValue', ref: ref(1), value: { type: 'text', value: 'x' } }]])],
  },
];

const W1 = aref(1, 101);
const activation = (pon: number, n: number, trees: Record<string, unknown>) => widgetAnnot(pon, n, undefined, { actions: trees });

const widgetCases: ScenarioCase[] = [
  {
    name: 'activation-without-actions',
    covers: "Without the actions plugin activateWidget takes the form path, which is an unscripted no-op: kind 'form', status unchanged.",
    plugins: FORM_ONLY,
    engine: { documents: { a: doc([list(FIELDS)]) } },
    snapshot: 'none',
    steps: [...boot, call('activateWidget', ['obj:1', W1]), call('notifyWidgetEvent', ['obj:1', W1, 'mouseDown'])],
  },
  {
    name: 'activation-is-delegated',
    covers:
      "With the actions plugin activateWidget dispatches the widget's /A tree with a widget source and returns kind 'dispatched'; with no tree at all the form path runs.",
    plugins: WITH_ACTIONS,
    engine: {
      documents: {
        a: doc([
          list(FIELDS),
          { path: 'forms.applyEffects', value: effectsOk(1) },
          annotations(1, [activation(1, 101, { activate: tree(resetNode([byName('other')])) }), widgetAnnot(1, 102)]),
        ]),
      },
    },
    snapshot: 'none',
    steps: [...boot, act('onAction', [fnSpec('onAction')]), call('activateWidget', ['obj:1', W1]), call('activateWidget', ['obj:2', aref(1, 102)])],
  },
  {
    name: 'a-refused-activation-is-dispatched',
    covers: 'A tree the dispatcher refuses (an incomplete one) still counts as dispatched: only a tree-less inert result falls back.',
    plugins: WITH_ACTIONS,
    engine: {
      documents: {
        a: doc([list(FIELDS), annotations(1, [activation(1, 101, { activate: tree(js('x()'), true) })])]),
      },
    },
    snapshot: 'none',
    steps: [...boot, call('activateWidget', ['obj:1', W1])],
  },
  {
    name: 'the-widget-event-feed',
    covers:
      'Without the annotation plugin a cursor enter hovers with unknown flags and the dispatcher resolves the tree; cursor exit hovers nothing; down, up, focus and blur dispatch directly with a widget source.',
    plugins: WITH_ACTIONS,
    engine: {
      documents: {
        a: doc([
          list(FIELDS),
          annotations(1, [
            activation(1, 101, {
              cursorEnter: tree(resetNode([byName('other')])),
              mouseDown: tree(resetNode([byName('other')])),
              focus: tree(resetNode([byName('other')])),
            }),
          ]),
          { path: 'forms.applyEffects', value: effectsOk(1) },
        ]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      act('onAction', [fnSpec('onAction')]),
      call('notifyWidgetEvent', ['obj:1', W1, 'cursorEnter']),
      call('notifyWidgetEvent', ['obj:1', W1, 'cursorExit']),
      call('notifyWidgetEvent', ['obj:1', W1, 'mouseDown']),
      call('notifyWidgetEvent', ['obj:1', W1, 'mouseUp']),
      call('notifyWidgetEvent', ['obj:1', W1, 'focus']),
      call('notifyWidgetEvent', ['obj:1', W1, 'blur']),
    ],
  },
  {
    name: 'hover-pre-checks-the-folded-annotation',
    covers:
      "With the annotation plugin a cursor enter on a loaded widget without enter or exit trees costs no dispatch; one with a tree hovers with its flags; an annotation that isn't a widget, or isn't loaded, hovers with unknown flags.",
    plugins: WITH_BOTH,
    engine: {
      documents: {
        a: doc([
          list(FIELDS),
          {
            path: 'annotations.listRawAll',
            value: {
              pages: [
                {
                  pageState: { pageObjectNumber: 1 },
                  annotations: [
                    activation(1, 101, { cursorExit: tree(resetNode([byName('other')])) }),
                    widgetAnnot(1, 102),
                    { ...widgetAnnot(1, 103), subtype: 'square', color: { r: 1, g: 2, b: 3 }, interiorColor: null, opacity: 1, cloudyIntensity: null, rectDifferences: null },
                  ],
                },
              ],
            },
          },
          annotations(1, [activation(1, 101, { cursorExit: tree(resetNode([byName('other')])) }), widgetAnnot(1, 102)]),
          { path: 'forms.applyEffects', value: effectsOk(1) },
        ]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      act('onAction', [fnSpec('onAction')]),
      call('notifyWidgetEvent', ['obj:2', aref(1, 102), 'cursorEnter']),
      call('notifyWidgetEvent', ['obj:1', W1, 'cursorEnter']),
      call('notifyWidgetEvent', ['obj:1', W1, 'cursorExit']),
      call('notifyWidgetEvent', ['obj:3', aref(1, 103), 'cursorEnter']),
      call('notifyWidgetEvent', ['obj:9', aref(1, 404), 'cursorEnter']),
      call('notifyWidgetEvent', ['obj:9', aref(1, 404), 'cursorExit']),
    ],
  },
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/form-actions.json',
    describes: 'packages/plugin/form beside the actions plugin, scripting off: the reset-form executor, the submit resolver, the script commit sink, widget activation and the widget event feed.',
    cases: [...executorCases, ...submitCases, ...sinkCases, ...widgetCases],
  },
];
