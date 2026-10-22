import assert from 'node:assert/strict';
import { test } from 'node:test';
import { suites as scripting } from '../src/suites/form-scripting';
import { runScenario } from '../src/scenario';
test('all 46 current form scripting cases execute without obsolete calls or unmatched engine requests', async () => {
  const cases = scripting.flatMap((suite) => (suite.kind === 'scenario' ? [...suite.cases] : []));
  assert.equal(cases.length, 46);
  for (const input of cases) {
    assert.ok('steps' in input);
    const result = await runScenario(input);
    assert.deepEqual(result.unmatched, [], input.name);
    assert.deepEqual(
      result.trace.filter((entry) => entry.outcome && 'threw' in entry.outcome),
      [],
      input.name,
    );
  }
});

import { suites as actions } from '../src/suites/form-actions';
import type { ScenarioCase, TraceEntry } from '../src/scenario';
import { ref, objRef, pr, operationAt, LOCAL, act } from '../src/suites/form-script-fixtures';
const inputs = [...scripting, ...actions].flatMap((suite) =>
  suite.kind === 'scenario' ? [...suite.cases] : [],
);
const cache = new Map<string, ReturnType<typeof runScenario>>();
const run = (name: string) => {
  let result = cache.get(name);
  if (!result) {
    const input = inputs.find((input) => input.name === name);
    assert.ok(input && 'steps' in input, name);
    result = runScenario(input);
    cache.set(name, result);
  }
  return result;
};
const object = (value: unknown): Record<string, unknown> => {
  assert.ok(value !== null && typeof value === 'object' && !Array.isArray(value));
  return value as Record<string, unknown>;
};
const array = (value: unknown): unknown[] => {
  assert.ok(Array.isArray(value));
  return value;
};
const resolved = (trace: TraceEntry[]) =>
  trace.flatMap((entry) =>
    entry.settled.filter((item) => 'resolved' in item).map((item) => item.resolved),
  );
const controllers = (trace: TraceEntry[]) =>
  resolved(trace)
    .filter((value) => value !== null && typeof value === 'object' && 'effects' in value)
    .map(object);
const callbacks = (trace: TraceEntry[], name: string) =>
  trace.flatMap((entry) => entry.callbacks.filter((callback) => callback.fn === name));
const engine = (trace: TraceEntry[], path: string) =>
  trace.flatMap((entry) => entry.engine.map(object).filter((call) => call.path === path));
const effect = (number: number, value: unknown) => ({ kind: 'setValue', ref: ref(number), value });
const textEffect = (number: number, value: string | null) => effect(number, { value });
const accepted = (effects: unknown[] = [], over: Record<string, unknown> = {}) => ({
  status: 'accepted',
  effects,
  bootEffects: [],
  uiEffects: [],
  diagnostics: [],
  ...over,
});
const failed = (kind: string, message: string, diagnostics: unknown[] = []) => ({
  status: 'failed',
  effects: [],
  bootEffects: [],
  uiEffects: [],
  diagnostics,
  error: { kind, message },
});
const rejected = (over: Record<string, unknown> = {}) => ({
  status: 'rejected',
  effects: [],
  bootEffects: [],
  uiEffects: [],
  diagnostics: [],
  ...over,
});
const diagnostic = (message: string, code = 'script-error') => ({ code, message });
const incomplete = failed('invalid-output', 'Refusing to execute an incomplete PDF action tree');
const bootDiagnostic = (message: string) =>
  diagnostic('Document boot script failed (continuing without it): ' + message);
const alert = (message: string, phase = 'user', icon = 0) => ({
  kind: 'alert',
  message,
  icon,
  phase,
});
const expected: Record<string, unknown[]> = {
  'standalone-configured-environment': [
    accepted([textEffect(1, 'Grace')]),
    accepted([textEffect(1, 'Hopper')]),
  ],
  'standalone-default-environment': [accepted([textEffect(2, '')])],
  'standalone-identity-callback': [accepted()],
  'standalone-dispose': [accepted([textEffect(1, 'x')])],
  'form-values-cross-into-the-overlay': [
    accepted(),
    accepted([textEffect(4, null)]),
    accepted([textEffect(5, 'M')]),
    accepted([textEffect(6, 'g')]),
    accepted([textEffect(6, null)]),
    accepted([effect(7, { selectedValues: ['r', 'g'] })]),
    failed('invalid-output', "Form field 'name' (text) does not take this value"),
    failed('invalid-output', "Form field 'go' (pushbutton) does not take this value"),
    failed('invalid-output', "Form field 'name' (text) does not take this value"),
  ],
  'a-cleared-toggle-proposes-off': [accepted([textEffect(4, null)])],
  'validated-values-cross-back': [
    ['', 9],
    [null, 4],
    [null, 4],
    ['5', 4],
    ['g', 6],
    [null, 6],
  ]
    .map(([value, number]) =>
      accepted([textEffect(Number(number), value === null ? null : String(value))]),
    )
    .concat([
      accepted([effect(7, { selectedValues: ['g'] })]),
      accepted([effect(7, { selectedValues: [] })]),
      accepted([textEffect(5, 'true')]),
    ]),
  'boot-effects-land-with-the-commit': [
    accepted([textEffect(1, 'initialized'), textEffect(2, 'mine')], {
      bootEffects: [textEffect(1, 'initialized')],
      uiEffects: [alert('Needs a newer viewer', 'boot', 1)],
    }),
    accepted([textEffect(2, 'again')]),
  ],
  'a-failed-boot-degrades': [
    accepted([textEffect(1, 'a')], {
      diagnostics: [bootDiagnostic('app.findComponent is not a function')],
    }),
  ],
  'a-boot-error-output-degrades': [
    accepted([textEffect(2, 'b')], {
      uiEffects: [alert('nag', 'boot')],
      diagnostics: [
        diagnostic('app.findComponent', 'unsupported-api'),
        bootDiagnostic('not a function'),
      ],
    }),
  ],
  'a-failed-catalog-read-degrades': [
    accepted([textEffect(1, 'a')], { diagnostics: [bootDiagnostic('actions unavailable')] }),
  ],
  'rejection-keeps-boot-effects': [1, 2, 3].map(() =>
    rejected({
      bootEffects: [textEffect(1, 'booted')],
      diagnostics: [diagnostic('app.viewerVersion', 'unsupported-api')],
    }),
  ),
  'activation-runs-in-the-transaction': [
    accepted([textEffect(1, 'Mouse Up:Field')], {
      uiEffects: [alert('Summary ready'), { kind: 'gotoPage', page: 1, phase: 'user' }],
    }),
  ],
  'activation-edits-its-own-field': [accepted([textEffect(9, 'mine')])],
  'recalculate-keeps-a-boot-reset': [
    accepted([{ kind: 'reset', refs: [ref(4)] }], {
      bootEffects: [{ kind: 'reset', refs: [ref(4)] }],
    }),
  ],
  'activation-refusals': [
    incomplete,
    accepted(),
    accepted(),
    failed('exception', 'activation broke'),
  ],
  'keystroke-two-passes': [accepted([textEffect(1, 'typed-into'), textEffect(2, 'ABC!')])],
  'keystroke-splices-in-utf-16': ['😀Y', 'abc-d', 'abc+d', 'a*cd', ''].map((value) =>
    accepted([textEffect(2, value)]),
  ),
  'keystroke-empty-values': [accepted(), accepted(), accepted(), accepted()],
  'keystroke-fault-ladder': [
    accepted([textEffect(2, '42')], {
      diagnostics: [
        diagnostic('Keystroke script failed (kept the typed value): typing broke'),
        diagnostic('Keystroke commit script failed (kept the typed value): commit broke'),
      ],
    }),
    failed('budget', 'typing budget'),
    failed('budget', 'commit budget'),
    rejected(),
    rejected(),
    rejected({
      diagnostics: [
        diagnostic('Keystroke script failed (kept the typed value): typing broke again'),
      ],
    }),
  ],
  'keystroke-trees': [incomplete, accepted([textEffect(3, 'b')]), accepted([textEffect(4, 'Yes')])],
  'validate-fault-ladder': [
    rejected({ uiEffects: [alert('Invalid email')] }),
    accepted([textEffect(2, 'ok')], {
      diagnostics: [diagnostic('Validate script failed (accepted the value): validator broke')],
    }),
    failed('budget', 'synthetic budget fault'),
    accepted([textEffect(1, 'validated'), textEffect(2, 'a@b.test')]),
  ],
  'validate-trees': [incomplete, accepted([textEffect(3, 'b')])],
  'calculation-order': [
    accepted(
      [
        textEffect(2, '5'),
        textEffect(4, '10'),
        { kind: 'setAppearanceText', ref: ref(4), text: '$10.00' },
        { kind: 'setAppearanceText', ref: ref(6), text: 'E' },
      ],
      {
        diagnostics: [
          diagnostic('Calculate script failed (field left unchanged): explode is not defined'),
        ],
      },
    ),
  ],
  'calculation-refusals': [failed('budget', 'calc budget'), incomplete],
  'recalculate-anchors': [accepted([textEffect(4, '9')]), accepted(), accepted()],
  'format-fault-ladder': [
    accepted([textEffect(2, '7')], {
      diagnostics: [diagnostic('Format script failed (kept the raw value): kaboom')],
    }),
    failed('budget', 'format budget'),
    incomplete,
    accepted([textEffect(4, '10')]),
  ],
  'effects-canonicalise': [
    accepted([
      { kind: 'reset', refs: [ref(3)] },
      textEffect(4, 're-set'),
      { kind: 'setDisplay', ref: ref(5), display: 'hidden' },
      textEffect(7, 'dflt'),
      { kind: 'setAppearanceText', ref: ref(5), text: 'last' },
      { kind: 'setAppearanceText', ref: ref(3), text: 'three' },
    ]),
  ],
  'no-change-is-unchanged': [
    accepted(),
    failed('invalid-output', "Form field 'obj:99' no longer exists"),
    accepted([textEffect(1, 'x')]),
  ],
  'a-throwing-run-fails': [failed('invalid-output', 'realm crashed')],
  'the-effects-budget': [
    {
      ...failed('budget', 'Form script transaction exceeded 2 effects'),
      uiEffects: [alert('one'), alert('two')],
    },
    accepted([textEffect(2, 'b')], { uiEffects: [alert('one')] }),
  ],
  'the-output-budget-at-the-boundary': [
    accepted([textEffect(1, 'a')], { uiEffects: [alert('é € 😀 ascii', 'boot', 2)] }),
  ],
  'the-output-budget-exceeded': [
    accepted([textEffect(1, 'a')], {
      uiEffects: [alert('é € 😀 ascii', 'boot', 2)],
      diagnostics: [bootDiagnostic('Form script transaction output exceeded 191 bytes')],
    }),
    {
      ...failed('budget', 'Form script transaction output exceeded 191 bytes'),
      uiEffects: [alert('é € 😀 ascii', 'user', 2)],
    },
  ],
  'the-time-budget': [
    failed('budget', 'Form script transaction timed out'),
    accepted([textEffect(2, '')], { uiEffects: [alert('late boot', 'boot')] }),
  ],
  'the-time-budget-shrinks': [accepted([textEffect(2, '')])],
};
test('all 35 direct controller cases have independently specified complete effects, errors, phases and diagnostics', async () => {
  assert.equal(Object.keys(expected).length, 35);
  for (const [name, result] of Object.entries(expected))
    assert.deepEqual(controllers((await run(name)).trace), result, name);
});
const error = (name: string, message: string, over: Record<string, unknown> = {}) => ({
  $error: { name, message, ...over },
});
const pluginError = (
  message: string,
  code = 'operation-failed',
  over: Record<string, unknown> = {},
) => error('PluginError', message, { code, capability: 'form', permission: null, ...over });
const notFound = pluginError('no form field owns this widget', 'not-found');
const expectedRejections: Record<string, unknown[]> = {
  'standalone-dispose': [error('Error', 'Form scripting controller is disposed')],
  'a-throwing-run-fails': [error('Error', "Form field 'go' (pushbutton) does not take this value")],
  'native-effect-statuses': [
    pluginError('batch threw', 'operation-failed', {
      cause: error('EngineError', 'batch threw', { code: 'Io' }),
    }),
    pluginError('atomic unsupported', 'operation-failed', {
      cause: error('EngineError', 'atomic unsupported', { code: 'Unsupported' }),
    }),
  ],
  'scripted-writes-clear-the-spinner': [pluginError('boom')],
  'scripted-activation': [notFound],
  'scripted-activation-matches-refs': [notFound, notFound, notFound],
  'a-kindless-address-matches-no-candidate': [notFound],
  'reset-form-action-door': [
    pluginError('gone', 'operation-failed', {
      cause: error('EngineError', 'gone', { code: 'Io' }),
    }),
  ],
};
test('all 59 cases settle completely with only exact named rejection channels and no reports', async () => {
  assert.equal(inputs.length, 59);
  assert.equal(new Set(inputs.map((input) => input.name)).size, 59);
  for (const input of inputs) {
    const result = await run(input.name),
      trace = result.trace;
    assert.deepEqual(result.unmatched, [], input.name);
    assert.deepEqual(
      trace.filter((entry) => entry.outcome && 'threw' in entry.outcome),
      [],
      input.name,
    );
    assert.deepEqual(
      trace.flatMap((entry) => entry.reports),
      [],
      input.name,
    );
    assert.deepEqual(
      trace.flatMap((entry) =>
        entry.settled.filter((item) => 'rejected' in item).map((item) => item.rejected),
      ),
      expectedRejections[input.name] ?? [],
      input.name,
    );
    const settled = new Set(trace.flatMap((entry) => entry.settled.map((item) => item.promise)));
    assert.deepEqual(
      trace.filter(
        (entry) =>
          entry.outcome && 'promise' in entry.outcome && !settled.has(entry.outcome.promise),
      ),
      [],
      input.name + ' pending',
    );
    const diagnostics = trace.flatMap((entry) =>
      entry.callbacks.filter((callback) => callback.fn.startsWith('console.')),
    );
    assert.deepEqual(diagnostics, [], input.name);
  }
});
const inputOf = (name: string): ScenarioCase => {
  const input = inputs.find((input) => input.name === name);
  assert.ok(input && 'steps' in input);
  return input;
};
const readReturns = (name: string, trace: TraceEntry[], method = 'getValue') =>
  inputOf(name).steps.flatMap((step, index) => {
    if (step.op !== 'call' || step.method !== method) return [];
    const outcome = trace[index]!.outcome;
    assert.ok(outcome && 'returned' in outcome, name + ' ' + method);
    return [outcome.returned];
  });
const writes = (trace: TraceEntry[]) =>
  engine(trace, 'apply').map((call) => object(array('args' in call ? call.args : [])[0]).ops);
const write = (number: number, value: string | null) => ({
  type: 'forms.setValue',
  field: ref(number),
  value: { value },
});
const reset = (...numbers: number[]) => ({ type: 'forms.reset', fields: numbers.map(ref) });
const expectedOps: Record<string, unknown[]> = {
  'native-effect-statuses': [
    [write(1, 'Ada')],
    [write(1, 'Grace')],
    [write(1, 'Hopper')],
    [write(1, 'Again')],
  ],
  'commit-through-the-shared-realm': [
    [write(2, '3'), write(3, '6'), { type: 'forms.setAppearanceText', field: ref(3), text: '$6' }],
  ],
  'scripted-writes-clear-the-spinner': [[write(2, 'ok')]],
  'batch-writes-count-rejections': [[write(1, 'Grace')], [write(1, 'Hopper')]],
  'reset-recalculates': [[reset(2), write(4, '0')]],
  'scripted-activation': [[write(1, 'clicked')]],
  'scripted-activation-matches-refs': [],
  'a-kindless-address-matches-no-candidate': [],
  'scripted-activation-reads-the-folded-annotation': [[write(1, 'clicked')]],
  'script-nodes-commit-through-the-form': [[write(1, 'from-js')]],
  'no-sandbox-host': [[write(2, '3')]],
  'the-reset-form-executor': [[reset(1, 2)], [reset(3)], [reset(1, 2, 3, 6)], [reset(3)]],
  'an-inert-reset': [[reset(1, 2, 3, 6)]],
  'a-failed-reset-fails-the-node': [[reset(1, 2, 3, 6)], [reset(1, 2, 3, 6)]],
  'reset-form-action-door': [[reset(3)], [reset(1, 2, 3, 6)], [reset(1, 2, 3, 6)]],
  'the-submit-resolver': [],
  'resolve-submit-dataset-door': [],
  'commit-script-form-effects': [[write(1, 'x')], [write(1, 'y')]],
  'commit-sink-without-annotation': [[write(1, 'x')]],
  'activation-without-actions': [],
  'activation-is-delegated': [[reset(3)]],
  'a-refused-activation-is-dispatched': [],
  'the-widget-event-feed': [[reset(3)], [reset(3)], [reset(3)]],
  'hover-pre-checks-the-folded-annotation': [[reset(3)]],
};
const values: Record<string, unknown[]> = {
  'native-effect-statuses': [{ value: 'Grace' }, { value: 'Grace' }, { value: 'Grace' }],
  'commit-through-the-shared-realm': [{ value: '3' }, { value: '6' }],
  'scripted-writes-clear-the-spinner': [null, null, { value: 'ok' }],
  'batch-writes-count-rejections': [{ value: 'Hopper' }],
  'reset-recalculates': [null, { value: '0' }, { value: '0' }],
  'scripted-activation': [{ value: 'clicked' }],
  'scripted-activation-reads-the-folded-annotation': [{ value: 'clicked' }],
  'script-nodes-commit-through-the-form': [{ value: 'from-js' }],
  'no-sandbox-host': [{ value: '3' }, { value: '2' }],
  'the-reset-form-executor': [null, null, null, { checked: false }],
  'reset-form-action-door': [null],
  'commit-script-form-effects': [{ value: 'x' }, { value: 'x' }, { value: 'x' }],
  'commit-sink-without-annotation': [{ value: 'x' }],
  'activation-is-delegated': [null],
  'the-widget-event-feed': [null],
  'hover-pre-checks-the-folded-annotation': [null],
};
test('all 24 native integration cases send independently specified atomic ops and expose complete successful or rolled-back values', async () => {
  assert.equal(Object.keys(expectedOps).length, 24);
  for (const [name, ops] of Object.entries(expectedOps)) {
    const { trace } = await run(name);
    assert.deepEqual(writes(trace), ops, name);
    assert.deepEqual(readReturns(name, trace), values[name] ?? [], name + ' values');
    assert.equal(
      engine(trace, 'forms.list').length,
      name === 'commit-script-form-effects' ? 2 : 1,
      name + ' no forced reread',
    );
    assert.equal(
      engine(trace, 'annotations.list').length,
      [
        'scripted-activation-reads-the-folded-annotation',
        'hover-pre-checks-the-folded-annotation',
      ].includes(name)
        ? 1
        : name === 'commit-script-form-effects'
          ? 2
          : 0,
      name + ' no reload',
    );
  }
  const initial = { 'parent.c1': 'one', 'parent.c2': 'two', other: 'three', agree: true };
  for (const name of ['an-inert-reset', 'a-failed-reset-fails-the-node'])
    assert.deepEqual(
      readReturns(name, (await run(name)).trace, 'exportValues'),
      name === 'an-inert-reset' ? [initial] : [initial, initial],
    );
  const resetTrace = (await run('reset-form-action-door')).trace;
  assert.deepEqual(readReturns('reset-form-action-door', resetTrace, 'exportValues'), [
    { 'parent.c1': 'one', 'parent.c2': 'two', agree: true },
    { agree: false },
  ]);
  const complete = object(
    readReturns(
      'the-reset-form-executor',
      (await run('the-reset-form-executor')).trace,
      'getSnapshot',
    )[0],
  );
  const fields = array(complete.fields).map(object);
  assert.deepEqual(
    fields
      .filter((field) => ['text', 'checkbox'].includes(String(field.family)))
      .map((field) => [field.ref, field.valueEntry]),
    [
      [ref(1), { kind: 'none' }],
      [ref(2), { kind: 'none' }],
      [ref(3), { kind: 'none' }],
      [ref(6), { kind: 'scalar', value: 'Off' }],
    ],
  );
  assert.equal(fields.find((field) => field.family === 'checkbox')!.checked, false);
  assert.equal(
    object(array(fields.find((field) => field.family === 'checkbox')!.widgets)[0]).checked,
    false,
  );
});
test('sandbox inputs carry current fields, configured environment and exact K/V/C/F stage values', async () => {
  const configured = (await run('standalone-configured-environment')).trace;
  const boot = callbacks(configured, 'sandbox.boot');
  assert.equal(boot.length, 1);
  assert.deepEqual(boot[0]!.args, [
    0,
    ['var a = 1;', 'var b = 2;'],
    {
      document: { id: 'a', fileName: 'proposal.pdf', pageCount: 2, pageNumber: 0 },
      identity: { name: 'Ada', loginName: '', corporation: '', email: 'ada@example.test' },
      environment: { nowMs: 1234, utcOffsetMinutes: 180, randomSeed: 7 },
      fields: [
        {
          ref: ref(1),
          name: 'name',
          family: 'text',
          value: 'Ada',
          defaultValue: null,
          display: 'visible',
          readOnly: false,
          required: false,
        },
      ],
      event: { kind: 'name-tree-boot' },
    },
    {
      maxExecutionMs: 50,
      maxMemoryBytes: 16777216,
      maxStackBytes: 524288,
      maxEffects: 256,
      maxOutputBytes: 1048576,
    },
  ]);
  for (const name of ['fileName', 'now', 'offset', 'seed'])
    assert.equal(callbacks(configured, name).length, 1, name);
  const defaultRun = callbacks((await run('standalone-default-environment')).trace, 'sandbox.run');
  assert.deepEqual(object(defaultRun[0]!.args[2]).identity, {
    name: 'Una',
    loginName: 'u-1',
    corporation: '',
    email: '',
  });
  assert.deepEqual(object(defaultRun[0]!.args[2]).environment, {
    nowMs: 1500,
    utcOffsetMinutes: -new Date(1500).getTimezoneOffset(),
    randomSeed: 3669834966,
  });
  const identity = callbacks((await run('standalone-identity-callback')).trace, 'sandbox.boot');
  assert.deepEqual(object(identity[0]!.args[2]).identity, {
    name: 'u-2',
    loginName: 'u-2',
    corporation: 'Fn Inc',
    email: '',
  });
  const passes = callbacks((await run('keystroke-two-passes')).trace, 'sandbox.run');
  assert.deepEqual(
    passes.map((pass) => [pass.args[1], object(pass.args[2]).event]),
    [
      [
        'ks()',
        {
          kind: 'field-keystroke',
          target: ref(2),
          source: ref(2),
          value: 'old',
          change: 'abc',
          selStart: 0,
          selEnd: 3,
          willCommit: false,
        },
      ],
      [
        'ks()',
        {
          kind: 'field-keystroke',
          target: ref(2),
          source: ref(2),
          value: 'ABC',
          change: '',
          selStart: 0,
          selEnd: 0,
          willCommit: true,
        },
      ],
    ],
  );
  assert.deepEqual(
    passes.map((pass) => array(object(pass.args[2]).fields).map((field) => object(field).value)),
    [
      ['Ada', 'old'],
      ['typed-into', 'old'],
    ],
  );
  const shared = callbacks((await run('commit-through-the-shared-realm')).trace, 'sandbox.run');
  assert.deepEqual(
    shared.map((pass) => [pass.args[1], object(object(pass.args[2]).event).kind]),
    [
      ['AFNumber_Keystroke(2)', 'field-keystroke'],
      ['AFNumber_Keystroke(2)', 'field-keystroke'],
      ['validate()', 'field-validate'],
      ['calc()', 'field-calculate'],
      ['fmt()', 'field-format'],
    ],
  );
  assert.deepEqual(
    shared.map((pass) => array(object(pass.args[2]).fields).map((field) => object(field).value)),
    [
      ['1', '2'],
      ['1', '2'],
      ['3', '2'],
      ['3', '2'],
      ['3', '6'],
    ],
  );
  const anchor = callbacks((await run('recalculate-anchors')).trace, 'sandbox.run');
  assert.deepEqual(
    anchor.map((pass) => object(pass.args[2]).event),
    [
      { kind: 'field-calculate', target: ref(4), value: '2' },
      { kind: 'field-format', target: ref(4), value: '9' },
    ],
  );
  assert.deepEqual(
    anchor.map((pass) => object(object(pass.args[2]).document).pageNumber),
    [1, 1],
  );
  const pages = callbacks((await run('keystroke-empty-values')).trace, 'sandbox.run').filter(
    (pass) => object(object(pass.args[2]).event).willCommit === false,
  );
  assert.deepEqual(
    pages.map((pass) => object(object(pass.args[2]).document).pageNumber),
    [1, 0, 0, 1],
  );
  assert.deepEqual(
    callbacks((await run('the-time-budget-shrinks')).trace, 'sandbox.run').map(
      (pass) => object(pass.args[3]).maxExecutionMs,
    ),
    [50, 30],
  );
  for (const name of Object.keys(expected))
    assert.deepEqual(
      engine((await run(name)).trace, 'apply'),
      [],
      name + ' controller owns no writes',
    );
});
test('native scripted confirmation preserves complete fields, own widget rows, exact opIds and transactional facts', async () => {
  const name = 'commit-through-the-shared-realm',
    input = inputOf(name),
    { trace } = await run(name);
  const operations = engine(trace, 'apply');
  assert.equal(operations.length, 1);
  assert.deepEqual('args' in operations[0]! ? operations[0]!.args : [], [
    {
      ops: [
        write(2, '3'),
        write(3, '6'),
        { type: 'forms.setAppearanceText', field: ref(3), text: '$6' },
      ],
    },
    { opId: operationAt(1) },
  ]);
  const facts = callbacks(trace, 'valueChanged');
  assert.equal(facts.length, 2);
  assert.deepEqual(
    facts.map((fact) => ({
      ref: object(object(fact.args[0]).field).ref,
      value: object(object(fact.args[0]).field).value,
      origin: object(fact.args[0]).origin,
    })),
    [2, 3].map((number, index) => ({
      ref: ref(number),
      value: number === 2 ? '3' : '6',
      origin: { ...LOCAL, tx: { id: operationAt(1), index, count: 3 } },
    })),
  );
  const snapshot = object(readReturns(name, trace, 'getSnapshot')[0]);
  assert.deepEqual(
    array(snapshot.fields).map((field) => [object(field).ref, object(field).valueEntry]),
    [
      [ref(2), { kind: 'scalar', value: '3' }],
      [ref(3), { kind: 'scalar', value: '6' }],
    ],
  );
  assert.deepEqual(
    array(snapshot.widgets).map((widget) => [object(widget).ref, object(widget).field]),
    [
      [objRef(102), ref(2)],
      [objRef(103), ref(3)],
    ],
  );
  assert.deepEqual(
    callbacks(trace, 'ui.alert').map((callback) => callback.args),
    [
      ['boot nag', { origin: 'user', phase: 'boot', icon: 1 }],
      ['validated', { origin: 'user', phase: 'user', icon: 0 }],
    ],
  );
  assert.deepEqual(
    callbacks(trace, 'onScriptDiagnostic').map((callback) => callback.args),
    [[{ code: 'unsupported-api', message: 'util.printf' }]],
  );
  assert.deepEqual(callbacks(trace, 'onScriptError'), []);
  for (const name of ['the-reset-form-executor', 'reset-form-action-door', 'reset-recalculates']) {
    const input = inputOf(name);
    for (const response of input.engine?.documents?.a?.responses ?? []) {
      if (response.path !== 'apply' || !response.value) continue;
      const answer = object(response.value),
        items = array(answer.items).map(object),
        events = (response.events ?? []).map(object);
      for (const item of items) {
        if (item.type !== 'forms.reset' && item.type !== 'forms.setValue') continue;
        const fields = item.type === 'forms.reset' ? array(item.fields) : [item.field];
        assert.deepEqual(
          object(item.meta).changedFields,
          fields.map((field) => object(field).ref),
          name + ' own changed refs',
        );
        for (const field of fields) {
          const record = object(field),
            own = events.find(
              (event) => JSON.stringify(object(event.field).ref) === JSON.stringify(record.ref),
            );
          assert.ok(own);
          assert.deepEqual(own.field, record);
          assert.deepEqual(
            array(own.widgets).map((widget) => object(widget).field),
            array(own.widgets).map(() => record.ref),
          );
          assert.deepEqual(object(own.meta).changedFields, [record.ref]);
          assert.deepEqual(object(own.meta).changedWidgets, record.widgets);
        }
      }
      for (const [index, event] of events.entries())
        assert.deepEqual(
          object(event.origin).tx,
          { id: object(answer.meta).opId, index, count: events.length },
          name + ' tx',
        );
    }
  }
});
const methodResolved = (name: string, trace: TraceEntry[], method: string) => {
  const promises = new Set(
    inputOf(name).steps.flatMap((step, index) => {
      const outcome = trace[index]!.outcome;
      return step.op === 'call' && step.method === method && outcome && 'promise' in outcome
        ? [outcome.promise]
        : [];
    }),
  );
  return trace.flatMap((entry) =>
    entry.settled
      .filter((item) => promises.has(item.promise) && 'resolved' in item)
      .map((item) => item.resolved),
  );
};
test('action refusals, explicit submit diagnostics and provenance stay exact at their current channels', async () => {
  const inert = (detail: string, status = 'inert', code = 'executor-inert') => ({
    status: status === 'failed' ? 'partial' : status,
    nodes: [{ path: [], type: 'reset-form', status, detail }],
    diagnostics: [{ code, message: 'reset-form: ' + detail }],
  });
  assert.deepEqual(
    methodResolved('an-inert-reset', (await run('an-inert-reset')).trace, 'execute'),
    [
      inert('reset-form resolved no fields'),
      inert('reset-form resolved no fields'),
      inert('reset unavailable', 'failed', 'executor-failed'),
    ],
  );
  assert.deepEqual(
    methodResolved(
      'a-failed-reset-fails-the-node',
      (await run('a-failed-reset-fails-the-node')).trace,
      'execute',
    ),
    [
      inert('reset rejected', 'failed', 'executor-failed'),
      inert('batch refused', 'failed', 'executor-failed'),
    ],
  );
  const buttonDiagnostic = {
    code: 'submit-entry-unsupported',
    message: "submit: 'button' (pushbutton) has no representable submission value in v1 — skipped",
  };
  const entries = [
    { name: 'parent.c1', value: 'one' },
    { name: 'parent.c2', value: 'two' },
    { name: 'other', value: 'three' },
    { name: 'agree', value: 'Yes' },
  ];
  const dataset = (url: string, items: unknown[], over: Record<string, unknown> = {}) => ({
    url,
    method: 'post',
    format: 'fdf',
    flagsRaw: 0,
    entries: items,
    origin: 'user',
    event: { scope: 'activate' },
    ...over,
  });
  const host = (await run('resolve-submit-dataset-door')).trace;
  assert.deepEqual(methodResolved('resolve-submit-dataset-door', host, 'resolveSubmitDataset'), [
    dataset('https://home.test/x', []),
    dataset('https://home.test/x', entries, {
      charSet: 'utf-16',
      origin: 'lifecycle',
      event: { scope: 'document', name: 'open' },
    }),
    dataset('https://home.test/x', entries),
  ]);
  assert.deepEqual(
    callbacks(host, 'diagnose').map((callback) => callback.args),
    [[buttonDiagnostic]],
  );
  const submission = (await run('the-submit-resolver')).trace;
  assert.deepEqual(
    callbacks(submission, 'submitHandler').map((callback) => callback.args[0]),
    [
      dataset('https://home.test/submit', entries.slice(0, 2)),
      dataset('https://home.test/submit', entries, { charSet: 'utf-8' }),
    ],
  );
  assert.deepEqual(
    methodResolved('the-submit-resolver', submission, 'execute').map(
      (value) => object(value).diagnostics,
    ),
    [
      [
        buttonDiagnostic,
        {
          code: 'submit-entry-unsupported',
          message:
            "submit: 'secret' carries the NoExport flag — the veto is unconditional (ISO 12.7.6.2), skipped",
        },
      ],
      [],
    ],
  );
  const sink = (await run('commit-script-form-effects')).trace;
  assert.deepEqual(methodResolved('commit-script-form-effects', sink, 'commitScriptFormEffects'), [
    { status: 'applied' },
    { status: 'applied' },
    { status: 'failed', error: 'form.fill requires doc.forms.fill' },
  ]);
  assert.equal(engine(sink, 'apply').filter((call) => 'doc' in call && call.doc === 'b').length, 0);
  const batch = (await run('batch-writes-count-rejections')).trace;
  assert.deepEqual(methodResolved('batch-writes-count-rejections', batch, 'setValues'), [
    {
      applied: [ref(1)],
      skipped: [],
      failed: [
        {
          ref: ref(2),
          error: {
            code: 'invalid-input',
            message: 'a text field takes { value }, not { selectedValues }',
            capability: 'form',
            permission: null,
          },
        },
      ],
    },
  ]);
  assert.deepEqual(methodResolved('batch-writes-count-rejections', batch, 'importValues'), [
    {
      applied: [ref(1)],
      skipped: [
        { ref: 'missing', reason: 'the form has no field with this name' },
        { ref: 'email', reason: "a text field can't take this value" },
      ],
      failed: [],
    },
  ]);
  const validation = callbacks(batch, 'validationRejected');
  assert.equal(validation.length, 1);
  assert.deepEqual(object(validation[0]!.args[0]).issues, [
    { code: 'script-error', message: 'bad email' },
  ]);
  assert.deepEqual(object(object(validation[0]!.args[0]).field).valueEntry, { kind: 'none' });
  const refusedActivation = methodResolved(
    'a-refused-activation-is-dispatched',
    (await run('a-refused-activation-is-dispatched')).trace,
    'activateWidget',
  ).map(object);
  assert.equal(refusedActivation[0]!.kind, 'dispatched');
  const result = object(refusedActivation[0]!.result);
  assert.equal(result.status, 'refused');
  assert.deepEqual(object(object(array(result.steps)[0]).result), {
    status: 'refused',
    nodes: [],
    diagnostics: [{ code: 'incomplete-tree', message: 'refused: the action tree is incomplete' }],
  });
  const feed = callbacks((await run('the-widget-event-feed')).trace, 'onAction');
  assert.equal(feed.length, 3);
  for (const callback of feed)
    assert.deepEqual(object(callback.args[0]).source, {
      kind: 'widget',
      field: ref(1),
      annotation: objRef(101),
      page: pr(1),
    });
  assert.equal(
    callbacks((await run('hover-pre-checks-the-folded-annotation')).trace, 'onAction').length,
    1,
  );
});
test('form-pipeline lifecycle print preserves the known policy gap while an action-executor print respects M14', async () => {
  const { trace } = await run('reset-recalculates');
  assert.deepEqual(
    callbacks(trace, 'ui.print').map((callback) => callback.args),
    [[{ origin: 'lifecycle', phase: 'user' }]],
  );
  assert.deepEqual(
    callbacks(trace, 'ui.alert').map((callback) => callback.args),
    [['recalculated', { origin: 'lifecycle', phase: 'user', icon: 0 }]],
  );
  assert.deepEqual(
    callbacks(trace, 'onScriptError').map((callback) => callback.args),
    [[{ error: { kind: 'budget', message: 'recalc budget' }, source: null }]],
  );
  const actions = methodResolved('reset-recalculates', trace, 'execute').map(object);
  assert.equal(actions.length, 1);
  assert.equal(actions[0]!.status, 'executed');
  assert.deepEqual(callbacks(trace, 'onScriptDiagnostic'), []);
  assert.deepEqual(actions[0]!.diagnostics, []);
  assert.deepEqual(
    callbacks(trace, 'actionDiagnostic').map((callback) => callback.args),
    [[{ code: 'blocked', action: 'javascript', source: { kind: 'api' } }]],
  );
});
import { definePlugin } from '@embedpdf/core';
import { registerPlugin } from '../src/scenario';
registerPlugin('form-script-intentional-construction-error', {
  factory: () => {
    throw new Error('intended scripting host construction failure');
  },
});
test('ordinary, refused and construction-failed script hosts restore exact console and entropy hooks', async () => {
  const warn = Object.getOwnPropertyDescriptor(console, 'warn'),
    error = Object.getOwnPropertyDescriptor(console, 'error'),
    entropy = Object.getOwnPropertyDescriptor(crypto, 'getRandomValues'),
    prototype = Object.getPrototypeOf(crypto);
  const assertRestored = () => {
    assert.deepEqual(Object.getOwnPropertyDescriptor(console, 'warn'), warn);
    assert.deepEqual(Object.getOwnPropertyDescriptor(console, 'error'), error);
    assert.deepEqual(Object.getOwnPropertyDescriptor(crypto, 'getRandomValues'), entropy);
    assert.equal(Object.getPrototypeOf(crypto), prototype);
  };
  for (const name of [
    'no-sandbox-host',
    'commit-script-form-effects',
    'scripted-activation-reads-the-folded-annotation',
  ]) {
    const first = await runScenario(inputOf(name));
    assertRestored();
    const second = await runScenario(inputOf(name));
    assertRestored();
    assert.deepEqual(second, first);
    await assert.rejects(
      runScenario({
        ...inputOf(name),
        plugins: [
          ...inputOf(name).plugins!,
          { factory: 'form-script-intentional-construction-error' },
        ],
      }),
      { message: 'intended scripting host construction failure' },
    );
    assertRestored();
  }
  const getter = {
    configurable: true,
    enumerable: true,
    get: () => {
      throw new Error('script host warn getter refused');
    },
  };
  try {
    Object.defineProperty(console, 'warn', getter);
    await assert.rejects(runScenario(inputOf('no-sandbox-host')), {
      message: 'script host warn getter refused',
    });
    assert.deepEqual(Object.getOwnPropertyDescriptor(console, 'warn'), {
      ...getter,
      set: undefined,
    });
    assert.deepEqual(Object.getOwnPropertyDescriptor(console, 'error'), error);
  } finally {
    assert.ok(warn);
    Object.defineProperty(console, 'warn', warn);
  }
  assertRestored();
});
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { canonical } from '../src/canonical';
import { suites as core } from '../src/suites/form-core';
import { suites as lifecycle } from '../src/suites/form-core-lifecycle';
import { suites as fill } from '../src/suites/form-fill';
import { suites as design } from '../src/suites/form-design';
test('exact 59-case migration closes every original 220 input against immutable archives and reviewed i/ii manifests', async () => {
  const root = new URL('../', import.meta.url),
    history = new URL('historical/task-12b2b2-iii/', root),
    oldHistory = new URL('historical/task-12b2b2-i/', root);
  const read = async (name: string, url = history) =>
    JSON.parse(await readFile(new URL(name, url), 'utf8')) as unknown;
  const before = array(await read('original-inputs.json')).map(object),
    original = array(await read('original-inputs.json', oldHistory)).map(object),
    baseline = object(await read('baseline.json')),
    account = object(await read('accounting.json')),
    family = object(await read('family.json'));
  const hash = (value: string | Uint8Array) => createHash('sha256').update(value).digest('hex');
  assert.equal(before.length, 59);
  assert.equal(original.length, 220);
  assert.deepEqual(
    [account.originalOwned, account.currentOwned, account.mapped, account.renamed],
    [59, 59, 59, 0],
  );
  assert.deepEqual(account.retired, []);
  assert.deepEqual(account.added, []);
  const whole = [...core, ...lifecycle, ...fill, ...design, ...scripting, ...actions].flatMap(
    (suite) => suite.cases.map((input) => ({ file: suite.file, name: input.name, input })),
  );
  assert.equal(whole.length, 221);
  assert.equal(new Set(whole.map((row) => row.file + ':' + row.name)).size, 221);
  const mappings = array(account.mappings).map(object),
    allMappings = array(family.mappings).map(object);
  assert.equal(mappings.length, 59);
  assert.equal(allMappings.length, 220);
  assert.equal(
    new Set(allMappings.map((row) => object(row.before).file + ':' + object(row.before).name)).size,
    220,
  );
  assert.deepEqual(
    [family.original, family.current, family.mapped, family.renamed],
    [220, 221, 220, 0],
  );
  assert.deepEqual(family.retired, []);
  assert.equal(array(family.added).length, 1);
  for (const mapping of allMappings) {
    const from = object(mapping.before),
      to = object(mapping.after),
      old = original.find((row) => row.file === from.file && row.name === from.name),
      current = whole.find((row) => row.file === to.file && row.name === to.name);
    assert.ok(old && current);
    assert.equal(hash(JSON.stringify(old.input)), from.canonicalInputSha256);
    assert.equal(hash(JSON.stringify(canonical(current.input))), to.canonicalInputSha256);
    assert.equal(mapping.task11InputDrift, false);
  }
  const first = object(await read('accounting.json', oldHistory));
  const second = object(await read('accounting.json', new URL('historical/task-12b2b2-ii/', root)));
  assert.deepEqual(allMappings.slice(0, 101), first.mappings);
  assert.deepEqual(allMappings.slice(101, 161), second.mappings);
  assert.deepEqual(allMappings.slice(161), mappings);
  assert.deepEqual(family.added, first.added);
  for (const mapping of mappings) {
    const from = object(mapping.before);
    assert.deepEqual(
      before.find((row) => row.file === from.file && row.name === from.name),
      original.find((row) => row.file === from.file && row.name === from.name),
    );
    assert.ok(array(mapping.sourceEvidence).length >= 2);
    assert.ok(typeof mapping.reason === 'string' && mapping.reason.length > 30);
  }
  for (const source of array(baseline.sources).map(object)) {
    const name = String(source.path).split('/').at(-1)!;
    const bytes = await readFile(new URL(name + '.txt', history));
    assert.equal(hash(bytes), source.sha256);
    assert.deepEqual(bytes, await readFile(new URL(name + '.txt', oldHistory)));
  }
  const oldBaseline = object(await read('baseline.json', oldHistory));
  for (const source of array(oldBaseline.sources).map(object)) {
    const name = String(source.path).split('/').at(-1)!;
    assert.equal(hash(await readFile(new URL(name + '.txt', oldHistory))), source.sha256);
    if (name === 'form-fixtures.ts')
      assert.equal(hash(await readFile(new URL('src/suites/' + name, root))), source.sha256);
  }
});

import { ActionsToken } from '../../../packages/plugin/actions/src/host-contract';
registerPlugin('form-script-realm-state', {
  factory: (_, env) =>
    definePlugin({
      id: 'form-script-realm-state',
      token: env.token('form-script-realm-state'),
      scope: 'document',
      requires: [ActionsToken],
      create: (ctx) => ({
        api: {
          get: () => {
            const actions = ctx.get(ActionsToken);
            return {
              configured: actions.getSettings().javascript.enabled,
              enabled: actions.isScriptingEnabled(),
              realm: actions.scriptRealm !== undefined,
              detached: actions.createDetachedScriptRealm !== undefined,
            };
          },
          transact: async () => {
            const actions = ctx.get(ActionsToken);
            if (!actions.scriptRealm || !actions.createDetachedScriptRealm) return null;
            const detached = actions.createDetachedScriptRealm({
              doc: ctx.doc,
              document: () => ctx.document(),
              bootSources: async () => [],
            });
            try {
              return {
                normal: await actions.scriptRealm.transaction(
                  async () => 'normal no-run transaction',
                ),
                detached: await detached.transaction(async () => 'detached no-run transaction'),
              };
            } finally {
              detached.dispose();
            }
          },
        },
      }),
    }),
});
test('an omitted sandbox factory preserves the authored switch and creates the actual lazy default realm', async () => {
  const input = inputOf('no-sandbox-host');
  for (const enabled of [true, false])
    for (const sandbox of [null, { builds: [{}] }]) {
      const result = await runScenario({
        ...input,
        plugins: [
          ...input.plugins!.map((plugin) =>
            plugin.factory === 'form-actions-current'
              ? { ...plugin, config: { openSequence: 'off', javascript: { enabled, sandbox } } }
              : plugin,
          ),
          { factory: 'form-script-realm-state' },
        ],
        steps: [
          ...input.steps.slice(0, 2),
          act('isScriptingEnabled', []),
          { op: 'call', target: { token: 'form-script-realm-state', doc: 'a' }, method: 'get' },
          {
            op: 'call',
            target: { token: 'form-script-realm-state', doc: 'a' },
            method: 'transact',
          },
        ],
      });
      assert.deepEqual(result.unmatched, []);
      assert.deepEqual(result.trace[2]!.outcome, { returned: enabled });
      assert.deepEqual(result.trace[3]!.outcome, {
        returned: { configured: enabled, enabled, realm: enabled, detached: enabled },
      });
      assert.deepEqual(
        methodResolvedForProbe(result.trace),
        enabled
          ? [{ normal: 'normal no-run transaction', detached: 'detached no-run transaction' }]
          : [null],
      );
      assert.deepEqual(
        result.trace.flatMap((entry) => entry.callbacks),
        [],
      );
    }
});

const methodResolvedForProbe = (trace: TraceEntry[]) =>
  trace[4]!.settled.filter((item) => 'resolved' in item).map((item) => item.resolved);
test('the lazy-default case explicitly skips form validation and confirms native values without script diagnostics', async () => {
  const input = inputOf('no-sandbox-host'),
    { trace, unmatched } = await run('no-sandbox-host');
  const form = input.plugins!.find((plugin) => plugin.factory === 'form-current');
  assert.ok(form);
  assert.deepEqual(form.config, { validation: 'none' });
  const actions = input.plugins!.find((plugin) => plugin.factory === 'form-actions-current');
  assert.ok(actions);
  assert.deepEqual(object(actions.config).javascript, { enabled: true, sandbox: null });
  assert.deepEqual(trace[2]!.outcome, { returned: true });
  assert.deepEqual(
    methodResolved('no-sandbox-host', trace, 'setValue').map((value) => ({
      status: object(value).status,
      value: object(object(value).field).value,
    })),
    [{ status: 'applied', value: '3' }],
  );
  assert.deepEqual(readReturns('no-sandbox-host', trace), [{ value: '3' }, { value: '2' }]);
  assert.deepEqual(writes(trace), [[write(2, '3')]]);
  assert.deepEqual(unmatched, []);
  assert.deepEqual(
    trace.flatMap((entry) => entry.callbacks),
    [],
  );
  assert.deepEqual(
    trace.flatMap((entry) => entry.reports),
    [],
  );
});
