/**
 * `packages/plugin/form/src/scripting.ts`: the K/V/C/F transaction. Two
 * halves:
 *
 *   - the standalone controller (`createFormScriptingController` with
 *     `config`, the arm stamp uses) through the `form-scripting` driver:
 *     environment, boot order, keystroke, validate, calculate and format
 *     passes, effect canonicalisation, budgets and the fault ladder;
 *   - the capability over the actions plugin's shared realm: commits,
 *     writes, reset recalculation, scripted activation, the surfacing port,
 *     the commit sink, and the host with no sandbox.
 *
 * The sandbox is the scripted one from `acrojs.ts`: every factory call,
 * `boot`, `run` and `dispose` lands in the trace with its canonical input, and
 * answers come from the case. Replayed by
 * `crates/plugins/form/tests/conformance.rs`.
 */
import type { ResponseSpec } from '../fake-engine';
import type { PluginSpec, ScenarioCase, Step } from '../scenario';
import type { Suite } from '../vectors';

import {
  act,
  advance,
  annotations,
  aref,
  boot,
  byName,
  call,
  checkbox,
  combobox,
  doc,
  effectsOk,
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
  widgetAnnot,
  standalonePlugins,
  type StandaloneConfig,
  pr,
  v3Widget,
} from './form-fixtures';

// ── sandbox answers ───────────────────────────────────────────────────────────

const ev = (value: unknown = null, over: Record<string, unknown> = {}) => ({ rc: true, value, change: '', selStart: 0, selEnd: 0, ...over });
const out = (over: Record<string, unknown> = {}) => ({ output: over });
const rejects = (value: unknown = null) => out({ event: ev(value, { rc: false }) });
const exception = (message = 'boom') => out({ error: { kind: 'exception', message } });
const budgetFault = (message = 'synthetic budget fault') => out({ error: { kind: 'budget', message } });
const setValue = (n: number, value: unknown) => ({ kind: 'setValue', ref: ref(n), value });
const textValue = (value: string) => ({ type: 'text', value });
const alert = (message: string, icon = 0) => ({ kind: 'alert', message, icon });

const ACTIONS_READ = (scripts: string[] = ['var lib = 1;']): ResponseSpec => ({
  path: 'actions.read',
  value: { nameTreeScripts: scripts.map((script, i) => ({ name: `lib${i}`, action: tree(js(script)) })), openAction: null, openDestination: null },
});
const withScripts = { services: ['actions'] };

const kvcf = (over: Record<string, unknown>) => ({ actions: over });
const K = (script = 'AFNumber_Keystroke(2)') => tree(js(script));

/** A standalone driver case: the driver over document `a`. */
const solo = (config: Partial<StandaloneConfig> & { sandbox: StandaloneConfig['sandbox'] }) => standalonePlugins(config);
const commit = (n: number, value: unknown) => standalone('commit', [ref(n), value]);
const applyOk = (count = 8): ResponseSpec => ({ path: 'forms.applyEffects', value: effectsOk(count) });

const NAME = text(1, 'name', 'Ada');
const AMOUNT = text(2, 'amount', '1', kvcf({ keystroke: K(), validate: tree(js('validate()')) }));
const TOTAL = text(3, 'total', '2', kvcf({ calculate: tree(js('calc()')), format: tree(js('fmt()')) }));

// ── the standalone controller ─────────────────────────────────────────────────

const environmentCases: ScenarioCase[] = [
  {
    name: 'standalone-configured-environment',
    covers:
      "The standalone host reads the name-tree scripts once, boots once per realm with the configured file name, identity, clock, offset and seed, and commits the value; a second commit does not boot again.",
    plugins: solo({
      sandbox: { builds: [{}] },
      now: fnSpec('now', { returns: 1234 }),
      utcOffsetMinutes: fnSpec('offset', { returns: 180 }),
      randomSeed: fnSpec('seed', { returns: 7 }),
      identity: { name: 'Ada', email: 'ada@example.test' },
      fileName: fnSpec('fileName', { returns: 'proposal.pdf' }),
    }),
    engine: { documents: { a: doc([list([NAME]), ACTIONS_READ(['var a = 1;', 'var b = 2;']), applyOk()], withScripts) } },
    snapshot: 'none',
    steps: [...boot, commit(1, textValue('Grace')), commit(1, textValue('Hopper'))],
  },
  {
    name: 'standalone-default-environment',
    covers:
      "Without configuration the clock is the host's, the offset the host zone's, the seed seedFrom(document id, run), the file name the document's, the identity the engine's claims; a document without catalog actions boots nothing but still builds the realm.",
    plugins: solo({ sandbox: { builds: [{}] } }),
    engine: {
      documents: {
        a: doc([list([text(2, 'code', 'old', kvcf({ keystroke: K('up()') }))]), applyOk()], {
          security: { allows: '*', identity: { user_id: 'u-1', display_name: 'Una', group_id: 'g-1' } },
        }),
      },
    },
    snapshot: 'none',
    steps: [...boot, advance(1500), commit(2, textValue('new'))],
  },
  {
    name: 'standalone-identity-callback',
    covers: 'An identity callback is called per run and layered over the engine claims.',
    plugins: solo({ sandbox: { builds: [{}] }, identity: fnSpec('identity', { returns: { corporation: 'Fn Inc' } }) }),
    engine: {
      documents: {
        a: doc([list([NAME]), ACTIONS_READ()], { services: ['actions'], security: { allows: '*', identity: { user_id: 'u-2' } } }),
      },
    },
    snapshot: 'none',
    steps: [...boot, standalone('recalculate')],
  },
  {
    name: 'standalone-dispose',
    covers: 'dispose() releases the owned realm; a later transaction rejects.',
    plugins: solo({ sandbox: { builds: [{}] } }),
    engine: { documents: { a: doc([list([NAME]), ACTIONS_READ(), applyOk()], withScripts) } },
    snapshot: 'none',
    steps: [...boot, commit(1, textValue('x')), standalone('dispose'), commit(1, textValue('y')), standalone('dispose')],
  },
];

const CHECK = checkbox(4, 'agree', true, kvcf({ validate: tree(js('v()')) }));
const RADIO = radio(5, 'size', [
  v3Widget({ annotObjectNumber: 105, pageObjectNumber: 1, onState: 'S', checked: false }),
  v3Widget({ annotObjectNumber: 106, pageObjectNumber: 1, onState: 'M', checked: false }),
], kvcf({ validate: tree(js('v()')) }));
const COMBO = combobox(6, 'colour', 'r', kvcf({ validate: tree(js('v()')) }));
const LIST = listbox(7, 'colours', ['r'], kvcf({ validate: tree(js('v()')) }));
const BUTTON = pushbutton(8, 'go');
const VALUE_FIELDS = [NAME, CHECK, RADIO, COMBO, LIST, BUTTON, text(9, 'plain', 'p', kvcf({ validate: tree(js('v()')) }))];
const PLAIN_VALUE_FIELDS = [
  NAME,
  checkbox(4, 'agree', true),
  radio(5, 'size', [v3Widget({ annotObjectNumber: 105, pageObjectNumber: 1, onState: 'S', checked: false })]),
  combobox(6, 'colour', 'r'),
  listbox(7, 'colours', ['r']),
  BUTTON,
];

const valueCases: ScenarioCase[] = [
  {
    name: 'form-values-cross-into-the-overlay',
    covers:
      "A toggle proposes its state (null as 'Off'), a combobox its first choice ('' for none), a listbox every choice; a value whose type does not match the family fails the transaction before any run.",
    plugins: solo({ sandbox: { builds: [{}] } }),
    engine: { documents: { a: doc([list(PLAIN_VALUE_FIELDS), applyOk()]) } },
    snapshot: 'none',
    steps: [
      ...boot,
      commit(4, { type: 'toggle', state: 'Yes' }),
      commit(4, { type: 'toggle', state: null }),
      commit(5, { type: 'toggle', state: 'M' }),
      commit(6, { type: 'choice', values: ['g', 'r'] }),
      commit(6, { type: 'choice', values: [] }),
      commit(7, { type: 'choice', values: ['r', 'g'] }),
      commit(1, { type: 'toggle', state: 'Yes' }),
      commit(8, textValue('press')),
      commit(1, { type: 'choice', values: ['x'] }),
    ],
  },
  {
    name: 'a-cleared-toggle-proposes-off',
    covers: "A toggle to null reaches the validator as 'Off'.",
    plugins: solo({ sandbox: { builds: [{ run: [out({ event: ev('Off') })] }] } }),
    engine: { documents: { a: doc([list([CHECK]), applyOk()]) } },
    snapshot: 'none',
    steps: [...boot, commit(4, { type: 'toggle', state: null })],
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
    engine: { documents: { a: doc([list(VALUE_FIELDS), applyOk()]) } },
    snapshot: 'none',
    steps: [
      ...boot,
      commit(9, textValue('typed')),
      commit(4, { type: 'toggle', state: 'Yes' }),
      commit(4, { type: 'toggle', state: 'Yes' }),
      commit(4, { type: 'toggle', state: 'Yes' }),
      commit(6, { type: 'choice', values: ['g'] }),
      commit(6, { type: 'choice', values: ['g'] }),
      commit(7, { type: 'choice', values: ['g'] }),
      commit(7, { type: 'choice', values: ['g'] }),
      commit(5, { type: 'toggle', state: 'S' }),
    ],
  },
];

const bootCases: ScenarioCase[] = [
  {
    name: 'boot-effects-land-with-the-commit',
    covers:
      "A boot's form effects and UI requests join the first transaction, tagged 'boot', ahead of the user's value; later transactions see no boot.",
    plugins: solo({
      sandbox: { builds: [{ boot: [out({ formEffects: [setValue(1, textValue('initialized'))], uiEffects: [alert('Needs a newer viewer', 1)] })] }] },
    }),
    engine: { documents: { a: doc([list([NAME, text(2, 'other', 'o')]), ACTIONS_READ(), applyOk()], withScripts) } },
    snapshot: 'none',
    steps: [...boot, commit(2, textValue('mine')), commit(2, textValue('again'))],
  },
  {
    name: 'a-failed-boot-degrades',
    covers:
      "A boot that throws, or answers with an error, is a script-error diagnostic and the transaction goes on without its effects; its UI requests still surface.",
    plugins: solo({
      sandbox: {
        builds: [
          { boot: [{ throws: { message: 'app.findComponent is not a function' } }] },
          {},
        ],
      },
    }),
    engine: { documents: { a: doc([list([NAME]), ACTIONS_READ(), applyOk()], withScripts) } },
    snapshot: 'none',
    steps: [...boot, commit(1, textValue('a'))],
  },
  {
    name: 'a-boot-error-output-degrades',
    covers: 'A boot output carrying an error keeps its UI requests and diagnostics, drops its form effects, and adds the degrade diagnostic.',
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
    engine: { documents: { a: doc([list([NAME, text(2, 'other', 'o')]), ACTIONS_READ(), applyOk()], withScripts) } },
    snapshot: 'none',
    steps: [...boot, commit(2, textValue('b'))],
  },
  {
    name: 'a-failed-catalog-read-degrades',
    covers: 'A name-tree read that rejects degrades to the host diagnostic; the value still commits.',
    plugins: solo({ sandbox: { builds: [{}] } }),
    engine: {
      documents: {
        a: doc([list([NAME]), { path: 'actions.read', error: { name: 'EngineError', message: 'actions unavailable', code: 'Io' } }, applyOk()], withScripts),
      },
    },
    snapshot: 'none',
    steps: [...boot, commit(1, textValue('a'))],
  },
  {
    name: 'rejection-keeps-boot-effects',
    covers:
      'A rejected value still commits the boot effects alone and resolves rejected with their result; a boot batch that fails turns the rejection into a failure; with no batch door it fails before the engine.',
    plugins: solo({
      sandbox: {
        builds: [
          {
            boot: [out({ formEffects: [setValue(1, textValue('booted'))], diagnostics: [{ code: 'unsupported-api', message: 'app.viewerVersion' }] })],
            run: [rejects('x')],
          },
        ],
      },
    }),
    engine: {
      documents: {
        a: doc([list([NAME, text(2, 'guarded', '', kvcf({ validate: tree(js('no()')) }))]), ACTIONS_READ(), applyOk()], withScripts),
        b: doc(
          [
            list([NAME, text(2, 'guarded', '', kvcf({ validate: tree(js('no()')) }))]),
            ACTIONS_READ(),
            { path: 'forms.applyEffects', value: { results: [{ index: 0, status: 'skipped', fields: [], changedWidgets: [] }], changedWidgets: [], meta: null } },
          ],
          withScripts,
        ),
        c: doc([list([NAME, text(2, 'guarded', '', kvcf({ validate: tree(js('no()')) }))]), ACTIONS_READ()], { ...withScripts, lacks: ['forms.applyEffects'] }),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }] },
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'c', bytes: { $bytes: '' } }] },
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
      "activate(ref, tree) runs the tree's JavaScript as widget-activate against the overlay and commits its effects with its UI requests; no calculation follows.",
    plugins: solo({
      sandbox: { builds: [{ run: [out({ formEffects: [setValue(1, textValue('Mouse Up:Field'))], uiEffects: [alert('Summary ready'), { kind: 'gotoPage', page: 1 }] })] }] },
    }),
    engine: { documents: { a: doc([list([NAME, BUTTON, TOTAL], {}, [ref(3)]), applyOk()]) } },
    snapshot: 'none',
    steps: [...boot, standalone('activate', [ref(8), tree(js('go()', [js('then()')]))])],
  },
  {
    name: 'activation-edits-its-own-field',
    covers: "An activation that sets its own field's value commits it; the activated field is not formatted, since no value was proposed.",
    plugins: solo({ sandbox: { builds: [{ run: [out({ formEffects: [setValue(9, textValue('mine'))] })] }] } }),
    engine: { documents: { a: doc([list([text(9, 'self', 'old', kvcf({ format: tree(js('fmt()')) }))]), applyOk()]) } },
    snapshot: 'none',
    steps: [...boot, standalone('activate', [ref(9), tree(js('me()'))])],
  },
  {
    name: 'recalculate-keeps-a-boot-reset',
    covers: "recalculate() proposes nothing, so a boot script's reset of the anchor field survives as a reset effect.",
    plugins: solo({ sandbox: { builds: [{ boot: [out({ formEffects: [{ kind: 'reset', refs: [ref(4)] }] })] }] } }),
    engine: {
      documents: {
        a: doc([list([text(4, 'total', 'changed', { defaultValueEntry: { kind: 'scalar', value: 'dflt' } })], {}, [ref(4)]), ACTIONS_READ(), applyOk()], withScripts),
      },
    },
    snapshot: 'none',
    steps: [...boot, standalone('recalculate')],
  },
  {
    name: 'activation-refusals',
    covers:
      'An incomplete tree fails before any run; a tree with no JavaScript commits nothing; a run error fails the transaction.',
    plugins: solo({ sandbox: { builds: [{ run: [exception('activation broke')] }] } }),
    engine: { documents: { a: doc([list([NAME, BUTTON]), applyOk()]) } },
    snapshot: 'none',
    steps: [
      ...boot,
      standalone('activate', [ref(8), tree(js('x()'), true)]),
      standalone('activate', [ref(8), tree(null)]),
      standalone('activate', [ref(8), tree(resetNode(null))]),
      standalone('activate', [ref(8), tree(js('x()'))]),
    ],
  },
];

const CODE = (over: Record<string, unknown> = {}, value = 'old') => text(2, 'code', value, kvcf({ keystroke: K('ks()'), ...over }));

const keystrokeCases: ScenarioCase[] = [
  {
    name: 'keystroke-two-passes',
    covers:
      "A text commit runs the keystroke script twice: once paste-shaped (the whole value in change, the old value selected, willCommit false), whose selection splice becomes the proposal, then willCommit with the spliced value, whose event.value is committed.",
    plugins: solo({
      sandbox: {
        builds: [
          {
            run: [
              out({ event: ev('old', { change: 'ABC', selStart: 0, selEnd: 3 }), formEffects: [setValue(1, textValue('typed-into'))] }),
              out({ event: ev('ABC!') }),
            ],
          },
        ],
      },
    }),
    engine: { documents: { a: doc([list([NAME, CODE()]), applyOk()]) } },
    snapshot: 'none',
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
    engine: { documents: { a: doc([list([CODE({}, '😀x')]), applyOk()]) } },
    snapshot: 'none',
    steps: [...boot, commit(2, textValue('z')), commit(2, textValue('z')), commit(2, textValue('z')), commit(2, textValue('z')), commit(2, textValue('z'))],
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
            text(2, 'code', '', kvcf({ keystroke: K('ks()') }), [[102, 2]]),
            text(3, 'unplaced', '', kvcf({ keystroke: K('ks()') }), [[0, 0]]),
            text(5, 'second', '', kvcf({ keystroke: K('ks()') }), [[0, 0], [105, 2]]),
            text(4, 'lost', '', kvcf({ keystroke: K('ks()') }), [[104, 77]]),
          ]),
          applyOk(),
        ]),
      },
    },
    snapshot: 'none',
    steps: [...boot, commit(2, { type: 'text', value: null }), commit(3, textValue('u')), commit(4, textValue('l')), commit(5, textValue('s'))],
  },
  {
    name: 'keystroke-fault-ladder',
    covers:
      "An exception in either keystroke pass keeps the typed value with a diagnostic; any other error fails the transaction; a false rc in either pass rejects the value, keeping the diagnostics so far.",
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
    engine: { documents: { a: doc([list([CODE()]), applyOk()]) } },
    snapshot: 'none',
    steps: [...boot, commit(2, textValue('42')), commit(2, textValue('43')), commit(2, textValue('44')), commit(2, textValue('45')), commit(2, textValue('46')), commit(2, textValue('47'))],
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
          applyOk(),
        ]),
      },
    },
    snapshot: 'none',
    steps: [...boot, commit(2, textValue('a')), commit(3, textValue('b')), commit(4, { type: 'toggle', state: 'Yes' })],
  },
];

const GUARD = (over: Record<string, unknown> = {}) => text(2, 'email', '', kvcf({ validate: tree(js('check()')), ...over }));

const validateCases: ScenarioCase[] = [
  {
    name: 'validate-fault-ladder',
    covers:
      "A validator's false rc rejects the value with its alert; an exception accepts it with a diagnostic; any other error fails; success commits the validator's value and effects.",
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
    engine: { documents: { a: doc([list([NAME, GUARD()]), applyOk()]) } },
    snapshot: 'none',
    steps: [...boot, commit(2, textValue('invalid')), commit(2, textValue('ok')), commit(2, textValue('ok')), commit(2, textValue('A@B.TEST'))],
  },
  {
    name: 'validate-trees',
    covers: 'An incomplete validate tree fails; one with no JavaScript is skipped and the value commits.',
    plugins: solo({ sandbox: { builds: [{}] } }),
    engine: {
      documents: {
        a: doc([list([GUARD({ validate: tree(js('x()'), true) }), text(3, 'empty', '', kvcf({ validate: tree(null) }))]), applyOk()]),
      },
    },
    snapshot: 'none',
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
              out({ formEffects: [{ kind: 'setAppearanceText', ref: ref(6), text: 'E' }, { kind: 'setAppearanceText', ref: ref(4), text: '$10.00' }] }),
            ],
          },
        ],
      },
    }),
    engine: { documents: { a: doc([list(CALC_FIELDS, {}, [null, ref(99), ref(3), ref(4), ref(5), ref(6), ref(4)]), applyOk()]) } },
    snapshot: 'none',
    steps: [...boot, commit(2, textValue('5'))],
  },
  {
    name: 'calculation-refusals',
    covers: 'A calculate error that is not an exception fails the transaction, as does an incomplete calculate tree.',
    plugins: solo({ sandbox: { builds: [{ run: [budgetFault('calc budget')] }] } }),
    engine: {
      documents: {
        a: doc([list([text(2, 'amount', '3'), text(3, 'total', '', kvcf({ calculate: tree(js('t()')) }))], {}, [ref(3)]), applyOk()]),
        b: doc([list([text(2, 'amount', '3'), text(3, 'total', '', kvcf({ calculate: tree(js('t()'), true) }))], {}, [ref(3)]), applyOk()]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }] },
      commit(2, textValue('4')),
      standalone('commit', [ref(2), textValue('4')], 'b'),
    ],
  },
  {
    name: 'recalculate-anchors',
    covers:
      "recalculate() anchors on the first live /CO field, else the first field, and runs with no source; a first-field anchor outside the /CO chain is neither calculated nor formatted; a document with no fields is an honest no-op.",
    plugins: solo({ sandbox: { builds: [{ run: [out({ formEffects: [setValue(4, textValue('9'))] }), out({})] }] } }),
    engine: {
      documents: {
        a: doc([list(CALC_FIELDS.slice(2), {}, [null, ref(99), ref(4)]), applyOk()]),
        b: doc([list(CALC_FIELDS.slice(2)), applyOk()]),
        c: doc([list([])]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }] },
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'c', bytes: { $bytes: '' } }] },
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
    plugins: solo({ sandbox: { builds: [{ run: [exception('kaboom'), budgetFault('format budget')] }] } }),
    engine: {
      documents: {
        a: doc([
          list([
            text(2, 'amount', '', kvcf({ format: tree(js('f()')) })),
            text(3, 'broken', '', kvcf({ format: tree(js('f()'), true) })),
            text(4, 'empty', '', kvcf({ format: tree(null) })),
          ]),
          applyOk(),
        ]),
      },
    },
    snapshot: 'none',
    steps: [...boot, commit(2, textValue('7')), commit(2, textValue('8')), commit(3, textValue('9')), commit(4, textValue('10'))],
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
          applyOk(),
        ]),
      },
    },
    snapshot: 'none',
    steps: [...boot, commit(2, textValue(''))],
  },
  {
    name: 'native-effect-statuses',
    covers:
      "The engine's per-effect statuses decide the result: any failed or skipped entry fails it with 'One or more native form effects failed', a rejected one fails it too, all unchanged is unchanged; an engine without a batch door fails before the call.",
    plugins: solo({ sandbox: { builds: [{}] } }),
    engine: {
      documents: {
        a: doc([
          list([NAME]),
          { path: 'forms.applyEffects', times: 1, value: { results: [{ index: 0, status: 'skipped', fields: [], changedWidgets: [] }], changedWidgets: [], meta: null } },
          { path: 'forms.applyEffects', times: 1, value: { results: [{ index: 0, status: 'rejected', fields: [], changedWidgets: [] }], changedWidgets: [], meta: null } },
          { path: 'forms.applyEffects', times: 1, value: { results: [{ index: 0, status: 'unchanged', fields: [], changedWidgets: [] }], changedWidgets: [], meta: null } },
          { path: 'forms.applyEffects', error: { name: 'EngineError', message: 'batch threw', code: 'Io' } },
        ]),
        b: doc([list([NAME])], { lacks: ['forms.applyEffects'] }),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'b', bytes: { $bytes: '' } }] },
      commit(1, textValue('a')),
      commit(1, textValue('b')),
      commit(1, textValue('c')),
      commit(1, textValue('d')),
      standalone('commit', [ref(1), textValue('e')], 'b'),
    ],
  },
  {
    name: 'no-change-is-unchanged',
    covers: 'A commit of the current value produces no effects: unchanged, no engine write. A field the snapshot no longer has fails.',
    plugins: solo({ sandbox: { builds: [{}] } }),
    engine: { documents: { a: doc([list([NAME])]) } },
    snapshot: 'none',
    steps: [...boot, commit(1, textValue('Ada')), commit(99, textValue('x')), standalone('commit', [{ kind: 'fqn', name: 'name' }, textValue('x')])],
  },
  {
    name: 'a-throwing-run-fails',
    covers: 'A sandbox run that throws fails the transaction with an invalid-output error carrying its message; a script setValue on a pushbutton rejects the transaction.',
    plugins: solo({
      sandbox: { builds: [{ run: [{ throws: { message: 'realm crashed' } }, out({ formEffects: [setValue(8, textValue('no'))] })] }] },
    }),
    engine: { documents: { a: doc([list([GUARD(), BUTTON]), applyOk()]) } },
    snapshot: 'none',
    steps: [...boot, commit(2, textValue('a')), commit(2, textValue('b'))],
  },
];

// ── budgets ───────────────────────────────────────────────────────────────────

const BUDGET = { maxExecutionMs: 50, maxMemoryBytes: 1024, maxStackBytes: 512, maxEffects: 2, maxOutputBytes: 100000 };
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
    covers: 'More form effects plus UI requests than maxEffects fails the transaction before the engine; exactly maxEffects passes.',
    plugins: solo({
      sandbox: {
        builds: [{ run: [out({ formEffects: [setValue(1, textValue('x'))], uiEffects: [alert('one'), alert('two')] }), out({ uiEffects: [alert('one')] })] }],
      },
      budget: BUDGET,
    }),
    engine: { documents: { a: doc([list([NAME, GUARD()]), applyOk()]) } },
    snapshot: 'none',
    steps: [...boot, commit(2, textValue('a')), commit(2, textValue('b'))],
  },
  {
    name: 'the-output-budget-at-the-boundary',
    covers: `Output bytes are the UTF-8 length of each output's JSON (${WIDE_BYTES} here, with 2-, 3- and 4-byte characters): exactly the budget passes.`,
    plugins: solo({ sandbox: { builds: [{ boot: [out(WIDE)] }] }, budget: { ...BUDGET, maxOutputBytes: WIDE_BYTES } }),
    engine: { documents: { a: doc([list([NAME]), ACTIONS_READ(), applyOk()], withScripts) } },
    snapshot: 'none',
    steps: [...boot, commit(1, textValue('a'))],
  },
  {
    name: 'the-output-budget-exceeded',
    covers: 'One byte over, a boot output degrades with the budget message, and a run output that crosses the aggregate fails the transaction.',
    plugins: solo({ sandbox: { builds: [{ boot: [out(WIDE)], run: [out(WIDE)] }] }, budget: { ...BUDGET, maxOutputBytes: WIDE_BYTES - 1 } }),
    engine: { documents: { a: doc([list([NAME, GUARD()]), ACTIONS_READ(), applyOk()], withScripts) } },
    snapshot: 'none',
    steps: [...boot, commit(1, textValue('a')), commit(2, textValue('b'))],
  },
  {
    name: 'the-time-budget',
    covers:
      "Each run receives what is left of the transaction's execution budget; once a run has taken all of it (a realm rebuilt behind a slow factory), the next run times out without reaching the sandbox, and the rebuilt realm's boot surfaces in the next transaction.",
    plugins: solo({
      sandbox: { builds: [{ boot: [{ dispose: true }] }, { defer: 'rebuild', boot: [out({ uiEffects: [alert('late boot')] })] }] },
      budget: BUDGET,
    }),
    engine: { documents: { a: doc([list([CODE()]), ACTIONS_READ(), applyOk()], withScripts) } },
    snapshot: 'none',
    steps: [...boot, commit(2, textValue('slow')), advance(60), settle('rebuild'), commit(2, textValue('next'))],
  },
  {
    name: 'the-time-budget-shrinks',
    covers: 'A run that took 20 of the 50 budgeted milliseconds leaves the next run 30.',
    plugins: solo({
      sandbox: { builds: [{ boot: [{ dispose: true }] }, { defer: 'rebuild' }] },
      budget: BUDGET,
    }),
    engine: { documents: { a: doc([list([CODE()]), ACTIONS_READ(), applyOk()], withScripts) } },
    snapshot: 'none',
    steps: [...boot, commit(2, textValue('slow')), advance(20), settle('rebuild')],
  },
];

// ── the capability over the actions plugin's realm ───────────────────────────

const OFF = { openSequence: 'off' };
const js_on = (sandbox: unknown) => ({ ...OFF, javascript: { enabled: true, sandbox } });
const scripted = (sandbox: unknown, extra: PluginSpec[] = []): PluginSpec[] => [
  { factory: 'interaction' },
  { factory: 'actions', config: js_on(sandbox) },
  ...extra,
  { factory: 'form' },
];
const USER = { origin: 'user', source: { kind: 'api' }, event: { scope: 'activate' } };
const adapter = { openUri: fnSpec('ui.openUri'), print: fnSpec('ui.print'), alert: fnSpec('ui.alert') };
const observe: Step[] = [
  act('setUiAdapter', [adapter]),
  act('onScriptDiagnostic', [fnSpec('onScriptDiagnostic')]),
  act('onScriptError', [fnSpec('onScriptError')]),
];

const REF_CANDIDATES = [
  { ...widgetAnnot(1, 109, undefined, { actions: { activate: tree(js('nm()')) } }), ref: { kind: 'nm', page: pr(1), nm: 'n1' } },
  { ...widgetAnnot(1, 110, undefined, { actions: { activate: tree(js('index()')) } }), ref: { kind: 'index', page: pr(1), index: 3, revision: 'r1' } },
];

const capabilityCases: ScenarioCase[] = [
  {
    name: 'commit-through-the-shared-realm',
    covers:
      "With scripting on, commitValue runs the K/V/C/F pipeline in the actions plugin's realm, surfaces boot UI requests, then user ones with diagnostics and the error, through the actions port, and re-reads after a batch.",
    plugins: scripted({
      builds: [
        {
          boot: [out({ uiEffects: [alert('boot nag', 1)] })],
          run: [
            out({ event: ev('1', { change: '3', selStart: 0, selEnd: 1 }) }),
            out({ event: ev('3') }),
            out({ event: ev('3'), uiEffects: [alert('validated')], diagnostics: [{ code: 'unsupported-api', message: 'util.printf' }] }),
            out({ formEffects: [setValue(3, textValue('6'))] }),
            out({ formEffects: [{ kind: 'setAppearanceText', ref: ref(3), text: '$6' }] }),
          ],
        },
      ],
    }),
    engine: { documents: { a: doc([list([AMOUNT, TOTAL], {}, [ref(3)]), ACTIONS_READ(), applyOk()], withScripts) } },
    snapshot: ['form::a'],
    steps: [...boot, ...observe, call('commitValue', [ref(2), textValue('3')])],
  },
  {
    name: 'scripted-writes-clear-the-spinner',
    covers:
      'A scripted write that commits nothing, is rejected, or fails clears the writing mark itself; one that commits leaves it to the re-read.',
    plugins: scripted({ builds: [{ run: [rejects('x'), budgetFault('boom'), out({ event: ev('ok') })] }] }),
    engine: { documents: { a: doc([list([NAME, GUARD()]), applyOk()]) } },
    snapshot: ['form::a'],
    steps: [
      ...boot,
      ...observe,
      call('setText', ['obj:1', 'Ada']),
      call('setText', ['obj:2', 'bad']),
      call('setText', ['obj:2', 'worse']),
      call('setText', ['obj:2', 'fine']),
    ],
  },
  {
    name: 'batch-writes-count-rejections',
    covers:
      "setValues and importValues report a rejected write as failed 'invalid-input' with the first diagnostic's message (or the status), and a failed one as 'operation-failed' with the error's message; a rejection is also announced on onValidationRejected with its diagnostics.",
    plugins: scripted({ builds: [{ run: [rejects('x'), budgetFault('boom'), out({ event: ev('ok') }), out({ event: ev('y', { rc: false }), diagnostics: [{ code: 'script-error', message: 'bad email' }] })] }] }),
    engine: { documents: { a: doc([list([NAME, GUARD()]), applyOk()]) } },
    snapshot: 'none',
    steps: [
      ...boot,
      ...observe,
      call('onValidationRejected', [fnSpec('validationRejected')]),
      call('setValues', [[
        { ref: ref(2), value: textValue('bad') },
        { ref: ref(2), value: textValue('worse') },
        { ref: ref(2), value: textValue('fine') },
        { ref: ref(2), value: textValue('noted') },
      ]]),
      call('importValues', [{ email: textValue('again') }]),
    ],
  },
  {
    name: 'reset-recalculates',
    covers:
      "With scripting on, a reset recalculates the /CO chain after its batch and folds the recalculation's UI, diagnostics and error into its result, surfaced with the caller's origin.",
    plugins: scripted({
      builds: [
        {
          run: [
            out({ formEffects: [setValue(4, textValue('0'))], uiEffects: [alert('recalculated')] }),
            out({ error: { kind: 'exception', message: 'calc threw' } }),
            out({}),
            budgetFault('recalc budget'),
          ],
        },
      ],
    }),
    engine: {
      documents: {
        a: doc([list([text(2, 'amount', '3'), text(4, 'total', '6', kvcf({ calculate: tree(js('t()')) }))], {}, [ref(4)]), applyOk()]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      ...observe,
      call('reset', ['obj:2']),
      call('resetFormAction', [[byName('amount')], false, 'lifecycle']),
      act('execute', [tree(resetNode(null)), USER]),
      call('resetFormAction', [null, false]),
    ],
  },
  {
    name: 'scripted-activation',
    covers:
      "When the dispatcher finds no /A tree, the form path reads the widget's activation from the page (loaded annotations first when the annotation plugin has them) and runs it as a scripted transaction; a widget without one is a scripted no-op.",
    plugins: scripted({ builds: [{ run: [out({ formEffects: [setValue(1, textValue('clicked'))] })] }] }),
    engine: {
      documents: {
        a: doc([
          list([NAME, BUTTON]),
          annotations(1, [widgetAnnot(1, 108), widgetAnnot(1, 109)], { times: 1 }),
          annotations(1, [
            widgetAnnot(1, 108, undefined, { actions: { activate: tree(js('click()')) } }),
            { ...widgetAnnot(1, 109), ref: { kind: 'nm', page: pr(1), nm: 'n1' } },
            { ...widgetAnnot(1, 110), ref: { kind: 'index', page: pr(1), index: 3, revision: 'r1' } },
            { ...widgetAnnot(1, 111), subtype: 'square' },
          ]),
          applyOk(),
        ]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      call('activateWidget', ['obj:8', aref(1, 108)]),
      call('activateWidget', ['obj:8', aref(1, 404)]),
      call('activateWidget', ['obj:8', { kind: 'nm', page: pr(1), nm: 'n1' }]),
      call('activateWidget', ['obj:8', { kind: 'index', page: pr(1), index: 3, revision: 'r1' }]),
      call('activateWidget', ['obj:8', { kind: 'index', page: pr(1), index: 3, revision: 'r2' }]),
      call('activateWidget', ['obj:8', aref(1, 111)]),
    ],
  },
  {
    name: 'scripted-activation-matches-refs',
    covers:
      "The form path finds the widget by ref the way sameAnnotationRef does: an /NM on the same page, an index with the same revision, never another page's object number.",
    plugins: scripted({ builds: [{ run: [out({ formEffects: [setValue(1, textValue('clicked'))] })] }] }),
    engine: {
      documents: {
        a: doc([
          list([NAME, BUTTON]),
          // Each activation reads the page twice: the dispatcher first
          // (finding no tree), then the form path.
          ...[0, 1, 2, 3, 4].flatMap((): ResponseSpec[] => [
            annotations(1, [], { times: 1 }),
            annotations(1, REF_CANDIDATES, { times: 1 }),
          ]),
          annotations(2, [], { times: 1 }),
          annotations(2, [widgetAnnot(1, 108, undefined, { actions: { activate: tree(js('wrong-page()')) } })], { times: 1 }),
          applyOk(),
        ]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      call('activateWidget', ['obj:8', { kind: 'nm', page: pr(1), nm: 'n1' }]),
      call('activateWidget', ['obj:8', { kind: 'nm', page: pr(1), nm: 'n2' }]),
      call('activateWidget', ['obj:8', { kind: 'index', page: pr(1), index: 3, revision: 'r1' }]),
      call('activateWidget', ['obj:8', { kind: 'index', page: pr(1), index: 4, revision: 'r1' }]),
      call('activateWidget', ['obj:8', { kind: 'index', page: pr(1), index: 3, revision: 'r2' }]),
      call('activateWidget', ['obj:8', aref(2, 108)]),
    ],
  },
  {
    name: 'scripted-activation-reads-the-folded-annotation',
    covers: 'With the annotation plugin a loaded widget answers its activation tree without a page read; one without an activation tree is a scripted no-op, and a hover tree with no root counts as none.',
    plugins: scripted({ builds: [{ run: [out({ formEffects: [setValue(1, textValue('clicked'))] })] }] }, [{ factory: 'annotation' }]),
    engine: {
      documents: {
        a: doc([
          list([NAME, BUTTON]),
          {
            path: 'annotations.listRawAll',
            value: {
              pages: [
                {
                  pageState: { page: pr(1) },
                  annotations: [
                    widgetAnnot(1, 108, undefined, { actions: { activate: tree(js('click()')) } }),
                    widgetAnnot(1, 109, undefined, { actions: { cursorEnter: tree(null) } }),
                    widgetAnnot(1, 110),
                  ],
                },
              ],
            },
          },
          annotations(1, [widgetAnnot(1, 108)]),
          applyOk(),
        ]),
      },
    },
    snapshot: 'none',
    steps: [
      ...boot,
      call('activateWidget', ['obj:8', aref(1, 108)]),
      call('activateWidget', ['obj:8', aref(1, 109)]),
      call('activateWidget', ['obj:8', aref(1, 110)]),
      call('notifyWidgetEvent', ['obj:8', aref(1, 109), 'cursorEnter']),
      call('notifyWidgetEvent', ['obj:8', aref(1, 110), 'cursorEnter']),
    ],
  },
  {
    name: 'script-nodes-commit-through-the-form',
    covers: "A JavaScript node's form effects reach the engine through the form plugin's commit sink, which re-reads.",
    plugins: scripted({ builds: [{ run: [out({ formEffects: [setValue(1, textValue('from-js'))] })] }] }),
    engine: { documents: { a: doc([list([NAME]), annotations(1, []), applyOk()]) } },
    snapshot: ['form::a'],
    steps: [...boot, act('execute', [tree(js('this.getField("name").value = "from-js";')), USER])],
  },
  {
    name: 'no-sandbox-host',
    covers:
      'A host with scripting enabled but no sandbox has no transaction port, so the form commits without scripts, exactly as with scripting disabled.',
    plugins: scripted(null),
    engine: {
      documents: {
        a: doc([list([AMOUNT, TOTAL], {}, [ref(3)]), annotations(1, []), { path: 'forms.setValue', value: { changedWidgets: [v3Widget({ annotObjectNumber: 102, pageObjectNumber: 1 })] } }]),
      },
    },
    snapshot: 'none',
    steps: [...boot, call('commitValue', [ref(2), textValue('3')]), call('activateWidget', ['obj:2', aref(1, 102)])],
  },
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/form-scripting.json',
    describes: 'packages/plugin/form scripting: the standalone K/V/C/F controller and the capability over the actions realm.',
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
      ...capabilityCases,
    ],
  },
];
