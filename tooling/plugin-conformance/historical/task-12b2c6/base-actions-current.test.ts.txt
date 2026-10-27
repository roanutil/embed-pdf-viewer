import assert from 'node:assert/strict';
import { test } from 'node:test';
import { suites } from '../src/suites/actions';
import { runScenario } from '../src/scenario';

test('current dispatcher executes document work before deferred navigation and URI callbacks', async () => {
  const suite = suites.find((suite) => suite.file === 'plugins/actions-dispatcher.json')!;
  assert.equal(suite.kind, 'scenario');
  if (suite.kind !== 'scenario') return;
  const result = await runScenario(
    suite.cases.find((entry) => entry.name === 'defers-navigation-and-external-effects')!,
  );
  assert.deepEqual(result.unmatched, []);
  assert.deepEqual(
    result.trace.flatMap((step) => (step.outcome && 'threw' in step.outcome ? [step.outcome] : [])),
    [],
  );
  assert.deepEqual(
    result.trace.flatMap((step) => step.callbacks).map((call) => call.fn),
    ['exec.javascript', 'exec.goto', 'ui.openUri'],
  );
  const calls = result.trace.flatMap((step) => step.callbacks);
  assert.deepEqual(object(calls[1]!.args[0]).destination, {
    kind: 'fit',
    page: { kind: 'objectNumber', objectNumber: 3 },
  });
  assert.deepEqual(calls[1]!.args[1], {
    origin: 'user',
    source: { kind: 'api' },
    event: { scope: 'activate' },
  });
  assert.deepEqual(calls[2]!.args, ['https://a.test/', { isMap: true, origin: 'user' }]);
  assert.deepEqual(result.trace.flatMap((step) => step.settled).at(-1)?.resolved, {
    status: 'executed',
    nodes: [
      { path: [], type: 'goto', status: 'executed' },
      { path: [0], type: 'javascript', status: 'executed' },
      { path: [0, 0], type: 'uri', status: 'executed' },
    ],
    diagnostics: [],
  });
});

test('current tree reads return catalog, field, page and annotation trees through current services', async () => {
  const suite = suites.find((suite) => suite.file === 'plugins/actions-dispatcher.json')!;
  if (suite.kind !== 'scenario') throw new Error('missing dispatcher suite');
  const result = await runScenario(suite.cases.find((entry) => entry.name === 'get-action-tree')!);
  assert.deepEqual(result.unmatched, []);
  assert.deepEqual(
    result.trace.flatMap((step) => step.settled).filter((entry) => 'rejected' in entry),
    [],
  );
  const resolved = result.trace.flatMap((step) => step.settled).map((entry) => entry.resolved);
  assert.ok(resolved.some((value) => JSON.stringify(value).includes('OpenAction')));
  assert.ok(resolved.some((value) => JSON.stringify(value).includes('A-5')));
  assert.ok(resolved.some((value) => JSON.stringify(value).includes('k()')));
});

import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { canonical, canonicalError, snapshotHooks } from '../src/canonical';
import type { TraceEntry } from '../src/scenario';
import { createCurrentActionsConfig } from '../src/suites/actions-current-fixtures';
const files = ['plugins/actions-dispatcher.json', 'plugins/actions-submit.json'];
const owned = suites.flatMap((suite) =>
  suite.kind === 'scenario' && files.includes(suite.file) ? suite.cases : [],
);
const object = (value: unknown): Record<string, unknown> => {
  assert.ok(value !== null && typeof value === 'object' && !Array.isArray(value));
  return value as Record<string, unknown>;
};
const execute = async (name: string) => {
  const entry = owned.find((entry) => entry.name === name);
  assert.ok(entry, name);
  return runScenario(entry);
};
const values = (trace: readonly TraceEntry[]) =>
  trace.flatMap((step) => step.settled).map((value) => value.resolved);
const actions = (trace: readonly TraceEntry[]) =>
  values(trace).filter((value) => value && typeof value === 'object' && 'nodes' in value);
const node = (type: string, status: string, path: number[] = [], detail?: string) => ({
  path,
  type,
  status,
  ...(detail === undefined ? {} : { detail }),
});
const diag = (code: string, message: string) => ({ code, message });
const result = (status: string, nodes: unknown[], diagnostics: unknown[] = []) => ({
  status,
  nodes,
  diagnostics,
});
const one = (
  status: string,
  type: string,
  state: string,
  diagnostics: unknown[] = [],
  detail?: string,
) => result(status, [node(type, state, [], detail)], diagnostics);
const done = (type: string) => one('executed', type, 'executed');
const noExecutor = (type: string) =>
  diag(
    'no-executor',
    `${type}: no executor registered${type === 'javascript' ? ' (scripting unavailable)' : ''}`,
  );
const noAdapter = (type: string) => diag('no-adapter', `${type}: no UI adapter installed`);
const scriptInert = one('inert', 'javascript', 'inert', [noExecutor('javascript')]);
const noResolver = one(
  'inert',
  'submit-form',
  'blocked',
  [
    diag(
      'no-submit-resolver',
      'submit-form: no dataset resolver registered (form plugin missing?)',
    ),
  ],
  'no dataset resolver',
);
const noAnnotSink = one('inert', 'hide', 'no-executor', [
  diag('no-commit-sink', 'hide: no annotation commit sink registered (annotation plugin absent)'),
]);
const noFormSink = one('inert', 'hide', 'no-executor', [
  diag('no-commit-sink', 'hide: no form commit sink registered (form plugin absent)'),
]);
/** Literal source-derived expectations; runner snapshots are never the expected semantic oracle. */
const expected: Record<string, { results: unknown[]; calls: string[] }> = {
  'incomplete-tree-is-refused': {
    results: [
      result('refused', [], [diag('incomplete-tree', 'refused: the action tree is incomplete')]),
    ],
    calls: ['onDiagnosticReported'],
  },
  'rootless-tree-is-inert': { results: [result('inert', [])], calls: [] },
  'walks-next-in-pdf-order': {
    results: [
      result('executed', [
        node('javascript', 'executed'),
        node('javascript', 'executed', [0]),
        node('javascript', 'executed', [0, 0]),
        node('javascript', 'executed', [1]),
      ]),
    ],
    calls: Array(4).fill('exec.javascript'),
  },
  'executed-then-failed-is-partial': {
    results: [
      result(
        'partial',
        [node('javascript', 'executed'), node('javascript', 'failed', [0], 'second broke')],
        [diag('executor-failed', 'javascript: second broke')],
      ),
    ],
    calls: ['exec.javascript', 'exec.javascript'],
  },
  'defers-navigation-and-external-effects': {
    results: [
      result('executed', [
        node('goto', 'executed'),
        node('javascript', 'executed', [0]),
        node('uri', 'executed', [0, 0]),
      ]),
    ],
    calls: ['exec.javascript', 'exec.goto', 'ui.openUri'],
  },
  'a-document-failure-drops-deferred-work': {
    results: [
      result(
        'partial',
        [
          node('goto', 'skipped'),
          node('javascript', 'failed', [0], 'exploded'),
          node('javascript', 'skipped', [0, 0]),
          node('reset-form', 'skipped', [0, 1]),
          node('hide', 'skipped', [0, 2]),
          node('uri', 'skipped', [0, 3]),
        ],
        [diag('executor-failed', 'javascript: exploded')],
      ),
    ],
    calls: ['exec.javascript', 'onDiagnosticReported'],
  },
  'policy-matrix-defaults': {
    results: [
      one('inert', 'uri', 'blocked', [
        diag('blocked', "uri: policy 'report' for origin 'lifecycle'"),
      ]),
      one('inert', 'submit-form', 'blocked'),
      one('inert', 'named', 'blocked'),
      result(
        'inert',
        [
          node('launch', 'blocked'),
          ...[
            'goto-remote',
            'goto-embedded',
            'sound',
            'movie',
            'import-data',
            'rendition',
            'thread',
            'set-ocg-state',
            'transition',
            'goto-3d-view',
            'unknown',
          ].map((type, index) => node(type, index < 5 ? 'blocked' : 'no-executor', [index])),
        ],
        ['launch', 'goto-remote', 'goto-embedded', 'sound', 'movie', 'import-data'].map((type) =>
          diag('blocked', `${type}: never executable`),
        ),
      ),
      one('inert', 'bogus', 'no-executor'),
    ],
    calls: Array(7).fill('onDiagnosticReported'),
  },
  'a-print-typed-node-uses-the-print-row': {
    results: [one('inert', 'print', 'no-executor', [noExecutor('print')])],
    calls: ['onDiagnosticReported'],
  },
  'policy-overrides': {
    results: [
      one('inert', 'uri', 'blocked'),
      one('inert', 'uri', 'blocked', [diag('blocked', "uri: policy 'report' for origin 'hover'")]),
      done('uri'),
      one('inert', 'goto', 'blocked', [diag('blocked', "goto: policy 'report' for origin 'user'")]),
    ],
    calls: ['onDiagnosticReported', 'ui.openUri', 'onDiagnosticReported'],
  },
  'policy-rows-merge-and-update-live': {
    results: [done('uri'), one('inert', 'uri', 'blocked'), one('inert', 'goto', 'blocked')],
    calls: ['ui.openUri'],
  },
  'execute-named': {
    results: [
      done('named'),
      done('named'),
      one('inert', 'named', 'no-executor', [noAdapter('print')], 'no UI adapter installed'),
      done('named'),
      one('inert', 'named', 'blocked'),
    ],
    calls: [
      'exec.named',
      'onExecuted',
      'exec.named',
      'onExecuted',
      'onDiagnosticReported',
      'onExecuted',
      'ui.print',
      'onExecuted',
      'onExecuted',
    ],
  },
  'get-action-tree': { results: [], calls: [] },
  'uri-without-an-adapter': {
    results: [one('inert', 'uri', 'no-executor', [noAdapter('uri')])],
    calls: ['onDiagnosticReported'],
  },
  'named-print-goes-to-the-adapter': {
    results: [result('executed', [node('named', 'executed'), node('named', 'executed', [0])])],
    calls: ['ui.print', 'exec.named'],
  },
  'print-refusals': {
    results: [
      one('inert', 'named', 'no-executor', [noAdapter('print')], 'no UI adapter installed'),
      one(
        'inert',
        'named',
        'blocked',
        [diag('blocked', 'print: doc.print is not allowed')],
        'doc.print is not allowed',
      ),
    ],
    calls: ['b.onDiagnostic', 'onDiagnosticReported'],
  },
  'hide-routes-by-plane': {
    results: [done('hide'), done('hide')],
    calls: ['sink.form', 'sink.annot', 'sink.form'],
  },
  'hide-and-later-document-work': {
    results: [
      result('executed', [node('hide', 'executed'), node('reset-form', 'executed', [0])]),
      result(
        'partial',
        [node('hide', 'failed', [], 'refused'), node('reset-form', 'skipped', [0])],
        [diag('executor-failed', 'hide: annotation 7: refused')],
      ),
    ],
    calls: ['sink.annot', 'exec.reset-form', 'sink.annot'],
  },
  'hide-without-targets': { results: [done('hide')], calls: [] },
  'hide-reports-honestly': {
    results: [
      noAnnotSink,
      noFormSink,
      one('executed', 'hide', 'executed', [
        diag('unresolved-target', "hide: no field named 'ghost'"),
      ]),
      one(
        'partial',
        'hide',
        'failed',
        [diag('executor-failed', 'hide: annotation 7: PermissionDenied: doc.annotate.modify')],
        'PermissionDenied: doc.annotate.modify',
      ),
      one('executed', 'hide', 'executed', [
        diag('executor-failed', 'hide: annotation 7: first refused'),
      ]),
      one('executed', 'hide', 'executed', [
        diag('executor-failed', 'hide: annotation 7: failed'),
        diag('executor-failed', 'hide: annotation 8: second'),
      ]),
      done('hide'),
    ],
    calls: [
      'onDiagnosticReported',
      'onDiagnosticReported',
      'onDiagnosticReported',
      'sink.annot',
      'onDiagnosticReported',
      'sink.annot',
      'onDiagnosticReported',
      'sink.annot',
      'onDiagnosticReported',
      'onDiagnosticReported',
      'sink.annot',
    ],
  },
  'hide-form-failures': {
    results: [
      one(
        'partial',
        'hide',
        'failed',
        [diag('executor-failed', 'hide: form refused')],
        'form refused',
      ),
      one('partial', 'hide', 'failed', [diag('executor-failed', 'hide: failed')], 'failed'),
      done('hide'),
    ],
    calls: ['sink.form', 'onDiagnosticReported', 'sink.form', 'onDiagnosticReported', 'sink.form'],
  },
  'hide-sink-throws': { results: [done('hide')], calls: ['sink.annot'] },
  'hide-forms-read-rejects': { results: [], calls: [] },
  'document-executors': {
    results: [
      one('inert', 'reset-form', 'no-executor', [noExecutor('reset-form')]),
      scriptInert,
      one(
        'inert',
        'reset-form',
        'inert',
        [diag('executor-inert', 'reset-form: nothing to reset')],
        'nothing to reset',
      ),
      ...['reset refused', 'reset threw', 'reset rejected'].map((message) =>
        one(
          'partial',
          'reset-form',
          'failed',
          [diag('executor-failed', `reset-form: ${message}`)],
          message,
        ),
      ),
      done('reset-form'),
      done('reset-form'),
    ],
    calls: [
      'onDiagnosticReported',
      'onDiagnosticReported',
      'exec.reset-form',
      'onDiagnosticReported',
      'exec.reset-form',
      'onDiagnosticReported',
      'exec.reset-form',
      'onDiagnosticReported',
      'exec.reset-form',
      'onDiagnosticReported',
      'exec.reset-form',
      'exec.reset-form',
    ],
  },
  'navigation-executors': {
    results: [
      result(
        'inert',
        [node('goto', 'no-executor'), node('named', 'no-executor', [0])],
        [noExecutor('goto'), noExecutor('named')],
      ),
      one('inert', 'goto', 'inert', [diag('executor-inert', 'goto: no stage')], 'no stage'),
      ...['goto threw', 'goto rejected'].map((message) =>
        one('partial', 'goto', 'failed', [diag('executor-failed', `goto: ${message}`)], message),
      ),
      done('goto'),
    ],
    calls: [
      'onDiagnosticReported',
      'onDiagnosticReported',
      'exec.goto',
      'onDiagnosticReported',
      'exec.goto',
      'onDiagnosticReported',
      'exec.goto',
      'onDiagnosticReported',
      'exec.goto',
    ],
  },
  'executors-are-last-wins-and-identity-safe': {
    results: [done('javascript'), scriptInert],
    calls: ['onDiagnosticReported', 'second', 'onDiagnosticReported'],
  },
  'adapter-is-identity-safe': {
    results: [
      done('uri'),
      one('inert', 'uri', 'no-executor', [noAdapter('uri')]),
      one('inert', 'uri', 'no-executor', [noAdapter('uri')]),
    ],
    calls: ['second.openUri'],
  },
  'sinks-and-resolver-are-identity-safe': {
    results: [done('hide'), noAnnotSink, done('submit-form'), done('submit-form'), noResolver],
    calls: [
      'annot.second',
      'onDiagnosticReported',
      'resolver',
      'handler.second',
      'resolver',
      'onDiagnosticReported',
    ],
  },
  'dispatches-serialize': {
    results: [done('javascript'), done('javascript')],
    calls: ['exec.javascript', 'exec.javascript'],
  },
  'hooks-deliver-and-isolate': {
    results: [scriptInert, scriptInert],
    calls: [
      'onDiagnostic.bad',
      'onDiagnosticReported',
      'onAction.bad',
      'onAction',
      'onDiagnostic.bad',
      'onAction.bad',
    ],
  },
  'a-user-execute-arms-the-open-sequence': {
    results: [done('named'), done('named'), done('named')],
    calls: Array(4).fill('exec.named'),
  },
  'payload-less-submit-is-inert': {
    results: [
      one(
        'inert',
        'submit-form',
        'inert',
        [
          diag(
            'submit-payload-unavailable',
            'submit-form: payload unavailable (older runtime extraction) — node inert',
          ),
        ],
        'submit payload unavailable (older runtime extraction)',
      ),
    ],
    calls: ['onDiagnosticReported'],
  },
  'no-resolver': { results: [noResolver], calls: ['onDiagnosticReported'] },
  'resolver-failures': {
    results: ['no fields', 'field plane gone'].map((message) =>
      one(
        'partial',
        'submit-form',
        'failed',
        [diag('executor-failed', `submit-form: dataset resolution: ${message}`)],
        message,
      ),
    ),
    calls: ['resolver', 'onDiagnosticReported', 'resolver', 'onDiagnosticReported'],
  },
  'no-sink': {
    results: [
      one(
        'inert',
        'submit-form',
        'blocked',
        [
          diag(
            'no-submit-sink',
            'submit-form: no handler installed and the document has no submit-capable home',
          ),
        ],
        'no submit sink',
      ),
    ],
    calls: ['resolver', 'onDiagnosticReported'],
  },
  'the-document-home': {
    results: [
      done('submit-form'),
      one(
        'partial',
        'submit-form',
        'failed',
        [
          diag(
            'executor-failed',
            'document home refused submit: permission denied: doc.forms.submit',
          ),
        ],
        'permission denied: doc.forms.submit',
      ),
    ],
    calls: ['resolver', 'resolver', 'onDiagnosticReported'],
  },
  'the-home-uses-the-configured-clock': {
    results: [done('submit-form')],
    calls: ['resolver', 'now'],
  },
  'a-handler-beats-the-home': {
    results: [done('submit-form'), done('submit-form')],
    calls: ['resolver', 'handler', 'resolver', 'handler.b'],
  },
  'handler-contract': {
    results: [
      one(
        'partial',
        'submit-form',
        'failed',
        [diag('executor-failed', 'submit handler threw: sync refuse')],
        'sync refuse',
      ),
      done('submit-form'),
      done('submit-form'),
    ],
    calls: [
      'resolver',
      'handler',
      'onDiagnosticReported',
      'resolver',
      'handler',
      'resolver',
      'handler',
      'onDiagnosticReported',
    ],
  },
  'submit-policy': {
    results: [
      one('inert', 'submit-form', 'blocked', [
        diag('blocked', "submit-form: policy 'report' for origin 'user'"),
      ]),
      done('submit-form'),
      done('submit-form'),
    ],
    calls: ['onDiagnosticReported', 'resolver', 'handler', 'resolver', 'handler'],
  },
  'submit-is-deferred-behind-document-work': {
    results: [
      result(
        'partial',
        [node('submit-form', 'skipped'), node('javascript', 'failed', [0], 'x')],
        [diag('executor-failed', 'javascript: x')],
      ),
    ],
    calls: ['exec.javascript'],
  },
};
const error = (message: string) => ({ $error: { name: 'Error', message } });
const rejections: Record<string, unknown[]> = {
  'hide-sink-throws': [error('sink broke')],
  'hide-forms-read-rejects': [
    {
      $error: {
        name: 'PluginError',
        message: 'closed',
        code: 'operation-failed',
        capability: 'actions',
        permission: null,
        cause: { $error: { name: 'EngineError', message: 'closed', code: 'DocClosed' } },
      },
    },
  ],
};

test('every owned dispatch and submit result, observer report and callback order has an exact semantic expectation', async () => {
  assert.deepEqual(owned.map((entry) => entry.name).sort(), Object.keys(expected).sort());
  for (const entry of owned) {
    const { trace, unmatched } = await runScenario(entry);
    assert.deepEqual(unmatched, [], entry.name);
    assert.equal(trace.length, entry.steps.length, entry.name);
    assert.deepEqual(
      trace.filter(
        (step) => step.status === 'unconstructed' || (step.outcome && 'threw' in step.outcome),
      ),
      [],
      entry.name,
    );
    assert.deepEqual(actions(trace), expected[entry.name]!.results, entry.name);
    const booleans: Record<string, boolean[]> = {
      'incomplete-tree-is-refused': [false],
      'rootless-tree-is-inert': [false],
      'policy-matrix-defaults': [false, true, true, false, false, true],
      'a-print-typed-node-uses-the-print-row': [true],
      'policy-overrides': [true],
      'policy-rows-merge-and-update-live': [false],
    };
    assert.deepEqual(
      trace.flatMap((step) =>
        step.outcome && 'returned' in step.outcome && typeof step.outcome.returned === 'boolean'
          ? [step.outcome.returned]
          : [],
      ),
      booleans[entry.name] ?? [],
      entry.name + ' canExecute',
    );

    assert.deepEqual(
      trace.flatMap((step) => step.callbacks).map((call) => call.fn),
      expected[entry.name]!.calls,
      entry.name,
    );
    assert.deepEqual(
      trace
        .flatMap((step) => step.settled)
        .filter((value) => 'rejected' in value)
        .map((value) => value.rejected),
      rejections[entry.name] ?? [],
      entry.name,
    );
    assert.deepEqual(
      trace.flatMap((step) => step.reports),
      entry.name === 'hooks-deliver-and-isolate'
        ? [
            error('diag observer broke'),
            error('observer broke'),
            error('diag observer broke'),
            error('observer broke'),
          ]
        : [],
      entry.name,
    );
    assert.deepEqual(
      trace.flatMap((step) => step.callbacks).filter((call) => call.fn.startsWith('console.')),
      [],
      entry.name,
    );
    const promised = trace.flatMap((step) =>
      step.outcome && 'promise' in step.outcome ? [step.outcome.promise] : [],
    );
    const settled = trace.flatMap((step) => step.settled).map((value) => value.promise);
    assert.deepEqual(
      settled.slice().sort(),
      promised.slice().sort(),
      entry.name + ' has no dangling promises',
    );
    assert.deepEqual(
      trace.flatMap((step) => step.stateChanges),
      [],
      entry.name + ' stateless',
    );
  }
});

const user = { origin: 'user', source: { kind: 'api' }, event: { scope: 'activate' } };
const namedNode = (name: string) => ({ type: 'named', subtype: 'Named', name, next: [] });
const scriptNode = (script: string) => ({
  type: 'javascript',
  subtype: 'JavaScript',
  script,
  next: [],
});
const treeValue = (root: unknown) => ({ root, incomplete: false, warningFlags: 0, warnings: [] });
const request = {
  url: 'https://home.test/submit',
  method: 'post',
  format: 'fdf',
  flagsRaw: 0,
  entries: [{ name: 'plain', value: 'visible' }],
  origin: 'user',
  event: { scope: 'activate' },
};
const intent = {
  url: 'https://home.test/submit',
  fields: null,
  exclude: false,
  includeNoValueFields: false,
  format: 'fdf',
  method: 'post',
  flagsRaw: 0,
};

test('raw tree resolution uses current refs, returns exact values and memoizes the catalog', async () => {
  const { trace } = await execute('get-action-tree');
  assert.deepEqual(values(trace).slice(3), [
    treeValue(namedNode('A-5')),
    treeValue(namedNode('E-5')),
    treeValue(namedNode('U-5')),
    null,
    null,
    treeValue(scriptNode('k()')),
    treeValue(scriptNode('k()')),
    null,
    null,
    null,
    treeValue(namedNode('O-1')),
    treeValue(namedNode('C-1')),
    null,
    null,
    treeValue(namedNode('OpenAction')),
    treeValue(namedNode('WS')),
    null,
    null,
  ]);
  assert.deepEqual(
    trace.flatMap((step) => step.engine).filter((op) => object(op).path === 'actions.get'),
    [
      { doc: 'a', path: 'actions.get', args: [] },
      { doc: 'b', path: 'actions.get', args: [] },
    ],
  );
  assert.equal(
    trace.flatMap((step) => step.engine).filter((op) => object(op).path === 'forms.list').length,
    10,
  );
  assert.deepEqual(
    trace
      .flatMap((step) => step.engine)
      .filter((op) => object(op).path === 'page.annotations.list'),
    Array.from({ length: 5 }, () => ({
      doc: 'a',
      path: 'page.annotations.list',
      pon: 1,
      args: [],
    })),
  );
});

test('hide resolves fields and widgets through the forms plane before annotation patches', async () => {
  const { trace } = await execute('hide-routes-by-plane');
  assert.deepEqual(
    trace.flatMap((step) => step.callbacks),
    [
      {
        fn: 'sink.form',
        args: [
          [
            {
              kind: 'setDisplay',
              ref: { kind: 'objectNumber', objectNumber: 100 },
              display: 'visible',
            },
            {
              kind: 'setDisplay',
              ref: { kind: 'objectNumber', objectNumber: 101 },
              display: 'visible',
            },
          ],
        ],
      },
      { fn: 'sink.annot', args: [[{ annotObjectNumber: 7, patch: { flags: { hidden: false } } }]] },
      {
        fn: 'sink.form',
        args: [
          [
            {
              kind: 'setDisplay',
              ref: { kind: 'objectNumber', objectNumber: 100 },
              display: 'hidden',
            },
          ],
        ],
      },
    ],
  );
  assert.deepEqual(
    trace.flatMap((step) => step.engine).slice(3),
    Array.from({ length: 2 }, () => ({ doc: 'a', path: 'forms.list', args: [] })),
  );
  const failure = await execute('hide-form-failures');
  assert.equal(
    failure.trace.flatMap((step) => step.callbacks).some((call) => call.fn === 'sink.annot'),
    false,
  );
  assert.deepEqual(
    (await execute('hide-without-targets')).trace.flatMap((step) => step.engine).slice(3),
    [],
  );
});

test('submit normalizes the resolver intent and sends current request fields and ISO clock to the home', async () => {
  for (const [name, sentAt, charSets] of [
    ['the-document-home', '1970-01-01T00:00:01.500Z', [undefined, 'latin1']],
    ['the-home-uses-the-configured-clock', '1970-01-01T00:00:01.234Z', [undefined]],
  ] as const) {
    const { trace } = await execute(name);
    const resolvers = trace
      .flatMap((step) => step.callbacks)
      .filter((call) => call.fn === 'resolver');
    assert.deepEqual(
      resolvers.map((call) => call.args),
      charSets.map(() => [intent, user, { $function: true }]),
    );
    assert.deepEqual(
      trace.flatMap((step) => step.engine).filter((op) => object(op).path === 'forms.submit'),
      charSets.map((charSet) => ({
        doc: 'a',
        path: 'forms.submit',
        args: [
          {
            entries: [{ name: 'plain', value: 'visible' }],
            intent: {
              url: 'https://home.test/submit',
              format: 'fdf',
              method: 'post',
              flagsRaw: 0,
              ...(charSet ? { charSet } : {}),
            },
            origin: 'user',
            sentAt,
          },
        ],
      })),
    );
  }
  const noSink = (await execute('no-sink')).trace
    .flatMap((step) => step.callbacks)
    .find((call) => call.fn === 'resolver')!;
  assert.deepEqual(noSink.args, [
    {
      ...intent,
      fields: [
        { kind: 'name', name: 'a' },
        { kind: 'objectNumber', objectNumber: 9 },
      ],
      charSet: 'utf-8',
    },
    user,
    { $function: true },
  ]);
});

test('handler acceptance bypasses the home, exposes the optional delegate and detached failure stays external', async () => {
  const { trace } = await execute('a-handler-beats-the-home');
  assert.deepEqual(
    trace.flatMap((step) => step.callbacks).filter((call) => call.fn.startsWith('handler')),
    [
      {
        fn: 'handler',
        args: [
          { ...request, entries: [], charSet: 'utf-8' },
          { submitToDocumentHome: { $function: true } },
        ],
      },
      { fn: 'handler.b', args: [request, { submitToDocumentHome: null }] },
    ],
  );
  assert.deepEqual(
    trace.flatMap((step) => step.engine).filter((op) => object(op).path === 'forms.submit'),
    [],
  );
  const detached = await execute('handler-contract');
  assert.deepEqual(
    detached.trace.at(-2)!.settled.map((value) => value.resolved),
    [done('submit-form')],
  );
  assert.deepEqual(detached.trace.at(-1)!.settled, []);
  assert.deepEqual(detached.trace.at(-1)!.callbacks, [
    {
      fn: 'onDiagnosticReported',
      args: [{ code: 'executor-failed', action: 'submit-form', source: { kind: 'api' } }],
    },
  ]);
});

test('queued execution does not run or settle later work before the deferred executor answers', async () => {
  const { trace } = await execute('dispatches-serialize');
  assert.deepEqual(trace[3]!.callbacks, [
    { fn: 'exec.javascript', args: [scriptNode('slow'), user] },
  ]);
  assert.deepEqual(trace[3]!.settled, []);
  assert.deepEqual(trace[4]!.callbacks, []);
  assert.deepEqual(trace[4]!.settled, []);
  assert.deepEqual(trace[5]!.callbacks, [
    { fn: 'exec.javascript', args: [scriptNode('fast'), user] },
  ]);
  assert.deepEqual(trace[5]!.settled, [
    { promise: 2, resolved: done('javascript') },
    { promise: 3, resolved: done('javascript') },
  ]);
  const open = (await execute('a-user-execute-arms-the-open-sequence')).trace.flatMap(
    (step) => step.callbacks,
  );
  assert.deepEqual(
    open.map((call) => object(call.args[0]).name),
    ['Hover', 'OpenAction', 'Click', 'Again'],
  );
  assert.deepEqual(
    open.map((call) => call.args[1]),
    [
      {
        origin: 'hover',
        source: { kind: 'api' },
        event: { scope: 'annotation', name: 'cursorEnter' },
      },
      {
        origin: 'lifecycle',
        source: { kind: 'document' },
        event: { scope: 'document', name: 'open' },
      },
      user,
      user,
    ],
  );
});

test('settings patches retain untouched policy origins and canExecute follows current policy', async () => {
  const { trace } = await execute('policy-rows-merge-and-update-live');
  const settings = [5, 8, 13].map((index) => object(object(trace[index]!.outcome).returned));
  const policies = settings.map((settings) => object(settings.policy));
  assert.deepEqual(
    policies.map((policy) => policy.uri),
    [
      { user: 'adapter', hover: 'allow', lifecycle: 'report' },
      { user: 'block', hover: 'allow', lifecycle: 'report' },
      { user: 'block', hover: 'allow', lifecycle: 'report' },
    ],
  );
  assert.deepEqual(
    policies.map((policy) => policy.goto),
    [
      { user: 'allow', hover: 'allow', lifecycle: 'allow' },
      { user: 'allow', hover: 'allow', lifecycle: 'block' },
      { user: 'allow', hover: 'allow', lifecycle: 'block' },
    ],
  );
  assert.deepEqual(trace[11]!.outcome, { returned: false });
  assert.deepEqual(settings[1], settings[2]);
  assert.deepEqual((await execute('incomplete-tree-is-refused')).trace.at(-1)!.outcome, {
    returned: false,
  });
  assert.deepEqual((await execute('rootless-tree-is-inert')).trace.at(-1)!.outcome, {
    returned: false,
  });
});

test('event listeners report exceptions to the kernel and preserve sibling delivery and unsubscribe identity', async () => {
  const { trace } = await execute('hooks-deliver-and-isolate');
  const payload = {
    tree: treeValue(scriptNode('orphan()')),
    result: scriptInert,
    source: { kind: 'api' },
  };
  assert.deepEqual(
    trace[6]!.callbacks.filter((call) => call.fn.startsWith('onAction')).map((call) => call.args),
    [[payload], [payload]],
  );
  assert.deepEqual(
    trace[9]!.callbacks.filter((call) => call.fn.startsWith('onAction')).map((call) => call.args),
    [[payload]],
  );
  const disposal = await execute('executors-are-last-wins-and-identity-safe');
  assert.deepEqual(
    disposal.trace.flatMap((step) => step.callbacks).filter((call) => call.fn === 'second'),
    [{ fn: 'second', args: [scriptNode('x'), user] }],
  );
});

test('pure trigger descriptors and URL encoding have exact independent results', () => {
  const pure = suites.find((suite) => suite.file === 'plugins/actions.json')!;
  if (pure.kind !== 'function') throw new Error('missing pure suite');
  const events = [
    { scope: 'activate' },
    ...['cursorEnter', 'cursorExit', 'mouseDown', 'mouseUp', 'focus', 'blur'].map((name) => ({
      scope: 'annotation',
      name,
    })),
    ...['open', 'close', 'visible', 'invisible'].map((name) => ({ scope: 'page', name })),
    ...['open', 'will-save', 'did-save', 'will-print', 'did-print', 'will-close'].map((name) => ({
      scope: 'document',
      name,
    })),
  ];
  const origins = [
    'user',
    'hover',
    'hover',
    'user',
    'user',
    'user',
    'user',
    ...Array(10).fill('lifecycle'),
  ];
  const wants = [
    ...events,
    ...origins,
    '',
    "flag&full%20name=Ada%20Lovelace%20%26%20co%3D1&pick%5B%5D=a%2Fb&pick%5B%5D=%C3%A7&pick%5B%5D=%F0%9F%98%80&safe-_.!~*'()=ok",
  ];
  assert.equal(pure.cases.length, wants.length);
  pure.cases.forEach((entry, index) =>
    assert.deepEqual(
      (pure.functions[entry.fn] as (...args: unknown[]) => unknown)(...entry.args),
      wants[index],
      entry.name,
    ),
  );
});

test('current factory preserves actual scripting enablement with null or omitted overrides', () => {
  // This tests config, not the default VM loader: enabled must never silently become false.
  const env = {
    log: () => {},
    defer: () => {
      throw new Error('unused');
    },
    token: () => {
      throw new Error('unused');
    },
  };
  for (const sandbox of [null, undefined]) {
    assert.deepEqual(createCurrentActionsConfig({ javascript: { enabled: true, sandbox } }, env), {
      javascript: { enabled: true },
    });
  }
});

const historical = new URL('../historical/task-12b2c2/', import.meta.url);
const readHistory = (name: string) => JSON.parse(readFileSync(new URL(name, historical), 'utf8'));
test('all 29 link inputs and exact outcomes remain frozen', async () => {
  const { suites: links } = await import('../src/suites/link');
  const available = [...suites, ...links];
  for (const saved of [
    ...readHistory('deferred-runtime.json').filter(
      (entry: { file: string }) =>
        ![
          'plugins/actions-triggers.json',
          'plugins/actions-lifecycle.json',
          'plugins/actions-script.json',
          'plugins/actions-hover.json',
        ].includes(entry.file),
    ),
    ...readHistory('link-runtime.json'),
  ] as {
    file: string;
    name: string;
    input: unknown;
    result?: unknown;
    outcome?: unknown;
    runnerException?: unknown;
  }[]) {
    const suite = available.find((suite) => suite.file === saved.file)!;
    const entry = suite.cases.find((entry) => entry.name === saved.name)!;
    assert.deepEqual(canonical(entry), saved.input, saved.name + ' complete input');
    if (suite.kind === 'scenario') {
      assert.ok('steps' in entry);
      let observed;
      try {
        observed = { result: await runScenario(entry) };
      } catch (error) {
        observed = { runnerException: canonicalError(error, snapshotHooks) };
      }
      assert.deepEqual(
        observed,
        saved.runnerException
          ? { runnerException: saved.runnerException }
          : { result: saved.result },
        saved.name + ' full outcome',
      );
    } else {
      assert.ok('fn' in entry);
      let outcome;
      try {
        outcome = {
          returned: canonical(
            (suite.functions[entry.fn] as (...args: unknown[]) => unknown)(...entry.args),
            snapshotHooks,
          ),
        };
      } catch (error) {
        outcome = { threw: canonicalError(error, snapshotHooks) };
      }
      assert.deepEqual(outcome, saved.outcome, saved.name + ' pure outcome');
    }
  }
});

const event = (code: string, action: string, source: unknown = { kind: 'api' }) => ({
  code,
  action,
  source,
});
const failureEvent = (type: string) => event('executor-failed', type);
const diagnosticEvents: Record<string, unknown[]> = {
  'incomplete-tree-is-refused': [event('incomplete-tree', 'javascript')],
  'a-document-failure-drops-deferred-work': [failureEvent('javascript')],
  'policy-matrix-defaults': [
    'uri',
    'launch',
    'goto-remote',
    'goto-embedded',
    'sound',
    'movie',
    'import-data',
  ].map((type) => event('blocked', type)),
  'a-print-typed-node-uses-the-print-row': [event('no-executor', 'print')],
  'policy-overrides': [event('blocked', 'uri'), event('blocked', 'goto')],
  'execute-named': [event('no-adapter', 'named')],
  'uri-without-an-adapter': [event('no-adapter', 'uri')],
  'print-refusals': [event('no-adapter', 'named'), event('blocked', 'named')],
  'hide-reports-honestly': [
    event('no-commit-sink', 'hide'),
    event('no-commit-sink', 'hide'),
    event('unresolved-target', 'hide'),
    ...Array.from({ length: 4 }, () => failureEvent('hide')),
  ],
  'hide-form-failures': [failureEvent('hide'), failureEvent('hide')],
  'document-executors': [
    event('no-executor', 'reset-form'),
    event('no-executor', 'javascript'),
    event('executor-inert', 'reset-form'),
    ...Array.from({ length: 3 }, () => failureEvent('reset-form')),
  ],
  'navigation-executors': [
    event('no-executor', 'goto'),
    event('no-executor', 'named'),
    event('executor-inert', 'goto'),
    failureEvent('goto'),
    failureEvent('goto'),
  ],
  'executors-are-last-wins-and-identity-safe': [
    event('duplicate-executor', 'javascript', null),
    event('no-executor', 'javascript'),
  ],
  'sinks-and-resolver-are-identity-safe': [
    event('no-commit-sink', 'hide'),
    event('no-submit-resolver', 'submit-form'),
  ],
  'hooks-deliver-and-isolate': Array.from({ length: 3 }, () => event('no-executor', 'javascript')),
  'payload-less-submit-is-inert': [event('submit-payload-unavailable', 'submit-form')],
  'no-resolver': [event('no-submit-resolver', 'submit-form')],
  'resolver-failures': [failureEvent('submit-form'), failureEvent('submit-form')],
  'no-sink': [event('no-submit-sink', 'submit-form')],
  'the-document-home': [failureEvent('submit-form')],
  'handler-contract': [failureEvent('submit-form'), failureEvent('submit-form')],
  'submit-policy': [event('blocked', 'submit-form')],
};
const engineRead = (path: string, doc = 'a') => ({ doc, path, args: [] });
const submitOp = (sentAt: string, charSet?: string) => ({
  doc: 'a',
  path: 'forms.submit',
  args: [
    {
      entries: [{ name: 'plain', value: 'visible' }],
      intent: {
        url: 'https://home.test/submit',
        format: 'fdf',
        method: 'post',
        flagsRaw: 0,
        ...(charSet ? { charSet } : {}),
      },
      origin: 'user',
      sentAt,
    },
  ],
});
const expectedOps: Record<string, unknown[]> = {
  'execute-named': [engineRead('actions.get')],
  'get-action-tree': [
    ...Array.from({ length: 5 }, () => [
      engineRead('forms.list'),
      { doc: 'a', path: 'page.annotations.list', pon: 1, args: [] },
    ]).flat(),
    ...Array.from({ length: 5 }, () => engineRead('forms.list')),
    engineRead('actions.get'),
    engineRead('actions.get', 'b'),
  ],
  'named-print-goes-to-the-adapter': [engineRead('actions.get')],
  'hide-routes-by-plane': [engineRead('forms.list'), engineRead('forms.list')],
  'hide-and-later-document-work': [engineRead('forms.list'), engineRead('forms.list')],
  'hide-reports-honestly': Array.from({ length: 7 }, () => engineRead('forms.list')),
  'hide-form-failures': Array.from({ length: 3 }, () => engineRead('forms.list')),
  'hide-sink-throws': [engineRead('forms.list')],
  'hide-forms-read-rejects': [engineRead('forms.list')],
  'sinks-and-resolver-are-identity-safe': [
    engineRead('forms.list'),
    engineRead('forms.list'),
    submitOp('1970-01-01T00:00:00.000Z'),
  ],
  'a-user-execute-arms-the-open-sequence': [engineRead('actions.get')],
  'the-document-home': [
    submitOp('1970-01-01T00:00:01.500Z'),
    submitOp('1970-01-01T00:00:01.500Z', 'latin1'),
  ],
  'the-home-uses-the-configured-clock': [submitOp('1970-01-01T00:00:01.234Z')],
};
test('every owned operation and diagnostic callback payload is precisely accounted for', async () => {
  for (const entry of owned) {
    const { trace } = await runScenario(entry);
    const callbacks = trace.flatMap((step) => step.callbacks);
    assert.deepEqual(
      callbacks.filter((call) => call.fn.includes('Diagnostic')).map((call) => call.args),
      (diagnosticEvents[entry.name] ?? []).map((value) => [value]),
      entry.name,
    );
    const operations = trace
      .flatMap((step) => step.engine)
      .filter(
        (operation) =>
          !['open', 'pages.list', 'render.getPolicy'].includes(String(object(operation).path)),
      );
    assert.deepEqual(operations, expectedOps[entry.name] ?? [], entry.name);
  }
});

test('named execution events carry the current source/tree/result and preserve link provenance', async () => {
  const { trace } = await execute('execute-named');
  const emitted = trace
    .flatMap((step) => step.callbacks)
    .filter((call) => call.fn === 'onExecuted');
  const names = ['NextPage', 'FirstPage', 'Print', 'Print', 'Print'];
  assert.deepEqual(
    emitted.map((call) => call.args),
    names.map((name, index) => [
      {
        tree: treeValue(namedNode(name)),
        result: expected['execute-named']!.results[index],
        source: index === 1 ? { kind: 'link' } : { kind: 'api' },
      },
    ]),
  );
});

test('legacy registrations, builders and deferred scenario source sections remain byte exact', () => {
  const original = readFileSync(new URL('original-actions.ts.txt', historical), 'utf8');
  const current = readFileSync(new URL('../src/suites/actions.ts', import.meta.url), 'utf8');
  for (const [start, end] of [
    ['// ── the observer-error host input', 'interface HoverPumpConfig'],
    ['// ── builders', '// ── the dispatcher'],
  ]) {
    const section = (source: string) => source.slice(source.indexOf(start!), source.indexOf(end!));
    assert.equal(section(current), section(original));
  }
});

test('current registered factory keeps a scripting realm enabled with a null factory override', async () => {
  const input = owned.find((entry) => entry.name === 'rootless-tree-is-inert')!;
  const observed = await runScenario({
    ...input,
    plugins: [
      {
        factory: 'actions-current',
        config: { openSequence: 'off', javascript: { enabled: true, sandbox: null } },
      },
    ],
    steps: [
      ...input.steps.slice(0, 2),
      { op: 'call', target: { token: 'actions', doc: 'a' }, method: 'isScriptingEnabled' },
      { op: 'call', target: { token: 'actions', doc: 'a' }, method: 'getSettings' },
    ],
  });
  assert.deepEqual(observed.unmatched, []);
  assert.deepEqual(observed.trace[2]!.outcome, { returned: true });
  assert.deepEqual(object(object(observed.trace[3]!.outcome).returned).javascript, {
    enabled: true,
    identity: null,
  });
  assert.deepEqual(
    observed.trace.flatMap((step) => step.callbacks),
    [],
  );
});
