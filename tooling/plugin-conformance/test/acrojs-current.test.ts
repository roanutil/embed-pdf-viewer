import assert from 'node:assert/strict';
import { test } from 'node:test';
import { suites } from '../src/suites/acrojs';
import { runScenario } from '../src/scenario';
import { canonical } from '../src/canonical';
const pure = suites.find((s) => s.kind === 'function')!;
const live = suites.find((s) => s.kind === 'scenario')!;
test('script host constructs and boots exactly once across transactions', async () => {
  assert.equal(live.kind, 'scenario');
  if (live.kind !== 'scenario') return;
  const result = await runScenario(
    live.cases.find((c) => c.name === 'boots-once-and-hands-the-output-to-one-caller')!,
  );
  assert.deepEqual(result.unmatched, []);
  assert.equal(
    result.trace.some((t) => t.status === 'unconstructed'),
    false,
  );
  const callbacks = result.trace.flatMap((t) => t.callbacks);
  assert.equal(callbacks.filter((c) => c.fn === 'sandbox.factory').length, 1);
  assert.equal(callbacks.filter((c) => c.fn === 'sandbox.boot').length, 1);
  assert.equal(callbacks.filter((c) => c.fn === 'sandbox.run').length, 1);
});
test('current snapshot has complete fields and script identity uses current security claims', () => {
  assert.equal(pure.kind, 'function');
  if (pure.kind !== 'function') return;
  const invoke = (name: string) => {
    const input = pure.cases.find((c) => c.name === name)!;
    return canonical((pure.functions[input.fn] as (...args: unknown[]) => unknown)(...input.args));
  };
  assert.deepEqual(invoke('fields-empty'), []);
  const fields = invoke('fields-from-snapshot');
  assert.ok(Array.isArray(fields));
  assert.equal(fields.length, 6);
  assert.deepEqual(invoke('identity-from-claims'), {
    name: 'Ada',
    loginName: 'u-1',
    corporation: 'g-1',
    email: '',
  });
});

test('kernel destroy releases a live realm and deferred disposal releases the eventual sandbox', async () => {
  assert.equal(live.kind, 'scenario');
  if (live.kind !== 'scenario') return;
  for (const name of ['kernel-destroy-owns-host', 'dispose-pending-factory']) {
    const input = live.cases.find((c) => c.name === name);
    assert.ok(input, name);
    const result = await runScenario(input);
    assert.deepEqual(result.unmatched, []);
    assert.deepEqual(
      result.trace
        .flatMap((t) => t.callbacks)
        .filter((c) => c.fn === 'sandbox.dispose')
        .map((c) => c.args),
      name === 'dispose-pending-factory' ? [[0], [1]] : [[0]],
    );
  }
});

import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { canonicalError, snapshotHooks } from '../src/canonical';
import type { TraceEntry } from '../src/scenario';
import { createScriptHost } from '../../../packages/core/acrojs/src/host';
const history = new URL('../historical/task-12b2c1/', import.meta.url);
const read = (name: string) => JSON.parse(readFileSync(new URL(name, history), 'utf8'));
const object = (value: unknown): Record<string, unknown> => {
  assert.ok(value !== null && typeof value === 'object' && !Array.isArray(value));
  return value as Record<string, unknown>;
};
const decode = (value: unknown): unknown => {
  if (Array.isArray(value)) return value.map(decode);
  if (value === null || typeof value !== 'object') return value;
  const row = object(value);
  if ('$number' in row) return Number(row.$number);
  if ('$undefined' in row) return undefined;
  return Object.fromEntries(Object.entries(row).map(([k, v]) => [k, decode(v)]));
};
const empty = {
  event: { rc: true, value: null, change: '', selStart: 0, selEnd: 0 },
  formEffects: [],
  annotEffects: [],
  uiEffects: [],
  diagnostics: [],
};
const diagnostic = (message: string) => ({
  ...empty,
  diagnostics: [{ code: 'script-error', message }],
});
const error = (message: string) => ({ $error: { name: 'Error', message } });
const resolved = (...values: unknown[]) => values.map((value) => ({ resolved: value }));
const rejected = (message: string) => ({ rejected: error(message) });
const output = {
  event: { rc: false, value: 'edited', change: 'x', selStart: 1, selEnd: 2 },
  formEffects: [{ kind: 'setValue', ref: { kind: 'fqn', name: 't' }, value: { value: 'edited' } }],
  annotEffects: [],
  uiEffects: [{ kind: 'print' }],
  diagnostics: [{ code: 'script-error', message: 'script refusal' }],
};
/** Literal expectations cover every settlement/diagnostic and callback order. */
const expected: Record<string, { settlements: unknown[]; calls: string[] }> = {
  'kernel-destroy-owns-host': {
    settlements: resolved([empty], { $undefined: true }),
    calls: ['factory:0', 'sources', 'run:0:owned()', 'dispose:0'],
  },
  'dispose-pending-factory': {
    settlements: resolved([empty]),
    calls: ['factory:0', 'sources', 'dispose:0', 'factory:1', 'run:1:admitted()', 'dispose:1'],
  },
  'sandbox-output-is-returned': {
    settlements: resolved([output]),
    calls: ['factory:0', 'sources', 'run:0:output()'],
  },
  'boots-once-and-hands-the-output-to-one-caller': {
    settlements: resolved([diagnostic('boot#0'), empty], [null]),
    calls: ['factory:0', 'sources', 'boot:0', 'run:0:a()'],
  },
  'run-auto-boots-and-stashes-the-output': {
    settlements: resolved([empty], [empty, null]),
    calls: ['factory:0', 'sources', 'boot:0', 'run:0:a()'],
  },
  'transactions-serialize': {
    settlements: [
      ...resolved(['slow', empty], [empty]),
      rejected('body broke'),
      ...resolved([empty]),
    ],
    calls: ['factory:0', 'sources', 'run:0:slow()', 'run:0:fast()', 'run:0:after()'],
  },
  'no-boot-sources': {
    settlements: resolved([null, empty, null]),
    calls: ['factory:0', 'sources', 'run:0:a()'],
  },
  'boot-sources-read-fails': {
    settlements: resolved(
      [
        diagnostic('Document boot script failed (continuing without it): catalog unreadable'),
        empty,
      ],
      [empty, empty],
    ),
    calls: ['factory:0', 'sources', 'run:0:a()', 'factory:1', 'sources', 'boot:1', 'run:1:b()'],
  },
  'resource-fault-rebuilds-and-reboots': {
    settlements: resolved([empty, empty], [empty]),
    calls: [
      'factory:0',
      'sources',
      'boot:0',
      'run:0:boom()',
      'factory:1',
      'boot:1',
      'run:1:after()',
    ],
  },
  'boot-fault': {
    settlements: resolved([empty, empty]),
    calls: ['factory:0', 'sources', 'boot:0', 'factory:1', 'boot:1', 'run:1:a()'],
  },
  budgets: {
    settlements: resolved([empty, empty, empty]),
    calls: ['factory:0', 'sources', 'boot:0', 'run:0:a()', 'run:0:b()'],
  },
  'input-shape': {
    settlements: resolved([empty, empty]),
    calls: ['factory:0', 'sources', 'run:0:a()', 'run:0:b()'],
  },
  'sandbox-run-throws': {
    settlements: [rejected('vm exploded'), ...resolved([empty])],
    calls: ['factory:0', 'sources', 'run:0:a()', 'run:0:b()'],
  },
  'sandbox-boot-throws': {
    settlements: [rejected('boot exploded'), ...resolved([null, empty])],
    calls: ['factory:0', 'sources', 'boot:0', 'run:0:a()'],
  },
  'boot-sources-throw': {
    settlements: [rejected('no catalog'), ...resolved([empty])],
    calls: ['factory:0', 'sources', 'run:0:a()'],
  },
  'factory-rejects': { settlements: [rejected('no vm'), rejected('no vm')], calls: ['factory:0'] },
  'factory-deferred': {
    settlements: resolved([empty, empty]),
    calls: ['factory:0', 'sources', 'run:0:a()', 'run:0:b()'],
  },
  dispose: {
    settlements: [...resolved([empty]), rejected('script host is disposed')],
    calls: ['factory:0', 'sources', 'run:0:a()', 'dispose:0'],
  },
  'dispose-before-any-realm': { settlements: [rejected('script host is disposed')], calls: [] },
  'dispose-with-a-rejected-factory': { settlements: [rejected('no vm')], calls: ['factory:0'] },
};
const cache = new Map<string, Promise<{ trace: TraceEntry[]; unmatched: string[] }>>();
const run = (name: string) => {
  assert.equal(live.kind, 'scenario');
  if (live.kind !== 'scenario') throw new Error('expected host suite');
  const input = live.cases.find((c) => c.name === name);
  assert.ok(input, name);
  let result = cache.get(name);
  if (!result) {
    result = runScenario(input);
    cache.set(name, result);
  }
  return result;
};
test('all host scenarios have exact settlements and no unexpected diagnostic channels', async () => {
  assert.equal(live.kind, 'scenario');
  if (live.kind !== 'scenario') return;
  assert.equal(live.cases.length, 20);
  assert.deepEqual(live.cases.map((c) => c.name).sort(), Object.keys(expected).sort());
  for (const input of live.cases) {
    const result = await run(input.name);
    assert.deepEqual(result.unmatched, [], input.name);
    assert.equal(result.trace.length, input.steps.length, input.name);
    assert.ok(
      result.trace.every((t) => t.status !== 'unconstructed'),
      input.name,
    );
    assert.deepEqual(
      result.trace.flatMap((t) => t.reports),
      [],
      input.name,
    );
    assert.deepEqual(
      result.trace.flatMap((t) => t.engine),
      [],
      input.name,
    );
    assert.deepEqual(
      result.trace.flatMap((t) => t.stateChanges),
      [],
      input.name,
    );
    assert.deepEqual(
      result.trace.flatMap((t) => t.documentEvents),
      [],
      input.name,
    );
    assert.deepEqual(
      result.trace.flatMap((t) => (t.outcome && 'threw' in t.outcome ? [t.outcome] : [])),
      [],
      input.name,
    );
    assert.deepEqual(
      result.trace
        .flatMap((t) => (t.outcome && 'returned' in t.outcome ? [t.outcome.returned] : []))
        .filter((v) => JSON.stringify(v) !== JSON.stringify({ $undefined: true })),
      [],
      input.name,
    );
    const settlements = result.trace.flatMap((t) => t.settled);
    assert.deepEqual(settlements[0], { promise: 0, resolved: { $undefined: true } }, input.name);
    assert.deepEqual(
      settlements.slice(1).map(({ promise, ...rest }) => rest),
      expected[input.name]!.settlements,
      input.name,
    );
    const ids = result.trace.flatMap((t) =>
      t.outcome && 'promise' in t.outcome ? [t.outcome.promise] : [],
    );
    assert.deepEqual(
      settlements.map((s) => s.promise).sort(),
      ids.sort(),
      input.name + ' no dangling promises',
    );
    const calls = result.trace
      .flatMap((t) => t.callbacks)
      .map((c) => {
        if (c.fn === 'sandbox.factory') return 'factory:' + c.args[0];
        if (c.fn === 'bootSources') {
          assert.deepEqual(c.args, []);
          return 'sources';
        }
        if (c.fn === 'sandbox.boot') return 'boot:' + c.args[0];
        if (c.fn === 'sandbox.run') return 'run:' + c.args[0] + ':' + c.args[1];
        assert.equal(c.fn, 'sandbox.dispose', input.name);
        return 'dispose:' + c.args[0];
      });
    assert.deepEqual(calls, expected[input.name]!.calls, input.name);
  }
});
test('mutex and deferred factory keep callbacks pending until the named settlement', async () => {
  for (const [name, index] of [
    ['transactions-serialize', 5],
    ['factory-deferred', 2],
  ] as const) {
    const { trace } = await run(name);
    assert.deepEqual(
      trace.slice(1, index).flatMap((t) => t.settled),
      [],
    );
    assert.equal(
      trace
        .slice(1, index)
        .flatMap((t) => t.callbacks)
        .filter((c) => c.fn === 'sandbox.run').length,
      0,
    );
    assert.ok(trace[index]!.callbacks.some((c) => c.fn === 'sandbox.run'));
  }
});
test('logged sandbox inputs pin environment sequence, boot event, budgets and optional world planes', async () => {
  const defaults = {
    maxExecutionMs: 50,
    maxMemoryBytes: 16777216,
    maxStackBytes: 524288,
    maxEffects: 256,
    maxOutputBytes: 1048576,
  };
  const document = { id: 'doc-1', fileName: 'x.pdf', pageCount: 1, pageNumber: 0 };
  const identity = { name: '', loginName: '', corporation: '', email: '' };
  const env = (randomSeed: number) => ({ nowMs: 1000, utcOffsetMinutes: 0, randomSeed });
  const basic = (event: unknown, seed: number) => ({
    document,
    identity,
    environment: env(seed),
    fields: [],
    event,
  });
  const first = (await run('boots-once-and-hands-the-output-to-one-caller')).trace.flatMap(
    (t) => t.callbacks,
  );
  assert.deepEqual(first.find((c) => c.fn === 'sandbox.boot')!.args, [
    0,
    ['boot()'],
    basic({ kind: 'name-tree-boot' }, 1983485049),
    defaults,
  ]);
  assert.deepEqual(first.find((c) => c.fn === 'sandbox.run')!.args, [
    0,
    'a()',
    basic({ kind: 'widget-activate' }, 1966707430),
    defaults,
  ]);
  const budget = (await run('budgets')).trace
    .flatMap((t) => t.callbacks)
    .filter((c) => c.fn === 'sandbox.boot' || c.fn === 'sandbox.run');
  assert.deepEqual(
    budget.map((c) => c.args[3]),
    [
      { maxExecutionMs: 7, maxMemoryBytes: 1, maxStackBytes: 1, maxEffects: 1, maxOutputBytes: 1 },
      {
        maxExecutionMs: 5,
        maxMemoryBytes: 1024,
        maxStackBytes: 256,
        maxEffects: 2,
        maxOutputBytes: 64,
      },
      { maxExecutionMs: 9, maxMemoryBytes: 2, maxStackBytes: 2, maxEffects: 3, maxOutputBytes: 4 },
    ],
  );
  assert.deepEqual(
    budget.map((c) => object(object(c.args[2]).environment).randomSeed),
    [1983485049, 1966707430, 1949929811],
  );
  const calls = (await run('input-shape')).trace
    .flatMap((t) => t.callbacks)
    .filter((c) => c.fn === 'sandbox.run');
  assert.deepEqual(
    calls.map((c) => c.args[2]),
    [
      {
        document: { id: 'd', fileName: 'f.pdf', pageCount: 4, pageNumber: 3 },
        identity: { name: 'N', loginName: 'L', corporation: 'C', email: 'E' },
        environment: env(1983485049),
        fields: [
          {
            ref: { kind: 'fqn', name: 't' },
            name: 't',
            family: 'text',
            value: 'v',
            defaultValue: null,
            display: 'visible',
            readOnly: false,
            required: true,
            options: [{ label: 'l', value: 'v' }],
          },
        ],
        annots: [],
        annotPages: [0, 2],
        annotsCoverDocument: false,
        event: {
          kind: 'field-keystroke',
          type: 'Field',
          name: 'Keystroke',
          change: 'x',
          selStart: 0,
          selEnd: 1,
          willCommit: false,
          modifier: true,
          shift: false,
          value: 'v',
          target: { kind: 'fqn', name: 't' },
          source: { kind: 'fqn', name: 't' },
        },
      },
      {
        document: { id: 'd', fileName: 'f.pdf', pageCount: 4, pageNumber: 1 },
        identity: { name: 'N', loginName: 'L', corporation: 'C', email: 'E' },
        environment: env(1966707430),
        fields: [],
        annotsCoverDocument: true,
        event: { kind: 'field-format' },
      },
    ],
  );
});
test('all pure occurrences preserve outcomes or explicitly migrate current field and identity inputs', () => {
  assert.equal(pure.kind, 'function');
  if (pure.kind !== 'function') return;
  const originals = read('original-pure-outcomes.json') as { name: string; outcome: unknown }[];
  assert.equal(pure.cases.length, 75);
  const changed = new Set([
    'fields-from-snapshot',
    'fields-empty',
    ...pure.cases.filter((c) => c.fn === 'resolveScriptIdentity').map((c) => c.name),
  ]);
  const negatives: string[] = [];
  for (const input of pure.cases) {
    let actual: unknown;
    try {
      actual = {
        returned: canonical(
          (pure.functions[input.fn] as (...args: unknown[]) => unknown)(...input.args.map(decode)),
          snapshotHooks,
        ),
      };
    } catch (e) {
      actual = { threw: canonicalError(e, snapshotHooks) };
      negatives.push(input.name);
    }
    if (!changed.has(input.name))
      assert.deepEqual(actual, originals.find((c) => c.name === input.name)!.outcome, input.name);
  }
  assert.deepEqual(negatives, [
    'sources-incomplete-throws',
    'sources-incomplete-is-truthiness',
    'sources-incomplete-string',
    'program-incomplete-throws',
  ]);
  const invoke = (name: string) => {
    const c = pure.cases.find((c) => c.name === name)!;
    return (pure.functions[c.fn] as (...args: unknown[]) => unknown)(...c.args);
  };
  const fields = invoke('fields-from-snapshot');
  assert.ok(Array.isArray(fields));
  assert.deepEqual(fields, [
    {
      ref: { kind: 'objectNumber', objectNumber: 5 },
      name: 'text',
      family: 'text',
      value: 'hi',
      defaultValue: null,
      display: 'visible',
      readOnly: false,
      required: false,
    },
    {
      ref: { kind: 'objectNumber', objectNumber: 6 },
      name: 'list',
      family: 'listbox',
      value: ['a', 'b'],
      defaultValue: null,
      display: 'visible',
      readOnly: true,
      required: true,
      options: [
        { label: 'A', value: 'a' },
        { label: 'B', value: 'b' },
      ],
    },
    {
      ref: { kind: 'objectNumber', objectNumber: 7 },
      name: 'box',
      family: 'checkbox',
      value: 'Off',
      defaultValue: 'Yes',
      display: 'visible',
      readOnly: false,
      required: false,
      exportValues: ['Yes'],
    },
    {
      ref: { kind: 'fqn', name: 'radio' },
      name: 'radio',
      family: 'radio',
      value: 'One',
      defaultValue: 'Off',
      display: 'visible',
      readOnly: false,
      required: false,
      exportValues: ['One'],
    },
    {
      ref: { kind: 'objectNumber', objectNumber: 9 },
      name: 'toggle-unsupported',
      family: 'checkbox',
      value: 'Off',
      defaultValue: 'Off',
      display: 'visible',
      readOnly: false,
      required: false,
      exportValues: ['Yes'],
    },
    {
      ref: { kind: 'objectNumber', objectNumber: 10 },
      name: 'unsupported',
      family: 'text',
      value: null,
      defaultValue: null,
      display: 'visible',
      readOnly: false,
      required: false,
    },
  ]);
  const identities = [
    { name: 'Ada', loginName: 'u-1', corporation: 'g-1', email: '' },
    { name: '', loginName: '', corporation: '', email: '' },
    { name: 'u-1', loginName: 'u-1', corporation: 'g-1', email: '' },
    { name: 'Grace', loginName: 'u-1', corporation: 'g-1', email: 'g@x.test' },
    { name: 'Ada', loginName: 'root', corporation: 'Acme', email: '' },
    { name: 'Ada', loginName: 'u-1', corporation: 'Fn Inc', email: '' },
  ];
  assert.deepEqual(
    pure.cases.filter((c) => c.fn === 'resolveScriptIdentity').map((c) => invoke(c.name)),
    identities,
  );
});
test('host transactions are plain promises; disposal refuses new admissions without an abort API', async () => {
  const host = createScriptHost({
    sandboxFactory: () => {
      throw new Error('no sandbox expected');
    },
    document: () => ({ id: 'd', fileName: 'x', pageCount: 1, pageNumber: 0 }),
    identity: () => ({ name: '', loginName: '', corporation: '', email: '' }),
    environment: () => ({ nowMs: 0, utcOffsetMinutes: 0, randomSeed: 1 }),
    bootSources: async () => [],
  });
  const promise = host.transaction(async () => 42);
  assert.equal('abort' in promise, false);
  host.dispose();
  assert.equal(await promise, 42);
  await assert.rejects(
    host.transaction(async () => 0),
    { name: 'Error', message: 'script host is disposed' },
  );
});
test('shared scripted exports and complete deferred consumer inputs remain unchanged', async () => {
  const original = readFileSync(new URL('original-acrojs.ts.txt', history), 'utf8');
  const current = readFileSync(new URL('../src/suites/acrojs.ts', import.meta.url), 'utf8');
  const block = (source: string) =>
    source.slice(
      source.indexOf('export interface AnswerSpec'),
      source.indexOf('// ── the script-host driver'),
    );
  assert.equal(block(current), block(original));
  for (const saved of read('deferred-inputs.json') as {
    source: string;
    sourceSha256: string;
    inputs: unknown;
  }[]) {
    const source = readFileSync(
      new URL('../src/suites/' + saved.source + '.ts', import.meta.url),
      'utf8',
    );
    if (saved.source !== 'actions' && saved.source !== 'link') {
      assert.equal(
        createHash('sha256').update(source).digest('hex'),
        saved.sourceSha256,
        saved.source,
      );
    }
    const module = await import('../src/suites/' + saved.source + '.ts');
    // C6 owns links and C2–C4 own actions; stage and legacy stamp stay frozen.
    const deferred = (entry: { file: string }) =>
      ![
        'plugins/link.json',
        'plugins/link-functions.json',
        'plugins/actions.json',
        'plugins/actions-dispatcher.json',
        'plugins/actions-submit.json',
        'plugins/actions-triggers.json',
        'plugins/actions-lifecycle.json',
        'plugins/actions-script.json',
        'plugins/actions-hover.json',
      ].includes(entry.file);
    assert.deepEqual(
      (module.suites ?? [])
        .filter(deferred)
        .map(({ file, kind, cases }: { file: string; kind: string; cases: unknown }) => ({
          file,
          kind,
          cases: canonical(cases),
        })),
      (saved.inputs as { file: string }[]).filter(deferred),
      saved.source,
    );
  }
});

test('every default-world sandbox callback records the complete current input and literal seed sequence', async () => {
  const seeds = [1983485049, 1966707430, 1949929811, 1933152192];
  for (const name of Object.keys(expected).filter((n) => !['input-shape', 'budgets'].includes(n))) {
    const callbacks = (await run(name)).trace.flatMap((t) => t.callbacks);
    let sequence = 0;
    for (const call of callbacks) {
      if (call.fn === 'sandbox.factory' || call.fn === 'sandbox.dispose') {
        assert.equal(call.args.length, 1, name);
        continue;
      }
      if (call.fn === 'bootSources') continue;
      assert.equal(call.args.length, 4, name);
      assert.deepEqual(
        call.args[2],
        {
          document: { id: 'doc-1', fileName: 'x.pdf', pageCount: 1, pageNumber: 0 },
          identity: { name: '', loginName: '', corporation: '', email: '' },
          environment: { nowMs: 1000, utcOffsetMinutes: 0, randomSeed: seeds[sequence++] },
          fields: [],
          event: { kind: call.fn === 'sandbox.boot' ? 'name-tree-boot' : 'widget-activate' },
        },
        name,
      );
      assert.deepEqual(
        call.args[3],
        {
          maxExecutionMs: 50,
          maxMemoryBytes: 16777216,
          maxStackBytes: 524288,
          maxEffects: 256,
          maxOutputBytes: 1048576,
        },
        name,
      );
      if (call.fn === 'sandbox.boot')
        assert.deepEqual(
          call.args[1],
          name === 'boot-sources-read-fails' ? ['retry()'] : ['boot()'],
          name,
        );
    }
  }
});
