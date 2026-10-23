import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { suites } from '../src/suites/commands';
import { runScenario } from '../src/scenario';
test('all 76 original command occurrences and current reason comparisons call surviving shipping owners', () => {
  assert.equal(
    suites.reduce((n, s) => n + s.cases.length, 0),
    79,
  );
  assert.doesNotMatch(
    readFileSync(new URL('../src/suites/commands.ts', import.meta.url), 'utf8'),
    /removedUpstream|ctx\.core\(/,
  );
});
test('command registry preserves insertion order and conflict refusal', async () => {
  const s = suites.find((s) => s.kind === 'scenario')!;
  assert.equal(s.kind, 'scenario');
  if (s.kind !== 'scenario') return;
  const result = await runScenario(s.cases.find((c) => c.name === 'registry')!);
  assert.deepEqual(result.unmatched, []);
  assert.deepEqual(result.trace[0].outcome, { returned: ['b', 'a'] });
});
import { canonical, snapshotHooks } from '../src/canonical';
import { createKernel } from '../../../packages/core/main/src/index';
import { createFakeEngine } from '../src/fake-engine';
import { commandsPlugin } from '../../../packages/plugin/commands/src/index';
import { CommandsToken } from '../../../packages/plugin/commands/src/host-contract';
const scenarioSuite = suites.find((s) => s.kind === 'scenario')!;
const scenario = async (name: string) => {
  assert.equal(scenarioSuite.kind, 'scenario');
  if (scenarioSuite.kind !== 'scenario') throw Error();
  return runScenario(scenarioSuite.cases.find((c) => c.name === name)!);
};
const returned = (t: Awaited<ReturnType<typeof scenario>>['trace'][number]): unknown =>
  t.outcome && 'returned' in t.outcome ? t.outcome.returned : undefined;
const record = (v: unknown) => v as Record<string, unknown>;
const ids = (v: unknown) => (v as { id: string }[]).map((x) => x.id);
const resolved = (t: Awaited<ReturnType<typeof scenario>>['trace'][number]) =>
  t.settled.map((s) => ('resolved' in s ? s.resolved : undefined)).filter((x) => x !== undefined);

test('all original equality cases have independently specified boolean answers', () => {
  const suite = suites[0];
  assert.equal(suite.kind, 'function');
  if (suite.kind !== 'function') return;
  const equal = new Set([
    'both-null',
    'equal-values',
    'same-object',
    'icon-absent-on-both',
    'accent-absent-vs-empty',
    'accent-primary-only-both',
    'no-shortcuts-both',
    'categories-are-not-compared',
    'disabled-reason-equal',
  ]);
  const cases = suite.cases.filter((c) => c.fn.startsWith('resolvedCommandsEqual'));
  assert.equal(cases.length, 27);
  for (const c of cases)
    assert.equal(
      (suite.functions[c.fn] as (...args: unknown[]) => unknown)(...c.args),
      equal.has(c.name),
      c.name,
    );
});
test('settings mutations preserve duplicate removal, no-op identity and registered configuration', () => {
  const suite = suites[0];
  if (suite.kind !== 'function') throw Error();
  const expect: Record<string, [string[], boolean]> = {
    'initial-state': [[], true],
    'disable-new': [['a', 'b'], false],
    'disable-into-empty': [['a'], false],
    'disable-existing-is-ignored': [['a', 'b'], true],
    'enable-removes-every-copy': [['b'], false],
    'enable-missing-is-ignored': [['a'], true],
    'enable-last': [[], false],
    'set-replaces': [['b', 'b', 'c'], false],
    'set-equal-is-a-new-state': [['a'], true],
    'set-empty': [[], false],
    'unknown-action-is-ignored': [['a'], true],
  };
  for (const c of suite.cases.filter((c) => c.fn === 'commandSettings')) {
    const result = record((suite.functions[c.fn] as (...args: unknown[]) => unknown)(...c.args));
    assert.deepEqual(record(result.settings).disabledCategories, expect[c.name][0], c.name);
    assert.equal(result.same, expect[c.name][1], c.name);
  }
  for (const c of suite.cases.filter((c) => c.fn === 'commandsPlugin')) {
    const result = record((suite.functions[c.fn] as (...args: unknown[]) => unknown)(...c.args));
    assert.equal(result.id, 'commands');
    assert.equal(result.scope, 'workspace');
    assert.equal(result.hasState, false);
    assert.equal(result.resolvesAnyCapability, true);
    assert.deepEqual(record(result.settings).registered, c.args[0]);
  }
});
test('all ten registration cases use current parsing and retain successful prefix on errors', () => {
  const suite = suites[0];
  if (suite.kind !== 'function') throw Error();
  const expected: Record<string, string[]> = {
    none: [],
    'no-shortcut': ['a'],
    'one-shortcut': ['a'],
    'many-shortcuts': ['a'],
    'empty-shortcut-list': ['a'],
    'empty-shortcut-is-plus': ['a'],
    'insertion-order': ['z', 'm', 'a', '10', '2'],
    'duplicate-throws-and-keeps-the-first': ['a', 'b'],
    'bad-modifier-throws-and-registers-nothing': ['a'],
    'bad-modifier-in-a-string': [],
  };
  for (const c of suite.cases.filter((c) => c.fn === 'registerCurrentCommands')) {
    const r = record((suite.functions[c.fn] as (...args: unknown[]) => unknown)(...c.args));
    assert.deepEqual(ids(r.entries), expected[c.name], c.name);
    assert.equal('error' in r, /duplicate|bad-modifier/.test(c.name), c.name);
  }
});
test('registry removers own their replacement and resetSettings restores configured definitions', async () => {
  const engine = createFakeEngine({}, (v) => v);
  const kernel = createKernel({
    engine: engine.engine,
    plugins: [
      commandsPlugin({ commands: [{ id: 'a', label: 'Original' }], disabledCategories: ['pro'] }),
    ],
  });
  const api = kernel.capability(CommandsToken);
  let changes = 0;
  const off = api.onSettingsChanged(() => changes++);
  const removeA = api.registerCommand({ id: 'a', label: 'First' }, { replace: true });
  const removeB = api.registerCommand({ id: 'a', label: 'Second' }, { replace: true });
  removeA();
  assert.equal(api.getCommand('a')?.label, 'Second');
  removeB();
  assert.equal(api.getCommand('a')?.label, 'Original');
  removeB();
  api.disableCategory('pro');
  assert.equal(changes, 0);
  api.enableCategory('pro');
  assert.equal(changes, 1);
  api.updateSettings({ disabledCommands: ['a'] });
  assert.equal(api.canExecute('a'), false);
  api.resetSettings();
  assert.deepEqual(api.getDisabledCategories(), ['pro']);
  assert.deepEqual(api.getDisabledCommands(), []);
  assert.equal(api.canExecute('a'), true);
  off();
  await kernel.destroy();
});
test('resolution, safe derivations, accents, categories and search produce literal successful values', async () => {
  let r = await scenario('resolve-defaults');
  assert.deepEqual(returned(r.trace[0]), {
    id: 'plain',
    label: 'plain',
    shortcuts: [],
    enabled: true,
    active: false,
    visible: true,
    categories: [],
  });
  assert.equal(returned(r.trace[3]), null);
  r = await scenario('throwing-derivations-fall-back');
  assert.deepEqual(resolved(r.trace[1]), [{ status: 'executed' }]);
  assert.equal(record(returned(r.trace[0])).enabled, true);
  assert.equal(record(returned(r.trace[0])).active, false);
  r = await scenario('icon-accent');
  assert.deepEqual(record(returned(r.trace[0])).iconAccent, {
    primary: '#e5484d',
    secondary: '#ffffff',
  });
  for (const i of [3, 4, 5, 6, 7]) assert.equal(record(returned(r.trace[i])).iconAccent, undefined);
  r = await scenario('categories');
  assert.deepEqual(resolved(r.trace[4]), [{ status: 'rejected', reason: 'hidden' }]);
  assert.deepEqual(returned(r.trace[15]), ['free', 'free']);
  assert.deepEqual(returned(r.trace[18]), []);
  assert.deepEqual(resolved(r.trace[19]), [{ status: 'executed' }]);
  r = await scenario('search');
  assert.deepEqual(
    r.trace.map((t) => ids(returned(t))),
    [
      ['zoom:in', 'Zoom:Out', 'panel:search', 'ümlaut'],
      ['zoom:in', 'Zoom:Out', 'panel:search', 'ümlaut'],
      ['zoom:in'],
      ['zoom:in'],
      [],
      [],
      [],
      ['zoom:in', 'Zoom:Out', 'panel:search'],
      ['ümlaut'],
      [],
      ['ümlaut'],
      [],
      ['panel:search'],
    ],
  );
});
test('labels follow locale and warn once; shell activity and routing are per document', async () => {
  let r = await scenario('i18n-labels');
  assert.equal(record(returned(r.trace[0])).label, 'Zoom In');
  assert.equal(record(returned(r.trace[5])).label, 'Acercar');
  assert.equal(record(returned(r.trace[1])).label, 'missing');
  assert.equal(
    r.trace.flatMap((t) => t.callbacks).filter((c) => c.fn === 'console.warn').length,
    1,
  );
  r = await scenario('shell-derived-active');
  for (const i of [21, 22, 23, 24, 25, 26, 27, 29])
    assert.equal(record(returned(r.trace[i])).active, true);
  for (const i of [28, 30, 31, 32, 33, 35, 36])
    assert.equal(record(returned(r.trace[i])).active, false);
  r = await scenario('execute-routing');
  assert.deepEqual(ids(returned(r.trace[22])), ['thumbs']);
  assert.deepEqual(ids(returned(r.trace[21])), ['thumbs', 'outline', 'print', 'layers', '']);
  r = await scenario('follows-the-active-document');
  assert.deepEqual(
    [4, 6, 8, 10, 12].map((i) => record(returned(r.trace[i])).active),
    [true, false, true, true, false],
  );
});
test('execution guards, pending run, reentrancy, functions and shortcut precedence are meaningful', async () => {
  let r = await scenario('execute-guards-and-run');
  assert.deepEqual(resolved(r.trace[3]), [{ status: 'rejected', reason: 'disabled' }]);
  assert.deepEqual(resolved(r.trace[4]), [{ status: 'rejected', reason: 'hidden' }]);
  assert.deepEqual(r.trace[10].settled, []);
  assert.deepEqual(resolved(r.trace[11]), [{ status: 'executed' }]);
  assert.deepEqual(returned(r.trace[7]), []);
  r = await scenario('registered-functions');
  assert.deepEqual(resolved(r.trace[3]), [{ status: 'executed' }]);
  assert.deepEqual(resolved(r.trace[4]), [{ status: 'rejected', reason: 'disabled' }]);
  assert.equal(r.trace.flatMap((t) => t.callbacks).filter((c) => c.fn === 'live.run').length, 1);
  r = await scenario('match-stroke');
  assert.deepEqual(returned(r.trace[1]), { commandId: 'palette', refusal: null });
  assert.deepEqual(returned(r.trace[13]), { commandId: 'other-k', refusal: null });
  assert.deepEqual(returned(r.trace[16]), { commandId: 'palette', refusal: null });
  assert.equal(returned(r.trace[4]), null);
  r = await scenario('re-entrant-registry');
  assert.deepEqual(ids(returned(r.trace[1])), ['first', 'third']);
  assert.deepEqual(ids(returned(r.trace[3])), ['first', 'third', 'late']);
  assert.deepEqual(
    r.trace[4].callbacks.map((c) => c.fn),
    ['chain.run', 'target.run', 'chain.run:uses'],
  );
  r = await scenario('unregister-behind-a-search');
  assert.deepEqual(returned(r.trace[2]), ['setup', 'b', 'c', 'd']);
  assert.deepEqual(ids(returned(r.trace[3])), ['setup', 'b', 'c', 'd']);
  r = await scenario('register-many-shortcuts-and-can-execute');
  assert.deepEqual(returned(r.trace[16]), ['a', 'b', 'e']);
  assert.deepEqual(returned(r.trace[18]), ['a', 'b']);
  assert.deepEqual(returned(r.trace[19]), [
    { commandId: 'a', shortcut: 'Mod+A' },
    { commandId: 'a', shortcut: 'Ctrl+Shift+A' },
    { commandId: 'b', shortcut: 'Mod+Shift+B' },
  ]);
});
test('events forward literal args, preserve listener siblings and stop after unsubscribe', async () => {
  const r = await scenario('execute-events');
  assert.deepEqual(
    r.trace[7].callbacks.map((c) => c.fn),
    ['go.run', 'executed', 'executed-2'],
  );
  assert.deepEqual(r.trace[7].callbacks[1].args, [
    { commandId: 'go', documentId: 'b', args: { zoom: 2 } },
  ]);
  assert.deepEqual(r.trace[8].callbacks[1].args, [
    { commandId: 'go', documentId: 'b', args: [1, 'two'] },
  ]);
  assert.deepEqual(
    r.trace[10].callbacks.map((c) => c.fn),
    ['fails.run', 'failed'],
  );
  assert.equal(record(record(r.trace[10].callbacks[1].args[0]).error).code, 'operation-failed');
  assert.deepEqual(
    r.trace[12].callbacks.map((c) => c.fn),
    ['off.enabled'],
  );
  assert.deepEqual(r.trace[13].callbacks, []);
  assert.deepEqual(
    r.trace[17].callbacks.map((c) => c.fn),
    ['go.run'],
  );
  assert.deepEqual(
    r.trace[18].callbacks.map((c) => c.fn),
    ['fails.run'],
  );
  assert.equal(r.trace.flatMap((t) => t.reports).length, 4);
});
test('current contexts target explicit, active, pending and absent documents without a core lens', async () => {
  const r = await scenario('command-ctx');
  const contexts = r.trace.flatMap((t) =>
    t.callbacks.filter((c) => c.fn === 'probe.run').map((c) => record(c.args[0]).documentId),
  );
  assert.deepEqual(contexts, [null, 'b', 'a', '', 'zzz', 'slow', 'slow', 'a', 'b']);
  const uses = r.trace.flatMap((t) => t.callbacks.filter((c) => c.fn === 'probe.run:uses'));
  assert.deepEqual(uses[0].args[1], { returned: null });
  assert.deepEqual(uses[1].args[0], { returned: false });
  assert.deepEqual(uses[1].args[10], { returned: 'b' });
  assert.deepEqual(uses[5].args[1], { returned: null });
  assert.equal(
    record(record(record(uses[1].args[5]).threw).$error).message,
    'No capability "nope".',
  );
});
test('late retained references are selected by the actual returned capability and have exact lifetime outcomes', async () => {
  const r = await scenario('after-destroy');
  const methods = record(returned(r.trace[0]));
  const memberNames = [
    'getSettings',
    'updateSettings',
    'resetSettings',
    'onSettingsChanged',
    'registerCommand',
    'registerCommands',
    'hasCommand',
    'getCommand',
    'listCommandIds',
    'resolveCommand',
    'listCommands',
    'searchCommands',
    'listShortcuts',
    'execute',
    'canExecute',
    'getRefusal',
    'getDisabledCategories',
    'isCategoryDisabled',
    'disableCategory',
    'enableCategory',
    'setDisabledCategories',
    'getDisabledCommands',
    'isCommandDisabled',
    'disableCommand',
    'enableCommand',
    'setDisabledCommands',
    'onExecuted',
    'onExecutionFailed',
    'matchStroke',
    'getMenuTarget',
  ];
  assert.deepEqual(Object.keys(methods), memberNames);
  for (const [index, name] of memberNames.entries())
    assert.deepEqual(methods[name], { $ref: index }, name);

  const expected: Record<number, string> = {
    3: 'resolveCommand',
    4: 'execute',
    6: 'listCommandIds',
    7: 'hasCommand',
    8: 'getMenuTarget',
    9: 'matchStroke',
    10: 'registerCommand',
    11: 'listCommandIds',
    12: 'resolveCommand',
    13: 'resolveCommand',
    14: 'execute',
    15: 'execute',
    16: 'searchCommands',
    17: 'getDisabledCategories',
    18: 'isCategoryDisabled',
    19: 'disableCategory',
    20: 'enableCategory',
    21: 'setDisabledCategories',
    23: 'listCommandIds',
  };
  if (scenarioSuite.kind !== 'scenario') throw Error();
  const input = scenarioSuite.cases.find((c) => c.name === 'after-destroy')!;
  for (const [step, method] of Object.entries(expected)) {
    const target = record(record(input.steps[+step]).target);
    assert.equal(target.ref, record(methods[method]).$ref, `${step} selects ${method}`);
  }
  assert.equal(record(returned(r.trace[3])).label, 'Zoom In');
  assert.deepEqual(resolved(r.trace[4]), [{ status: 'executed' }]);
  for (const i of [6, 9, 11, 12, 16, 23])
    assert.deepEqual(r.trace[i].outcome, {
      threw: {
        $error: {
          name: 'Error',
          message: '[kernel] capability("documents") on a destroyed kernel',
        },
      },
    });
  assert.deepEqual(returned(r.trace[7]), true);
  assert.deepEqual(returned(r.trace[8]), { menu: 'file' });
  assert.equal(returned(r.trace[13]), null);
  assert.deepEqual(resolved(r.trace[15]), [{ status: 'rejected', reason: 'not-found' }]);
  assert.deepEqual(returned(r.trace[17]), []);
  assert.deepEqual(returned(r.trace[18]), false);
});
test('permission and setting refusals preserve hidden/disabled distinction and segment boundaries', async () => {
  const r = await scenario('current-settings-permission-refusals');
  assert.deepEqual(resolved(r.trace[4]), [{ status: 'rejected', reason: 'hidden' }]);
  assert.deepEqual(returned(r.trace[5]), { reason: 'not-allowed', message: "Print isn't allowed" });
  const disabled = record(returned(r.trace[7]));
  assert.equal(disabled.visible, true);
  assert.equal(disabled.enabled, false);
  assert.equal(disabled.disabledReason, "Print isn't allowed");
  assert.deepEqual(resolved(r.trace[8]), [{ status: 'rejected', reason: 'disabled' }]);
  assert.deepEqual(returned(r.trace[9]), {
    commandId: 'print',
    refusal: { reason: 'not-allowed', message: "Print isn't allowed" },
  });
  assert.deepEqual(returned(r.trace[10]), {
    reason: 'not-allowed',
    message: "custom isn't allowed",
  });
  assert.deepEqual(returned(r.trace[12]), {
    reason: 'turned-off',
    message: "Print isn't available",
  });
  assert.equal(record(returned(r.trace[14])).visible, false);
  assert.equal(record(returned(r.trace[15])).visible, true);
  assert.deepEqual(returned(r.trace[17]), []);
  assert.deepEqual(returned(r.trace[18]), []);
  assert.deepEqual(
    r.trace
      .flatMap((t) => t.callbacks)
      .filter((c) => c.fn === 'settings.changed')
      .map((c) => record(c.args[0]).changed),
    [
      ['notAllowed'],
      ['disabledCommands'],
      ['disabledCategories'],
      ['disabledCategories', 'disabledCommands', 'notAllowed'],
    ],
  );
});

import { createHash } from 'node:crypto';
const history = new URL('../historical/task-12b2c5/', import.meta.url);
const read = (name: string): unknown => JSON.parse(readFileSync(new URL(name, history), 'utf8'));
const sha = (value: unknown) => createHash('sha256').update(JSON.stringify(value)).digest('hex');
test('all current cases reproduce complete inputs and every diagnostic channel exactly', async () => {
  const saved = read('current-inputs-runtime.json') as {
    file: string;
    name: string;
    input: unknown;
    inputSha256: string;
    outcome?: unknown;
    result?: unknown;
  }[];
  assert.equal(saved.length, 79);
  assert.equal(new Set(saved.map((s) => s.file + '/' + s.name)).size, 79);
  for (const row of saved) {
    const suite = suites.find((s) => s.file === row.file)!;
    const c = suite.cases.find((c) => c.name === row.name)!;
    assert.deepEqual(canonical(c), row.input, row.name + ' input');
    assert.equal(sha(canonical(c)), row.inputSha256, row.name + ' hash');
    if (suite.kind === 'scenario') {
      assert.ok('steps' in c);
      const result = await runScenario(c);
      assert.deepEqual(result, row.result, row.name + ' complete runtime');
      assert.deepEqual(result.unmatched, [], row.name + ' unmatched');
    } else {
      assert.ok('fn' in c);
      assert.deepEqual(
        {
          returned: canonical(
            (suite.functions[c.fn] as (...args: unknown[]) => unknown)(...c.args),
            snapshotHooks,
          ),
        },
        row.outcome,
        row.name + ' complete outcome',
      );
    }
  }
});
test('original definitions, complete occurrence accounting and every prior tooling file remain exact', () => {
  const originals = read('original-inputs.json') as {
    file: string;
    name: string;
    input: unknown;
    inputSha256: string;
  }[];
  const frozenOriginals = {
    'original-inputs.json': 'd463eae83ca65e49a65b88038620885c2f38bed694bb960b40dd177fd2d09dc5',
    'original-inputs-runtime.json':
      'c63f40ac6b94bc433b15e0e7690b3e667249b684ce97d0e83c4e00a294ff8909',
    'original-commands.ts.txt': 'efaf89480e0b0534419d95e2452637fd6c33dc66159f010be69ae35bb4d849f6',
  };
  for (const [name, want] of Object.entries(frozenOriginals))
    assert.equal(
      createHash('sha256')
        .update(readFileSync(new URL(name, history)))
        .digest('hex'),
      want,
      name + ' frozen C authority',
    );
  const accounting = record(read('accounting.json'));
  const mappings = accounting.mappings as {
    file: string;
    name: string;
    originalOccurrence: number;
    originalInputSha256: string;
    currentInputSha256: string;
  }[];
  assert.equal(originals.length, 76);
  assert.equal(mappings.length, 76);
  for (const [i, o] of originals.entries()) {
    assert.equal(sha(o.input), o.inputSha256, o.name);
    assert.equal(mappings[i].originalOccurrence, i);
    assert.equal(mappings[i].originalInputSha256, o.inputSha256);
    assert.equal(mappings[i].name, o.name);
  }
  assert.deepEqual(record(accounting.summary), {
    original: 76,
    current: 79,
    mapped: 76,
    wholeRetirements: 0,
    partialRetirements: 25,
    additions: 3,
    changedInputs: 39,
  });
  const preserved = record(read('preservation.json'));
  for (const p of preserved.files as { path: string; sha256: string }[]) {
    // Later owners cover these four active paths; immutable C5 manifests and all other 433 stay exact.
    if (
      [
        'tooling/plugin-conformance/src/suites/link.ts',
        'tooling/plugin-conformance/src/suites/metadata.ts',
        'tooling/plugin-conformance/test/acrojs-current.test.ts',
        'tooling/plugin-conformance/test/actions-current.test.ts',
      ].includes(p.path)
    )
      continue;
    assert.equal(
      createHash('sha256')
        .update(readFileSync(new URL('../../../' + p.path, import.meta.url)))
        .digest('hex'),
      p.sha256,
      p.path,
    );
  }
  const integrity = read('integrity.json') as Record<string, string>;
  for (const [p, want] of Object.entries(integrity)) {
    assert.equal(
      createHash('sha256')
        .update(readFileSync(new URL(p, history)))
        .digest('hex'),
      want,
      p,
    );
  }
  const evidence = record(read('source-evidence.json'));
  for (const p of evidence.currentOwners as {
    path: string;
    sha256: string;
    definition: string;
  }[]) {
    assert.equal(
      createHash('sha256').update(p.definition).digest('hex'),
      p.sha256,
      p.path + ' archived source',
    );
    assert.equal(
      readFileSync(new URL('../../../' + p.path, import.meta.url), 'utf8'),
      p.definition,
      p.path,
    );
  }
  assert.deepEqual(read('task11-current-drift.json'), []);
});
test('all intentional step, settled, callback and report failures have exact named source outcomes', async () => {
  const expectedThrows: Record<string, Record<number, string>> = {
    registry: {
      5: "duplicate command 'a'",
      6: '[ui-core] unknown modifier "Hyper" in "Hyper+D"',
      8: '[ui-core] unknown modifier "Nope" in "Nope+E"',
    },
    'register-many-shortcuts-and-can-execute': { 11: "duplicate command 'a'" },
    'a-bad-config-shortcut-refuses-the-kernel': {
      0: '[ui-core] unknown modifier "Hyper" in "Hyper+A"',
    },
    'after-destroy': Object.fromEntries(
      [6, 9, 11, 12, 16, 23].map((i) => [
        i,
        '[kernel] capability("documents") on a destroyed kernel',
      ]),
    ),
  };
  const expectedRejections: Record<string, string[]> = {
    'execute-guards-and-run': ['run failed', 'async run failed'],
    'execute-events': ['run failed', 'async run failed', 'run failed'],
    'after-destroy': ['[kernel] capability("documents") on a destroyed kernel'],
  };
  if (scenarioSuite.kind !== 'scenario') throw Error();
  for (const c of scenarioSuite.cases) {
    const r = await runScenario(c);
    const actual: Record<number, string> = {};
    for (const [i, t] of r.trace.entries())
      if (t.outcome && 'threw' in t.outcome)
        actual[i] = String(record(record(t.outcome.threw).$error).message);
    assert.deepEqual(actual, expectedThrows[c.name] ?? {}, c.name + ' step diagnostics');
    const rejected = r.trace
      .flatMap((t) => t.settled)
      .flatMap((s) => ('rejected' in s ? [String(record(record(s.rejected).$error).message)] : []));
    assert.deepEqual(rejected, expectedRejections[c.name] ?? [], c.name + ' settlements');
    const reports = r.trace.flatMap((t) => t.reports);
    assert.deepEqual(
      reports,
      c.name === 'execute-events'
        ? Array.from({ length: 4 }, () => ({
            $error: { name: 'Error', message: 'listener broke' },
          }))
        : [],
      c.name + ' reports',
    );
  }
  const r = await scenario('command-ctx');
  const uses = r.trace.flatMap((t) => t.callbacks.filter((c) => c.fn === 'probe.run:uses'));
  const expected = [
    [
      'Capability "shell" requires an active document.',
      'Capability "shell" requires an active document.',
      'No capability "nope".',
      'Capability "broken" requires an active document.',
      'Capability "shell" requires an active document.',
    ],
    ['No capability "nope".'],
    ['No capability "nope".'],
    ['No capability "nope".'],
    [
      'Capability "shell" unavailable: no document "zzz".',
      'Capability "shell" unavailable: no document "zzz".',
      'No capability "nope".',
      'Capability "broken" unavailable: no document "zzz".',
      'Capability "shell" unavailable: no document "zzz".',
    ],
    [
      'Capability "shell" unavailable: document "slow" is loading.',
      'Capability "shell" unavailable: document "slow" is loading.',
      'No capability "nope".',
      'Capability "broken" unavailable: document "slow" is loading.',
      'Capability "shell" unavailable: document "slow" is loading.',
    ],
    ['No capability "nope".'],
    ['No capability "nope".'],
    ['No capability "nope".'],
  ];
  assert.deepEqual(
    uses.map((c) =>
      c.args.flatMap((a) =>
        'threw' in record(a) ? [record(record(record(a).threw).$error).message] : [],
      ),
    ),
    expected,
  );
});
