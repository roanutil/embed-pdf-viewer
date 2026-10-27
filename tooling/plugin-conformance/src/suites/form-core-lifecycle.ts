/** Surviving status and pending-write semantics at their actual mirror/queue owner. */
import type { ScenarioCase } from '../scenario';
import type { ScenarioSuite } from '../vectors';
import { call, list, snapshot, text, row, ref, valueSet } from './form-current-fixtures';
import { boot, open, doc, settle, fail, pr } from './annotation-live-fixtures';
const NAME = text(1, 'name', 'Ada');
const WIDGET = row(101, NAME.ref, 1, { x: 10, y: 20, width: 100, height: 24 });
const FORM = snapshot([NAME], [WIDGET]);
const NEXT = text(1, 'name', 'Grace');
const PLUGINS = [
  { factory: 'interaction' },
  { factory: 'form-current', config: { validation: 'none' } },
];
const cases: ScenarioCase[] = [
  {
    name: 'formReducer/set-status',
    covers:
      'The real fields mirror is loading before its deferred list answers, then ready; no session SET_STATUS exists.',
    plugins: PLUGINS,
    engine: { documents: { a: doc([list(FORM, { defer: 'form-load' })]) } },
    snapshot: ['form::a'],
    steps: [
      ...boot,
      call('getStatus'),
      call('getSnapshot'),
      settle('form-load', FORM),
      call('getStatus'),
      call('getSnapshot'),
    ],
  },
  {
    name: 'formReducer/set-status-unchanged-is-no-change',
    covers:
      'A ready read stays ready across a refresh with identical fields; the empty session state emits no changes.',
    plugins: PLUGINS,
    engine: { documents: { a: doc([list(FORM)]) } },
    snapshot: ['form::a'],
    steps: [...boot, call('getStatus'), call('refresh'), call('getStatus'), call('getSnapshot')],
  },
  {
    name: 'current-form-confirmation-refusal-and-permission',
    covers:
      'Without annotation, a fill is predicted, confirmed once before resolution, then a refused fill leaves the confirmed value. A second document denies fill locally.',
    plugins: PLUGINS,
    engine: {
      documents: {
        a: doc([
          list(FORM),
          { path: 'apply', times: 1, defer: 'confirm', ...valueSet(1, NEXT, [WIDGET]) },
          { path: 'apply', times: 1, defer: 'refuse' },
        ]),
        b: doc([list(FORM)], {
          security: { allows: ['doc.forms.read'], allowsField: { fill: false } },
        }),
      },
    },
    snapshot: ['form::a', 'form::b'],
    steps: [
      ...boot,
      call('onValueChanged', [{ $fn: 'valueChanged' }]),
      call('canFill', [ref(1)]),
      call('getValue', [ref(1)]),
      call('setValue', [ref(1), { value: 'Grace' }]),
      call('getValue', [ref(1)]),
      call('listWidgets', [pr(1)]),
      settle('confirm', valueSet(1, NEXT, [WIDGET]).value),
      call('getValue', [ref(1)]),
      call('setValue', [ref(1), { value: 'refused' }]),
      call('getValue', [ref(1)]),
      fail('refuse', 'intended form refusal'),
      call('getValue', [ref(1)]),
      open('b'),
      call('canFill', [ref(1)], 'b'),
      call('setValue', [ref(1), { value: 'denied' }], 'b'),
      call('getValue', [ref(1)], 'b'),
      call('validate'),
      call('exportValues'),
    ],
  },
];
export const suites: ScenarioSuite[] = [
  {
    kind: 'scenario',
    file: 'plugins/form-core-lifecycle.json',
    describes:
      'Current form mirror status, actual pending confirmation/refusal and field permission gate.',
    cases,
  },
];
