/** Current K/V/C/F transactions over complete form snapshots, plus native form writes. */
import type { ResponseSpec } from '../fake-engine';
import type { PluginSpec, ScenarioCase, Step } from '../scenario';
import type { Suite } from '../vectors';

import {
  act,
  advance,
  boot,
  byName,
  call,
  checkbox,
  combobox,
  doc,
  fnSpec,
  js,
  list,
  listbox,
  pushbutton,
  radio,
  ref,
  resetNode,
  settle,
  standalone,
  text,
  tree,
  standalonePlugins,
  type StandaloneConfig,
  pr,
  placed,
  snapshot,
  formScenario,
  row,
  objRef,
  valuesAnswer,
  resetAnswer,
  apply,
  mutationMeta,
  operationAt,
  LOCAL,
  refused,
} from './form-script-fixtures';
import type {
  ChangeResult,
  DocumentEvent,
  FormEffect,
  FormFieldValue,
  FormFieldDTO,
  FormSnapshot,
  Identity,
  DocumentActionsSnapshot,
} from '@embedpdf/engine-core/runtime';
import type {
  ScriptOutput,
  ScriptEventOutput,
  ScriptValue,
} from '../../../../packages/core/acrojs/src/index';
import type { SandboxSpec } from './form-script-fixtures';
import { FormSnapshotSchema } from '../../../../packages/engine/core/src/forms/schema';

/** Why these cases record no state; see `ScenarioCase` in `../scenario`. */
const UNSNAPSHOTTED = {
  snapshot: 'none',
  reason:
    'Pins the script transaction (what crosses into the sandbox and what commits), through settled results, sandbox callbacks and engine calls. Its dispatched actions are compared; the state they build is not.',
} as const;

// ── sandbox answers ───────────────────────────────────────────────────────────

const ev = (value: ScriptValue = null, over: Partial<ScriptEventOutput> = {}) => ({
  rc: true,
  value,
  change: '',
  selStart: 0,
  selEnd: 0,
  ...over,
});
const out = (over: Partial<ScriptOutput> = {}) => ({ output: over });
const rejects = (value: ScriptValue = null) => out({ event: ev(value, { rc: false }) });
const exception = (message = 'boom') => out({ error: { kind: 'exception', message } });
const budgetFault = (message = 'synthetic budget fault') =>
  out({ error: { kind: 'budget', message } });
const setValue = (n: number, value: FormFieldValue): FormEffect => ({
  kind: 'setValue',
  ref: ref(n),
  value,
});
const textValue = (value: string) => ({ value });
const alert = (
  message: string,
  icon = 0,
): Extract<ScriptOutput['uiEffects'][number], { kind: 'alert' }> => ({
  kind: 'alert',
  message,
  icon,
});

const ACTIONS_READ = (scripts: string[] = ['var lib = 1;']): ResponseSpec => ({
  path: 'actions.get',
  value: {
    nameTreeScripts: scripts.map((script, i) => ({ name: `lib${i}`, action: tree(js(script)) })),
    openAction: null,
    openDestination: null,
  } satisfies DocumentActionsSnapshot,
});

const kvcf = (over: NonNullable<FormFieldDTO['actions']>) => ({ actions: over });
const K = (script = 'AFNumber_Keystroke(2)') => tree(js(script));

/** A standalone driver case: the driver over document `a`. */
const solo = (config: Partial<StandaloneConfig> & { sandbox: StandaloneConfig['sandbox'] }) =>
  standalonePlugins(config);
const commit = (n: number, value: FormFieldValue) => standalone('commit', [ref(n), value]);

const NAME = text(1, 'name', 'Ada');
const AMOUNT = text(2, 'amount', '1', kvcf({ keystroke: K(), validate: tree(js('validate()')) }));
const TOTAL = text(
  3,
  'total',
  '2',
  kvcf({ calculate: tree(js('calc()')), format: tree(js('fmt()')) }),
);

// ── the standalone controller ─────────────────────────────────────────────────

const environmentCases: ScenarioCase[] = [
  {
    name: 'standalone-configured-environment',
    covers:
      'The standalone host reads the name-tree scripts once, boots once per realm with the configured file name, identity, clock, offset and seed, and returns the value effect; a second commit does not boot again.',
    plugins: solo({
      sandbox: { builds: [{}] },
      now: fnSpec('now', { returns: 1234 }),
      utcOffsetMinutes: fnSpec('offset', { returns: 180 }),
      randomSeed: fnSpec('seed', { returns: 7 }),
      identity: { name: 'Ada', email: 'ada@example.test' },
      fileName: fnSpec('fileName', { returns: 'proposal.pdf' }),
    }),
    engine: { documents: { a: doc([list([NAME]), ACTIONS_READ(['var a = 1;', 'var b = 2;'])]) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, commit(1, textValue('Grace')), commit(1, textValue('Hopper'))],
  },
  {
    name: 'standalone-default-environment',
    covers:
      "Without configuration the clock is the host's, the offset the host zone's, the seed seedFrom(document id, run), the file name the document's, the identity the engine's claims; a document without catalog actions boots nothing but still builds the realm.",
    plugins: solo({ sandbox: { builds: [{}] } }),
    engine: {
      documents: {
        a: doc([list([text(2, 'code', 'old', kvcf({ keystroke: K('up()') }))])], {
          security: {
            allows: '*',
            identity: { userId: 'u-1', displayName: 'Una', groupId: 'g-1' } satisfies Identity,
          },
        }),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [...boot, advance(1500), commit(2, textValue('new'))],
  },
  {
    name: 'standalone-identity-callback',
    covers: 'An identity callback is called per run and layered over the engine claims.',
    plugins: solo({
      sandbox: { builds: [{}] },
      identity: fnSpec('identity', { returns: { corporation: 'Fn Inc' } }),
    }),
    engine: {
      documents: {
        a: doc([list([NAME]), ACTIONS_READ()], {
          security: { allows: '*', identity: { userId: 'u-2', groupId: 'g-2' } satisfies Identity },
        }),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [...boot, standalone('recalculate')],
  },
  {
    name: 'standalone-dispose',
    covers: 'dispose() releases the owned realm; a later transaction rejects.',
    plugins: solo({ sandbox: { builds: [{}] } }),
    engine: { documents: { a: doc([list([NAME]), ACTIONS_READ()]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      commit(1, textValue('x')),
      standalone('dispose'),
      commit(1, textValue('y')),
      standalone('dispose'),
    ],
  },
];

const CHECK = checkbox(4, 'agree', true, kvcf({ validate: tree(js('v()')) }));
const RADIO = radio(
  5,
  'size',
  [
    { ...placed(105), onState: 'S', exportValue: 'S', checked: false },
    { ...placed(106), onState: 'M', exportValue: 'M', checked: false },
  ],
  kvcf({ validate: tree(js('v()')) }),
);
const COMBO = combobox(6, 'colour', 'r', kvcf({ validate: tree(js('v()')) }));
const LIST = listbox(7, 'colours', ['r'], kvcf({ validate: tree(js('v()')) }));
const BUTTON = pushbutton(8, 'go', { widgets: [placed(108), placed(109), placed(110)] });
const VALUE_FIELDS = [
  NAME,
  CHECK,
  RADIO,
  COMBO,
  LIST,
  BUTTON,
  text(9, 'plain', 'p', kvcf({ validate: tree(js('v()')) })),
];
const PLAIN_VALUE_FIELDS = [
  NAME,
  checkbox(4, 'agree', true),
  radio(5, 'size', [{ ...placed(105), onState: 'S', exportValue: 'S', checked: false }]),
  combobox(6, 'colour', 'r'),
  listbox(7, 'colours', ['r']),
  BUTTON,
];

const valueCases: ScenarioCase[] = [
  {
    name: 'form-values-cross-into-the-overlay',
    covers:
      "A toggle proposes its state (null as 'Off'), a combobox its scalar choice (null for none), a listbox every choice; a value whose type does not match the family fails the transaction before any run.",
    plugins: solo({ sandbox: { builds: [{}] } }),
    engine: { documents: { a: doc([list(PLAIN_VALUE_FIELDS)]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      commit(4, { value: 'Yes' }),
      commit(4, { value: null }),
      commit(5, { value: 'M' }),
      commit(6, { value: 'g' }),
      commit(6, { value: null }),
      commit(7, { selectedValues: ['r', 'g'] }),
      commit(1, { checked: true }),
      commit(8, textValue('press')),
      commit(1, { selectedValues: ['x'] }),
    ],
  },
  {
    name: 'a-cleared-toggle-proposes-off',
    covers: "A toggle to null reaches the validator as 'Off'.",
    plugins: solo({ sandbox: { builds: [{ run: [out({ event: ev('Off') })] }] } }),
    engine: { documents: { a: doc([list([CHECK])]) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, commit(4, { value: null })],
  },
  {
    name: 'validated-values-cross-back',
    covers:
      "A validator's event.value becomes the committed value in the family's form: text String()s it (null as ''), a toggle maps null and 'Off' to a cleared state and stringifies the rest, a combobox keeps the first of a list, a listbox wraps a scalar.",
    plugins: solo({
      sandbox: {
        builds: [
          {
            run: [
              out({ event: ev(null) }),
              out({ event: ev(null) }),
              out({ event: ev('Off') }),
              out({ event: ev(5) }),
              out({ event: ev(['g', 'r']) }),
              out({ event: ev(null) }),
              out({ event: ev('g') }),
              out({ event: ev(null) }),
              out({ event: ev(true) }),
            ],
          },
        ],
      },
    }),
    engine: { documents: { a: doc([list(VALUE_FIELDS)]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      commit(9, textValue('typed')),
      commit(4, { value: 'Yes' }),
      commit(4, { value: 'Yes' }),
      commit(4, { value: 'Yes' }),
      commit(6, { value: 'g' }),
      commit(6, { value: 'g' }),
      commit(7, { selectedValues: ['g'] }),
      commit(7, { selectedValues: ['g'] }),
      commit(5, { value: 'S' }),
    ],
  },
];

const bootCases: ScenarioCase[] = [
  {
    name: 'boot-effects-land-with-the-commit',
    covers:
      "A boot's form effects and UI requests join the first transaction, tagged 'boot', ahead of the user's value; later transactions see no boot.",
    plugins: solo({
      sandbox: {
        builds: [
          {
            boot: [
              out({
                formEffects: [setValue(1, textValue('initialized'))],
                uiEffects: [alert('Needs a newer viewer', 1)],
              }),
            ],
          },
        ],
      },
    }),
    engine: { documents: { a: doc([list([NAME, text(2, 'other', 'o')]), ACTIONS_READ()]) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, commit(2, textValue('mine')), commit(2, textValue('again'))],
  },
  {
    name: 'a-failed-boot-degrades',
    covers:
      'A boot that throws, or answers with an error, is a script-error diagnostic and the transaction goes on without its effects; its UI requests still surface.',
    plugins: solo({
      sandbox: {
        builds: [{ boot: [{ throws: { message: 'app.findComponent is not a function' } }] }, {}],
      },
    }),
    engine: { documents: { a: doc([list([NAME]), ACTIONS_READ()]) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, commit(1, textValue('a'))],
  },
  {
    name: 'a-boot-error-output-degrades',
    covers:
      'A boot output carrying an error keeps its UI requests and diagnostics, drops its form effects, and adds the degrade diagnostic.',
    plugins: solo({
      sandbox: {
        builds: [
          {
            boot: [
              out({
                formEffects: [setValue(1, textValue('never'))],
                uiEffects: [alert('nag')],
                diagnostics: [{ code: 'unsupported-api', message: 'app.findComponent' }],
                error: { kind: 'exception', message: 'not a function' },
              }),
            ],
          },
        ],
      },
    }),
    engine: { documents: { a: doc([list([NAME, text(2, 'other', 'o')]), ACTIONS_READ()]) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, commit(2, textValue('b'))],
  },
  {
    name: 'a-failed-catalog-read-degrades',
    covers:
      'A name-tree read that rejects degrades to the host diagnostic; the value effect still returns.',
    plugins: solo({ sandbox: { builds: [{}] } }),
    engine: {
      documents: {
        a: doc([
          list([NAME]),
          {
            path: 'actions.get',
            error: { name: 'EngineError', message: 'actions unavailable', code: 'Io' },
          },
        ]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [...boot, commit(1, textValue('a'))],
  },
  {
    name: 'rejection-keeps-boot-effects',
    covers:
      'Every rejected controller transaction returns only its boot effects for the caller to write. Native atomic refusal is covered through the form capability.',
    plugins: solo({
      sandbox: {
        builds: [
          {
            boot: [
              out({
                formEffects: [setValue(1, textValue('booted'))],
                diagnostics: [{ code: 'unsupported-api', message: 'app.viewerVersion' }],
              }),
            ],
            run: [rejects('x')],
          },
        ],
      },
    }),
    engine: {
      documents: {
        a: doc([
          list([NAME, text(2, 'guarded', '', kvcf({ validate: tree(js('no()')) }))]),
          ACTIONS_READ(),
        ]),
        b: doc([
          list([NAME, text(2, 'guarded', '', kvcf({ validate: tree(js('no()')) }))]),
          ACTIONS_READ(),
        ]),
        c: doc([
          list([NAME, text(2, 'guarded', '', kvcf({ validate: tree(js('no()')) }))]),
          ACTIONS_READ(),
        ]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      {
        op: 'call',
        target: 'documents',
        method: 'open',
        args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }],
      },
      {
        op: 'call',
        target: 'documents',
        method: 'open',
        args: [{ kind: 'bytes', id: 'c', bytes: { $bytes: '' } }],
      },
      commit(2, textValue('x')),
      standalone('commit', [ref(2), textValue('x')], 'b'),
      standalone('commit', [ref(2), textValue('x')], 'c'),
    ],
  },
];

const activationCases: ScenarioCase[] = [
  {
    name: 'activation-runs-in-the-transaction',
    covers:
      "activate(ref, tree) runs the tree's JavaScript as widget-activate against the overlay and returns its effects with its UI requests; no calculation follows.",
    plugins: solo({
      sandbox: {
        builds: [
          {
            run: [
              out({
                formEffects: [setValue(1, textValue('Mouse Up:Field'))],
                uiEffects: [alert('Summary ready'), { kind: 'gotoPage', page: 1 }],
              }),
            ],
          },
        ],
      },
    }),
    engine: { documents: { a: doc([list([NAME, BUTTON, TOTAL], {}, [ref(3)])]) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, standalone('activate', [ref(8), tree(js('go()', [js('then()')]))])],
  },
  {
    name: 'activation-edits-its-own-field',
    covers:
      "An activation that sets its own field's value returns that effect; the activated field is not formatted, since no value was proposed.",
    plugins: solo({
      sandbox: { builds: [{ run: [out({ formEffects: [setValue(9, textValue('mine'))] })] }] },
    }),
    engine: {
      documents: { a: doc([list([text(9, 'self', 'old', kvcf({ format: tree(js('fmt()')) }))])]) },
    },
    ...UNSNAPSHOTTED,
    steps: [...boot, standalone('activate', [ref(9), tree(js('me()'))])],
  },
  {
    name: 'recalculate-keeps-a-boot-reset',
    covers:
      "recalculate() proposes nothing, so a boot script's reset of the anchor field survives as a reset effect.",
    plugins: solo({
      sandbox: { builds: [{ boot: [out({ formEffects: [{ kind: 'reset', refs: [ref(4)] }] })] }] },
    }),
    engine: {
      documents: {
        a: doc([
          list(
            [text(4, 'total', 'changed', { defaultValueEntry: { kind: 'scalar', value: 'dflt' } })],
            {},
            [ref(4)],
          ),
          ACTIONS_READ(),
        ]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [...boot, standalone('recalculate')],
  },
  {
    name: 'activation-refusals',
    covers:
      'An incomplete tree fails before any run; a tree with no JavaScript returns no effects; a run error fails the transaction.',
    plugins: solo({ sandbox: { builds: [{ run: [exception('activation broke')] }] } }),
    engine: { documents: { a: doc([list([NAME, BUTTON])]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      standalone('activate', [ref(8), tree(js('x()'), true)]),
      standalone('activate', [ref(8), tree(null)]),
      standalone('activate', [ref(8), tree(resetNode(null))]),
      standalone('activate', [ref(8), tree(js('x()'))]),
    ],
  },
];

const CODE = (over: NonNullable<FormFieldDTO['actions']> = {}, value = 'old') =>
  text(2, 'code', value, kvcf({ keystroke: K('ks()'), ...over }));

const keystrokeCases: ScenarioCase[] = [
  {
    name: 'keystroke-two-passes',
    covers:
      'A text commit runs the keystroke script twice: once paste-shaped (the whole value in change, the old value selected, willCommit false), whose selection splice becomes the proposal, then willCommit with the spliced value, whose event.value is committed.',
    plugins: solo({
      sandbox: {
        builds: [
          {
            run: [
              out({
                event: ev('old', { change: 'ABC', selStart: 0, selEnd: 3 }),
                formEffects: [setValue(1, textValue('typed-into'))],
              }),
              out({ event: ev('ABC!') }),
            ],
          },
        ],
      },
    }),
    engine: { documents: { a: doc([list([NAME, CODE()])]) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, commit(2, textValue('abc'))],
  },
  {
    name: 'keystroke-splices-in-utf-16',
    covers:
      'The splice counts UTF-16 code units (an astral character is two), clamps a selection end before its start, and reads negative and fractional offsets the way String#slice does.',
    plugins: solo({
      sandbox: {
        builds: [
          {
            run: [
              out({ event: ev('😀x', { change: 'Y', selStart: 2, selEnd: 3 }) }),
              { output: { event: { rc: true, value: '😀Y', change: '', selStart: 0, selEnd: 0 } } },
              out({ event: ev('abcd', { change: '-', selStart: 3, selEnd: 1 }) }),
              out({ event: ev('abc-d') }),
              out({ event: ev('abcd', { change: '+', selStart: -1, selEnd: -1 }) }),
              out({ event: ev('abc+d') }),
              out({ event: ev('abcd', { change: '*', selStart: 1.5, selEnd: 2.9 }) }),
              out({ event: ev('a*cd') }),
              out({ event: ev(42, { change: '!', selStart: 1, selEnd: 1 }) }),
              out({ event: ev(null) }),
            ],
          },
        ],
      },
    }),
    engine: { documents: { a: doc([list([CODE({}, '😀x')])]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      commit(2, textValue('z')),
      commit(2, textValue('z')),
      commit(2, textValue('z')),
      commit(2, textValue('z')),
      commit(2, textValue('z')),
    ],
  },
  {
    name: 'keystroke-empty-values',
    covers:
      "An empty field's old value and a null proposal both reach the keystroke script as ''; the field's page is its first placed widget's index in the document, 0 when it has none or the page is unknown; a direct widget before it is passed over.",
    plugins: solo({ sandbox: { builds: [{}] } }),
    engine: {
      documents: {
        a: doc([
          list([
            text(2, 'code', '', kvcf({ keystroke: K('ks()') }), [placed(102, 2)]),
            text(3, 'unplaced', '', kvcf({ keystroke: K('ks()') }), [placed(103, null)]),
            text(5, 'second', '', kvcf({ keystroke: K('ks()') }), [
              placed(106, null),
              placed(105, 2),
            ]),
            text(4, 'lost', '', kvcf({ keystroke: K('ks()') }), [placed(104, 77)]),
          ]),
        ]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      commit(2, { value: null }),
      commit(3, textValue('u')),
      commit(4, textValue('l')),
      commit(5, textValue('s')),
    ],
  },
  {
    name: 'keystroke-fault-ladder',
    covers:
      'An exception in either keystroke pass keeps the typed value with a diagnostic; any other error fails the transaction; a false rc in either pass rejects the value, keeping the diagnostics so far.',
    plugins: solo({
      sandbox: {
        builds: [
          {
            run: [
              exception('typing broke'),
              exception('commit broke'),
              budgetFault('typing budget'),
              out({ event: ev('x') }),
              budgetFault('commit budget'),
              rejects('x'),
              out({ event: ev('x') }),
              rejects('x'),
              exception('typing broke again'),
              rejects('x'),
            ],
          },
        ],
      },
    }),
    engine: { documents: { a: doc([list([CODE()])]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      commit(2, textValue('42')),
      commit(2, textValue('43')),
      commit(2, textValue('44')),
      commit(2, textValue('45')),
      commit(2, textValue('46')),
      commit(2, textValue('47')),
    ],
  },
  {
    name: 'keystroke-trees',
    covers:
      'An incomplete keystroke tree fails the transaction; one with no JavaScript is skipped; a keystroke on a non-text field never runs.',
    plugins: solo({ sandbox: { builds: [{}] } }),
    engine: {
      documents: {
        a: doc([
          list([
            text(2, 'broken', '', kvcf({ keystroke: tree(js('x()'), true) })),
            text(3, 'empty', '', kvcf({ keystroke: tree(null) })),
            checkbox(4, 'box', false, kvcf({ keystroke: K('never()') })),
          ]),
        ]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      commit(2, textValue('a')),
      commit(3, textValue('b')),
      commit(4, { value: 'Yes' }),
    ],
  },
];

const GUARD = (over: NonNullable<FormFieldDTO['actions']> = {}) =>
  text(2, 'email', '', kvcf({ validate: tree(js('check()')), ...over }));

const validateCases: ScenarioCase[] = [
  {
    name: 'validate-fault-ladder',
    covers:
      "A validator's false rc rejects the value with its alert; an exception accepts it with a diagnostic; any other error fails; success returns the validator's value and effects.",
    plugins: solo({
      sandbox: {
        builds: [
          {
            run: [
              out({ event: ev('invalid', { rc: false }), uiEffects: [alert('Invalid email')] }),
              exception('validator broke'),
              budgetFault(),
              out({ event: ev('a@b.test'), formEffects: [setValue(1, textValue('validated'))] }),
            ],
          },
        ],
      },
    }),
    engine: { documents: { a: doc([list([NAME, GUARD()])]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      commit(2, textValue('invalid')),
      commit(2, textValue('ok')),
      commit(2, textValue('ok')),
      commit(2, textValue('A@B.TEST')),
    ],
  },
  {
    name: 'validate-trees',
    covers:
      'An incomplete validate tree fails; one with no JavaScript is skipped and the value effect returns.',
    plugins: solo({ sandbox: { builds: [{}] } }),
    engine: {
      documents: {
        a: doc([
          list([
            GUARD({ validate: tree(js('x()'), true) }),
            text(3, 'empty', '', kvcf({ validate: tree(null) })),
          ]),
        ]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [...boot, commit(2, textValue('a')), commit(3, textValue('b'))],
  },
];

const CALC_FIELDS = [
  text(2, 'amount', '3'),
  text(3, 'broken', 'stale', kvcf({ calculate: tree(js('explode()')) })),
  text(4, 'total', '2', kvcf({ calculate: tree(js('total()')), format: tree(js('fmt()')) })),
  text(5, 'nocalc', 'n'),
  text(6, 'emptycalc', 'e', kvcf({ calculate: tree(null), format: tree(js('fmt()')) })),
];

/** `total` with its widget on page 2: the world's pageNumber says which field anchored. */
const TOTAL_ON_PAGE_2 = text(
  4,
  'total',
  '2',
  kvcf({ calculate: tree(js('total()')), format: tree(js('fmt()')) }),
  [placed(104, 2)],
);

const calculateCases: ScenarioCase[] = [
  {
    name: 'calculation-order',
    covers:
      "The /CO chain runs in order after the user's value, skipping null and dead entries and fields without a calculate tree; an exception skips that field with a diagnostic; the target and each calculated field are formatted once, in that order.",
    plugins: solo({
      sandbox: {
        builds: [
          {
            run: [
              exception('explode is not defined'),
              out({ formEffects: [setValue(4, textValue('10'))] }),
              out({ formEffects: [{ kind: 'setAppearanceText', ref: ref(4), text: '$10' }] }),
              out({
                formEffects: [
                  { kind: 'setAppearanceText', ref: ref(6), text: 'E' },
                  { kind: 'setAppearanceText', ref: ref(4), text: '$10.00' },
                ],
              }),
            ],
          },
        ],
      },
    }),
    engine: {
      documents: {
        a: doc([list(CALC_FIELDS, {}, [null, ref(99), ref(3), ref(4), ref(5), ref(6), ref(4)])]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [...boot, commit(2, textValue('5'))],
  },
  {
    name: 'calculation-refusals',
    covers:
      'A calculate error that is not an exception fails the transaction, as does an incomplete calculate tree.',
    plugins: solo({ sandbox: { builds: [{ run: [budgetFault('calc budget')] }] } }),
    engine: {
      documents: {
        a: doc([
          list(
            [text(2, 'amount', '3'), text(3, 'total', '', kvcf({ calculate: tree(js('t()')) }))],
            {},
            [ref(3)],
          ),
        ]),
        b: doc([
          list(
            [
              text(2, 'amount', '3'),
              text(3, 'total', '', kvcf({ calculate: tree(js('t()'), true) })),
            ],
            {},
            [ref(3)],
          ),
        ]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      {
        op: 'call',
        target: 'documents',
        method: 'open',
        args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }],
      },
      commit(2, textValue('4')),
      standalone('commit', [ref(2), textValue('4')], 'b'),
    ],
  },
  {
    name: 'recalculate-anchors',
    covers:
      "recalculate() anchors on the first live /CO field (here not the first field, and on page 2, which the world's pageNumber shows), else the first field, and runs with no source; a first-field anchor outside the /CO chain is neither calculated nor formatted; a document with no fields is an honest no-op.",
    plugins: solo({
      sandbox: {
        builds: [{ run: [out({ formEffects: [setValue(4, textValue('9'))] }), out({})] }],
      },
    }),
    engine: {
      documents: {
        a: doc([
          list([CALC_FIELDS[3], TOTAL_ON_PAGE_2, CALC_FIELDS[4]], {}, [null, ref(99), ref(4)]),
        ]),
        b: doc([list(CALC_FIELDS.slice(2))]),
        c: doc([list([])]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      {
        op: 'call',
        target: 'documents',
        method: 'open',
        args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }],
      },
      {
        op: 'call',
        target: 'documents',
        method: 'open',
        args: [{ kind: 'bytes', id: 'c', bytes: { $bytes: '' } }],
      },
      standalone('recalculate'),
      standalone('recalculate', [], 'b'),
      standalone('recalculate', [], 'c'),
    ],
  },
];

const formatCases: ScenarioCase[] = [
  {
    name: 'format-fault-ladder',
    covers:
      'A format exception keeps the raw value with a diagnostic; any other format error fails; an incomplete format tree fails; one with no JavaScript is skipped.',
    plugins: solo({
      sandbox: { builds: [{ run: [exception('kaboom'), budgetFault('format budget')] }] },
    }),
    engine: {
      documents: {
        a: doc([
          list([
            text(2, 'amount', '', kvcf({ format: tree(js('f()')) })),
            text(3, 'broken', '', kvcf({ format: tree(js('f()'), true) })),
            text(4, 'empty', '', kvcf({ format: tree(null) })),
          ]),
        ]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      commit(2, textValue('7')),
      commit(2, textValue('8')),
      commit(3, textValue('9')),
      commit(4, textValue('10')),
    ],
  },
];

const effectCases: ScenarioCase[] = [
  {
    name: 'effects-canonicalise',
    covers:
      'Script effects fold into the overlay: a reset back to a default becomes one leading reset effect, a later setValue un-resets (even back to the default), display changes become setDisplay, appearance text keeps the last per field in first-set order, and effects on fields the snapshot lacks are dropped.',
    plugins: solo({
      sandbox: {
        builds: [
          {
            run: [
              out({
                formEffects: [
                  { kind: 'reset', refs: [ref(3), ref(4), ref(99)] },
                  setValue(4, textValue('re-set')),
                  { kind: 'setDisplay', ref: ref(5), display: 'hidden' },
                  { kind: 'setDisplay', ref: ref(99), display: 'hidden' },
                  { kind: 'setAppearanceText', ref: ref(5), text: 'first' },
                  { kind: 'setAppearanceText', ref: ref(3), text: 'three' },
                  { kind: 'setAppearanceText', ref: ref(5), text: 'last' },
                  setValue(99, textValue('ghost')),
                  { kind: 'reset', refs: [ref(7)] },
                  setValue(7, textValue('dflt')),
                ],
              }),
            ],
          },
        ],
      },
    }),
    engine: {
      documents: {
        a: doc([
          list([
            text(2, 'trigger', '', kvcf({ validate: tree(js('v()')) })),
            text(3, 'a', 'changed', { defaultValueEntry: { kind: 'scalar', value: 'dflt' } }),
            text(4, 'b', 'changed', { defaultValueEntry: { kind: 'scalar', value: 'dflt' } }),
            text(5, 'c', 'c'),
            text(6, 'd', 'dflt', { defaultValueEntry: { kind: 'scalar', value: 'dflt' } }),
            text(7, 'e', 'changed', { defaultValueEntry: { kind: 'scalar', value: 'dflt' } }),
          ]),
        ]),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [...boot, commit(2, textValue(''))],
  },
  formScenario(
    'native-effect-statuses',
    'Native form writes use atomic ChangeResult instead of per-effect batch statuses. A no-op confirms unchanged; a successful write folds before return; each engine refusal rolls back to the confirmed value.',
    snapshot([NAME], [row(101, ref(1))]),
    [
      call('setValue', [ref(1), textValue('Ada')]),
      call('setValue', [ref(1), textValue('Grace')]),
      call('getValue', [ref(1)]),
      call('setValue', [ref(1), textValue('Hopper')]),
      call('getValue', [ref(1)]),
      call('setValue', [ref(1), textValue('Again')]),
      call('getValue', [ref(1)]),
    ],
    [
      apply({
        ...valuesAnswer(1, [NAME], [row(101, ref(1))]),
        value: {
          ...valuesAnswer(1, [NAME], [row(101, ref(1))]).value,
          meta: { ...valuesAnswer(1, [NAME], [row(101, ref(1))]).value.meta, undoable: false },
        },
      }),
      apply(valuesAnswer(2, [text(1, 'name', 'Grace')], [row(101, ref(1))])),
      refused('batch threw', 'Io'),
      refused('atomic unsupported', 'Unsupported'),
    ],
  ),
  {
    name: 'no-change-is-unchanged',
    covers:
      'A controller commit of the current value is accepted with no effects; no engine write belongs to the controller. A field the snapshot no longer has fails.',
    plugins: solo({ sandbox: { builds: [{}] } }),
    engine: { documents: { a: doc([list([NAME])]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      commit(1, textValue('Ada')),
      commit(99, textValue('x')),
      standalone('commit', [{ kind: 'fqn', name: 'name' }, textValue('x')]),
    ],
  },
  {
    name: 'a-throwing-run-fails',
    covers:
      'A sandbox run that throws fails the transaction with an invalid-output error carrying its message; a script setValue on a pushbutton rejects the transaction.',
    plugins: solo({
      sandbox: {
        builds: [
          {
            run: [
              { throws: { message: 'realm crashed' } },
              out({ formEffects: [setValue(8, textValue('no'))] }),
            ],
          },
        ],
      },
    }),
    engine: { documents: { a: doc([list([GUARD(), BUTTON])]) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, commit(2, textValue('a')), commit(2, textValue('b'))],
  },
];

// ── budgets ───────────────────────────────────────────────────────────────────

const BUDGET = {
  maxExecutionMs: 50,
  maxMemoryBytes: 1024,
  maxStackBytes: 512,
  maxEffects: 2,
  maxOutputBytes: 100000,
};
const emptyOutput = (over: Record<string, unknown>) => ({
  event: { rc: true, value: null, change: '', selStart: 0, selEnd: 0 },
  formEffects: [],
  annotEffects: [],
  uiEffects: [],
  diagnostics: [],
  ...over,
});
const WIDE = { uiEffects: [alert('é € 😀 ascii', 2)] };
const WIDE_BYTES = Buffer.byteLength(JSON.stringify(emptyOutput(WIDE)));

const budgetCases: ScenarioCase[] = [
  {
    name: 'the-effects-budget',
    covers:
      'More form effects plus UI requests than maxEffects fails the transaction before the engine; exactly maxEffects passes.',
    plugins: solo({
      sandbox: {
        builds: [
          {
            run: [
              out({
                formEffects: [setValue(1, textValue('x'))],
                uiEffects: [alert('one'), alert('two')],
              }),
              out({ event: ev('b'), uiEffects: [alert('one')] }),
            ],
          },
        ],
      },
      budget: BUDGET,
    }),
    engine: { documents: { a: doc([list([NAME, GUARD()])]) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, commit(2, textValue('a')), commit(2, textValue('b'))],
  },
  {
    name: 'the-output-budget-at-the-boundary',
    covers: `Output bytes are the UTF-8 length of each output's JSON (${WIDE_BYTES} here, with 2-, 3- and 4-byte characters): exactly the budget passes.`,
    plugins: solo({
      sandbox: { builds: [{ boot: [out(WIDE)] }] },
      budget: { ...BUDGET, maxOutputBytes: WIDE_BYTES },
    }),
    engine: { documents: { a: doc([list([NAME]), ACTIONS_READ()]) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, commit(1, textValue('a'))],
  },
  {
    name: 'the-output-budget-exceeded',
    covers:
      'One byte over, a boot output degrades with the budget message, and a run output that crosses the aggregate fails the transaction.',
    plugins: solo({
      sandbox: { builds: [{ boot: [out(WIDE)], run: [out(WIDE)] }] },
      budget: { ...BUDGET, maxOutputBytes: WIDE_BYTES - 1 },
    }),
    engine: { documents: { a: doc([list([NAME, GUARD()]), ACTIONS_READ()]) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, commit(1, textValue('a')), commit(2, textValue('b'))],
  },
  {
    name: 'the-time-budget',
    covers:
      "Each run receives what is left of the transaction's execution budget; once a run has taken all of it (a realm rebuilt behind a slow factory), the next run times out without reaching the sandbox, and the rebuilt realm's boot surfaces in the next transaction.",
    plugins: solo({
      sandbox: {
        builds: [
          { boot: [{ dispose: true }] },
          { defer: 'rebuild', boot: [out({ uiEffects: [alert('late boot')] })] },
        ],
      },
      budget: BUDGET,
    }),
    engine: { documents: { a: doc([list([CODE()]), ACTIONS_READ()]) } },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      commit(2, textValue('slow')),
      advance(60),
      settle('rebuild'),
      commit(2, textValue('next')),
    ],
  },
  {
    name: 'the-time-budget-shrinks',
    covers: 'A run that took 20 of the 50 budgeted milliseconds leaves the next run 30.',
    plugins: solo({
      sandbox: { builds: [{ boot: [{ dispose: true }] }, { defer: 'rebuild' }] },
      budget: BUDGET,
    }),
    engine: { documents: { a: doc([list([CODE()]), ACTIONS_READ()]) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, commit(2, textValue('slow')), advance(20), settle('rebuild')],
  },
];

// ── the capability over the actions plugin's realm ───────────────────────────

// The capability cases below use the actual form plugin, which owns native writes.
const USER = { origin: 'user', source: { kind: 'api' }, event: { scope: 'activate' } } as const;
const sharedFields = [
  text(2, 'amount', '3', kvcf({ keystroke: K(), validate: tree(js('validate()')) })),
  text(3, 'total', '6', kvcf({ calculate: tree(js('calc()')), format: tree(js('fmt()')) })),
];
const sharedValues = valuesAnswer(1, sharedFields, [row(102, ref(2)), row(103, ref(3))]);
const appearanceMeta = mutationMeta(1, [sharedFields[1]!], [row(103, ref(3))]);
const sharedAnswer = {
  value: {
    ...sharedValues.value,
    items: [
      ...sharedValues.value.items,
      {
        type: 'forms.setAppearanceText',
        field: sharedFields[1]!,
        widgets: [row(103, ref(3))],
        meta: appearanceMeta,
      },
    ],
  } satisfies ChangeResult,
  events: sharedValues.events
    .map((event, index) => ({
      ...event,
      origin: { ...LOCAL, tx: { id: operationAt(1), index, count: 3 } },
    }))
    .concat([
      {
        type: 'forms.updated',
        field: sharedFields[1]!,
        widgets: [row(103, ref(3))],
        meta: appearanceMeta,
        origin: { ...LOCAL, tx: { id: operationAt(1), index: 2, count: 3 } },
      },
    ] satisfies DocumentEvent[]),
};
const resetFields = [
  text(2, 'amount', ''),
  text(4, 'total', '0', kvcf({ calculate: tree(js('t()')) })),
];
const resetBase = resetAnswer(1, [resetFields[0]!], [row(102, ref(2))]);
const resetCalculated = valuesAnswer(1, [resetFields[1]!], [row(104, ref(4))]);
const resetCalculationAnswer = {
  value: {
    items: [...resetBase.value.items, ...resetCalculated.value.items],
    meta: mutationMeta(1, resetFields, [row(102, ref(2)), row(104, ref(4))]),
  },
  events: [...resetBase.events, ...resetCalculated.events].map((event, index) => ({
    ...event,
    origin: { ...LOCAL, tx: { id: operationAt(1), index, count: 2 } },
  })),
};
const observe: Step[] = [
  act('onDiagnosticReported', [fnSpec('actionDiagnostic')]),
  act('setUiAdapter', [{ print: fnSpec('ui.print'), alert: fnSpec('ui.alert') }]),
  act('onScriptDiagnosticReported', [fnSpec('onScriptDiagnostic')]),
  act('onScriptFailed', [fnSpec('onScriptError')]),
];
const widgetRows = [row(102, ref(2)), row(103, ref(3))];
const capabilityCases: ScenarioCase[] = [
  formScenario(
    'commit-through-the-shared-realm',
    'One held fill runs K/V/C/F in the actions realm and sends complete current ops. Confirmed fields and widget rows fold before setValue resolves.',
    snapshot([AMOUNT, TOTAL], widgetRows, [ref(3)]),
    [
      ...observe,
      call('onValueChanged', [fnSpec('valueChanged')]),
      call('setValue', [ref(2), textValue('3')]),
      call('getValue', [ref(2)]),
      call('getValue', [ref(3)]),
      call('getSnapshot'),
    ],
    [ACTIONS_READ(), apply(sharedAnswer)],
    {
      builds: [
        {
          boot: [out({ uiEffects: [alert('boot nag', 1)] })],
          run: [
            out({ event: ev('1', { change: '3', selStart: 0, selEnd: 1 }) }),
            out({ event: ev('3') }),
            out({
              event: ev('3'),
              uiEffects: [alert('validated')],
              diagnostics: [{ code: 'unsupported-api', message: 'util.printf' }],
            }),
            out({ formEffects: [setValue(3, textValue('6'))] }),
            out({ formEffects: [{ kind: 'setAppearanceText', ref: ref(3), text: '$6' }] }),
          ],
        },
      ],
    },
  ),
  formScenario(
    'scripted-writes-clear-the-spinner',
    'Unchanged, rejected, failed and accepted scripts release held writes; confirmed public values survive refusal.',
    snapshot([NAME, GUARD()], [row(101, ref(1)), row(102, ref(2))]),
    [
      ...observe,
      call('setValue', [ref(1), textValue('Ada')]),
      call('setValue', [ref(2), textValue('bad')]),
      call('getValue', [ref(2)]),
      call('setValue', [ref(2), textValue('worse')]),
      call('getValue', [ref(2)]),
      call('setValue', [ref(2), textValue('fine')]),
      call('getValue', [ref(2)]),
    ],
    [
      apply(
        valuesAnswer(
          4,
          [text(2, 'email', 'ok', kvcf({ validate: tree(js('check()')) }))],
          [row(102, ref(2))],
        ),
      ),
    ],
    { builds: [{ run: [rejects('x'), budgetFault('boom'), out({ event: ev('ok') })] }] },
  ),
  formScenario(
    'batch-writes-count-rejections',
    'Current batches validate plain values individually and recalculate once. A single-field validation refusal emits its exact event; batches report malformed family values separately.',
    snapshot([NAME, GUARD()], [row(101, ref(1)), row(102, ref(2))]),
    [
      ...observe,
      call('onValidationRejected', [fnSpec('validationRejected')]),
      call('setValue', [ref(2), textValue('bad')]),
      call('setValues', [
        [
          { ref: ref(1), value: textValue('Grace') },
          { ref: ref(2), value: { selectedValues: ['bad'] } },
        ],
      ]),
      call('importValues', [{ name: 'Hopper', missing: 'x', email: ['invalid'] }]),
      call('getValue', [ref(1)]),
    ],
    [
      apply(valuesAnswer(2, [text(1, 'name', 'Grace')], [row(101, ref(1))])),
      apply(valuesAnswer(3, [text(1, 'name', 'Hopper')], [row(101, ref(1))])),
    ],
    {
      builds: [
        {
          run: [
            out({
              event: ev('x', { rc: false }),
              diagnostics: [{ code: 'script-error', message: 'bad email' }],
            }),
          ],
        },
      ],
    },
  ),
  formScenario(
    'reset-recalculates',
    'A reset and calculation share one atomic change, with the action origin carried to surfaced UI. Script failure cancels every requested reset.',
    snapshot(
      [text(2, 'amount', '3'), text(4, 'total', '6', kvcf({ calculate: tree(js('t()')) }))],
      [row(102, ref(2)), row(104, ref(4))],
      [ref(4)],
    ),
    [
      ...observe,
      call('resetFormAction', [[byName('amount')], false, 'lifecycle']),
      call('getValue', [ref(2)]),
      call('getValue', [ref(4)]),
      call('resetFormAction', [null, false]),
      call('getValue', [ref(4)]),
      act('execute', [
        tree(js('print()')),
        {
          origin: 'lifecycle',
          source: { kind: 'api' },
          event: { scope: 'document', name: 'open' },
        },
      ]),
    ],
    [apply(resetCalculationAnswer)],
    {
      builds: [
        {
          run: [
            out({
              formEffects: [setValue(4, textValue('0'))],
              uiEffects: [alert('recalculated'), { kind: 'print' }],
            }),
            budgetFault('recalc budget'),
            out({ uiEffects: [{ kind: 'print' }] }),
          ],
        },
      ],
    },
  ),
  formScenario(
    'scripted-activation',
    'Widget actions resolve from the form snapshot and dispatch JavaScript through the registered form commit sink. Missing widgets reject; tree-less widgets fall back without a page reread.',
    snapshot(
      [NAME, BUTTON],
      [
        row(101, ref(1)),
        row(108, ref(8), 1, undefined, {
          fieldFamily: 'pushbutton',
          actions: { activate: tree(js('click()')) },
        }),
        row(109, ref(8), 1, undefined, { fieldFamily: 'pushbutton' }),
      ],
    ),
    [
      ...observe,
      call('activateWidget', [objRef(108)]),
      call('getValue', [ref(1)]),
      call('activateWidget', [objRef(109)]),
      call('activateWidget', [objRef(404)]),
    ],
    [apply(valuesAnswer(1, [text(1, 'name', 'clicked')], [row(101, ref(1))]))],
    { builds: [{ run: [out({ formEffects: [setValue(1, textValue('clicked'))] })] }] },
  ),
  formScenario(
    'scripted-activation-matches-refs',
    'Current baseIndex refs and unknown widget numbers are refused before dispatch. An object number with another page has no matching action row on that page and falls back to the form owner.',
    snapshot([NAME, BUTTON], [row(108, ref(8), 1, undefined, { fieldFamily: 'pushbutton' })]),
    [
      call('activateWidget', [{ kind: 'baseIndex', page: pr(1), baseIndex: 3 }]),
      call('activateWidget', [{ kind: 'baseIndex', page: pr(1), baseIndex: 4 }]),
      call('activateWidget', [objRef(404)]),
      call('activateWidget', [objRef(108, 2)]),
    ],
    [],
    { builds: [{}] },
  ),
  formScenario(
    'a-kindless-address-matches-no-candidate',
    'The surviving no-candidate refusal uses a current baseIndex widget address; malformed removed nm/index addresses remain archived.',
    snapshot([BUTTON], [row(108, ref(8), 1, undefined, { fieldFamily: 'pushbutton' })]),
    [call('activateWidget', [{ kind: 'baseIndex', page: pr(1), baseIndex: 0 }])],
    [],
    { builds: [{}] },
  ),
  formScenario(
    'scripted-activation-reads-the-folded-annotation',
    'Widget actions and hover flags are owned by the current form rows even while widgets are lent to annotation. Rootless actions and missing event trees remain inert.',
    snapshot(
      [NAME, BUTTON],
      [
        row(101, ref(1)),
        row(108, ref(8), 1, undefined, {
          fieldFamily: 'pushbutton',
          actions: { activate: tree(js('click()')) },
        }),
        row(109, ref(8), 1, undefined, {
          fieldFamily: 'pushbutton',
          actions: { cursorEnter: tree(null) },
        }),
        row(110, ref(8), 1, undefined, { fieldFamily: 'pushbutton' }),
      ],
    ),
    [
      call('activateWidget', [objRef(108)]),
      call('activateWidget', [objRef(109)]),
      call('activateWidget', [objRef(110)]),
      call('notifyWidgetEvent', [ref(8), objRef(109), 'cursorEnter']),
      call('notifyWidgetEvent', [ref(8), objRef(110), 'cursorEnter']),
      call('getValue', [ref(1)]),
    ],
    [apply(valuesAnswer(1, [text(1, 'name', 'clicked')], [row(101, ref(1))]))],
    { builds: [{ run: [out({ formEffects: [setValue(1, textValue('clicked'))] })] }] },
    true,
  ),
  formScenario(
    'script-nodes-commit-through-the-form',
    'A JavaScript action sends effects to the real form sink, confirms one change and exposes its public value without forced rereads.',
    snapshot([NAME], [row(101, ref(1))]),
    [
      ...observe,
      act('execute', [tree(js('this.getField("name").value = "from-js";')), USER]),
      call('getValue', [ref(1)]),
    ],
    [apply(valuesAnswer(1, [text(1, 'name', 'from-js')], [row(101, ref(1))]))],
    { builds: [{ run: [out({ formEffects: [setValue(1, textValue('from-js'))] })] }] },
  ),
  formScenario(
    'no-sandbox-host',
    'Enabled configuration with no injected factory creates the current lazy default realm. Explicit validation:none keeps this native fill outside scripts; this case observes the enabled realm without constructing its default VM.',
    snapshot([AMOUNT, TOTAL], widgetRows, [ref(3)]),
    [
      act('isScriptingEnabled', []),
      call('setValue', [ref(2), textValue('3')]),
      call('getValue', [ref(2)]),
      call('getValue', [ref(3)]),
      call('activateWidget', [objRef(102)]),
    ],
    [
      apply(
        valuesAnswer(
          1,
          [text(2, 'amount', '3', kvcf({ keystroke: K(), validate: tree(js('validate()')) }))],
          [row(102, ref(2))],
        ),
      ),
    ],
    null,
    false,
    {},
    { validation: 'none' },
  ),
];

/** Bind the complete current snapshot as a serialized controller argument before execution. */
function controllerInputs(input: ScenarioCase): ScenarioCase {
  return {
    ...input,
    steps: input.steps.map((step) => {
      if (
        step.op !== 'call' ||
        typeof step.target !== 'object' ||
        !('token' in step.target) ||
        step.target.token !== 'form-scripts-current' ||
        step.method === 'dispose'
      )
        return step;
      const form = input.engine?.documents?.[step.target.doc ?? 'a']?.responses?.find(
        (response) => response.path === 'forms.list',
      )?.value;
      return { ...step, args: [FormSnapshotSchema.parse(form), ...(step.args ?? [])] };
    }),
  };
}
export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/form-scripting.json',
    describes: 'Current form K/V/C/F controller transactions and native form capabilities.',
    cases: [
      ...environmentCases,
      ...valueCases,
      ...bootCases,
      ...activationCases,
      ...keystrokeCases,
      ...validateCases,
      ...calculateCases,
      ...formatCases,
      ...effectCases,
      ...budgetCases,
    ]
      .map(controllerInputs)
      .concat(capabilityCases),
  },
];
