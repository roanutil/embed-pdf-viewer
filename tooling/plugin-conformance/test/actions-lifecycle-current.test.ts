import assert from 'node:assert/strict';
import { test } from 'node:test';
import { suites } from '../src/suites/actions';
import { runScenario } from '../src/scenario';
const files = ['plugins/actions-triggers.json', 'plugins/actions-lifecycle.json'];
const owned = suites.flatMap((suite) =>
  suite.kind === 'scenario' && files.includes(suite.file) ? suite.cases : [],
);
const execute = async (name: string) => {
  const entry = owned.find((entry) => entry.name === name);
  assert.ok(entry, name);
  return runScenario(entry);
};
test('page triggers preserve O/PO, PC/C and visibility fan-out order', async () => {
  const observed = await execute('page-fan-out-order');
  assert.deepEqual(observed.unmatched, []);
  assert.deepEqual(
    observed.trace
      .flatMap((step) => step.callbacks)
      .filter((call) => call.fn === 'exec.named')
      .map((call) => call.args[0]),
    [
      { type: 'named', subtype: 'Named', name: 'pageO', next: [] },
      { type: 'named', subtype: 'Named', name: 'PO-11', next: [] },
      { type: 'named', subtype: 'Named', name: 'PO-12', next: [] },
      { type: 'named', subtype: 'Named', name: 'PC-11', next: [] },
      { type: 'named', subtype: 'Named', name: 'pageC', next: [] },
      { type: 'named', subtype: 'Named', name: 'PV-11', next: [] },
      { type: 'named', subtype: 'Named', name: 'PI-11', next: [] },
    ],
  );
});

import { readFileSync } from 'node:fs';
import { canonical, canonicalError, snapshotHooks } from '../src/canonical';
import type { TraceEntry } from '../src/scenario';
const object = (value: unknown): Record<string, unknown> => {
  assert.ok(value && typeof value === 'object' && !Array.isArray(value));
  return value as Record<string, unknown>;
};
const page = (objectNumber: number) => ({ kind: 'objectNumber', objectNumber });
const ref = (pon: number, objectNumber: number) => ({
  kind: 'objectNumber',
  page: page(pon),
  objectNumber,
});
const source = (pon: number, number?: number) =>
  number === undefined
    ? { kind: 'page', page: page(pon) }
    : { kind: 'annotation', annotation: ref(pon, number), page: page(pon) };
const docSource = { kind: 'document' };
const namedNode = (name: string) => ({ type: 'named', subtype: 'Named', name, next: [] });
const scriptNode = (script: string, next: unknown[] = []) => ({
  type: 'javascript',
  subtype: 'JavaScript',
  script,
  next,
});
const gotoNode = (pon: number) => ({
  type: 'goto',
  subtype: 'GoTo',
  destination: { kind: 'fit', page: page(pon) },
  next: [],
});
const treeValue = (root: unknown, incomplete = false) => ({
  root,
  incomplete,
  warningFlags: 0,
  warnings: incomplete ? ['incomplete'] : [],
});
const diagnostic = (code: string, message: string) => ({ code, message });
const nodeResult = (type: string, status = 'executed', detail?: string, path: number[] = []) => ({
  path,
  type,
  status,
  ...(detail === undefined ? {} : { detail }),
});
const dispatchResult = (status: string, nodes: unknown[], diagnostics: unknown[] = []) => ({
  status,
  nodes,
  diagnostics,
});
const done = (type = 'named') => dispatchResult('executed', [nodeResult(type)]);
const step = (source: unknown, root: unknown, result: unknown = done()) => ({
  source,
  tree: treeValue(root),
  result,
});
const ns = (name: string, pon: number, number?: number) =>
  step(source(pon, number), namedNode(name));
const ds = (script: string) => step(docSource, scriptNode(script), done('javascript'));
const triggerResult = (
  steps: unknown[] = [],
  status = steps.length ? 'executed' : 'inert',
  diagnostics: unknown[] = [],
) => ({ status, steps, diagnostics });
const inert = () => triggerResult();
const failedTrigger = (message: string) =>
  triggerResult([], 'refused', [
    diagnostic('trigger-failed', `trigger resolution failed: ${message}`),
  ]);
const replay = () =>
  triggerResult([], 'inert', [
    diagnostic('open-sequence-replayed', 'document open sequence already fired for this document'),
  ]);
const disabled = (family: string) =>
  triggerResult([], 'inert', [
    diagnostic('trigger-disabled', `${family}: trigger family disabled by config`),
  ]);
const incomplete = (source: unknown) => ({
  source,
  tree: treeValue(null, true),
  result: dispatchResult(
    'refused',
    [],
    [diagnostic('incomplete-tree', 'refused: the action tree is incomplete')],
  ),
});
const values = (trace: readonly TraceEntry[]) =>
  trace
    .flatMap((s) => s.settled)
    .flatMap((s) => {
      const value = s.resolved;
      return value && typeof value === 'object' && 'status' in value ? [value] : [];
    });
const callbacks = (trace: readonly TraceEntry[]) => trace.flatMap((s) => s.callbacks);
const shorthand = (trace: readonly TraceEntry[]) =>
  callbacks(trace).map((c) =>
    c.fn.startsWith('exec.')
      ? `${c.fn}:${object(c.args[0]).name ?? object(c.args[0]).script ?? 'goto'}`
      : c.fn,
  );
const diagnosticCall = 'onDiagnosticReported';
const expectedCalls: Record<string, string[]> = {
  'activate-resolves-the-annotation-tree': [
    'exec.named:click-9',
    'exec.named:click-nm',
    'exec.named:click-index',
    'exec.named:click-numberless',
  ],
  'annotation-events-resolve-their-aa-tree': ['E', 'X', 'D', 'U', 'Fo', 'Bl'].flatMap((n) => [
    `exec.named:${n}`,
    'onExecuted',
  ]),
  'a-source-hint-is-provenance-only': ['exec.named:e', 'onExecuted'],
  'a-precedes-aa-u': ['exec.named:U-2'],
  'an-incomplete-trigger-tree-is-refused': [diagnosticCall],
  'resolution-never-rejects': [diagnosticCall, diagnosticCall, diagnosticCall],
  'submission-order-is-execution-order': ['exec.named:closeA', 'exec.named:openB'],
  'page-fan-out-order': ['pageO', 'PO-11', 'PO-12', 'PC-11', 'pageC', 'PV-11', 'PI-11'].flatMap(
    (n) => [`exec.named:${n}`, 'onExecuted'],
  ),
  'a-failed-step-never-skips-its-siblings': ['exec.javascript:boom', 'exec.named:pageC'],
  'single-tree-lifecycle-annotations': ['exec.named:PC-31', 'exec.named:PV-32', 'exec.named:PI-33'],
  'an-all-inert-fan-out-is-inert': [],
  'deferred-navigation-flushes-per-step': ['exec.goto:goto', 'exec.named:PO'],
  'lifecycle-cache-and-invalidation': ['PV-11', 'PI-11', 'PV-11', 'PV-11', 'PV-11', 'PV-11'].map(
    (n) => `exec.named:${n}`,
  ),
  'trigger-gates': [diagnosticCall, diagnosticCall, diagnosticCall],
  'gates-default-open': [],
  'auto-fires-on-adapter-install': ['exec.goto:goto', 'exec.named:OpenAction', 'exec.named:O-3'],
  'auto-fires-ahead-of-the-first-user-dispatch': ['exec.named:OpenAction', 'exec.named:click'],
  'open-sequence-completed-event': [
    'exec.goto:goto',
    'exec.named:OpenAction',
    'completed.bad',
    'completed',
  ],
  'auto-a-submit-handler-does-not-arm': ['exec.named:OpenAction'],
  'headless-falls-back-to-the-first-page': ['exec.named:O-8'],
  'headless-with-a-report': ['exec.named:O-3'],
  'headless-with-no-pages': [],
  'off-releases-the-barrier': ['exec.named:O-1'],
  'a-replayed-open-trigger': [diagnosticCall, diagnosticCall],
  'open-sequence-read-fails': [diagnosticCall, 'exec.named:O-1', 'exec.named:O-3'],
  'no-actions-service': [],
  'coordinator-diffs': [
    'PV-1',
    'O-1',
    'C-1',
    'PI-1',
    'PV-2',
    'O-2',
    'PV-1',
    'C-2',
    'PI-1',
    'PI-2',
  ].map((n) => `exec.named:${n}`),
  'cascade-budget': ['O-1', 'O-2', 'O-1', 'O-2', 'O-1', 'O-2', 'O-1', 'O-2', 'O-1']
    .map((n) => `exec.named:${n}`)
    .concat([diagnosticCall, diagnosticCall, 'exec.named:O-2', 'exec.named:O-1']),
  'cascade-budget-resets-on-user-activity': [
    'O-1',
    'O-2',
    'O-1',
    'O-2',
    'O-1',
    'O-2',
    'O-1',
    'O-2',
    'O-1',
    'focus',
    'O-2',
    'O-1',
  ].map((n) => `exec.named:${n}`),
  'the-five-catalog-trees': ['ws', 'ds', 'wp', 'dp', 'wc'].map((n) => `exec.javascript:${n}`),
  'missing-trees-are-inert': [],
  'open-runs-before-the-first-document-event': [
    'exec.javascript:open',
    'exec.javascript:ws',
    diagnosticCall,
    'exec.javascript:ws',
  ],
  'a-rejected-catalog-read-is-evicted': [diagnosticCall, 'exec.javascript:ws'],
  'document-family-disabled': ['op.save'],
  'prepare-close': ['exec.javascript:wc'],
  'run-document-verb-serializes': [
    'exec.javascript:ws',
    'op.save',
    'exec.javascript:ds',
    'exec.javascript:ws',
    'op.save',
    'exec.javascript:ds',
  ],
  'run-document-verb-failure-laws': [
    'exec.javascript:ws',
    diagnosticCall,
    'op.save',
    'exec.javascript:ds',
    'exec.javascript:ws',
    'op.save',
    'exec.javascript:ws',
    'op.save',
  ],
  'run-document-verb-print-holds-the-latch': [
    'exec.javascript:wp',
    diagnosticCall,
    'op.print',
    'exec.javascript:dp',
    'exec.javascript:wp',
    diagnosticCall,
    'op.print',
    'exec.javascript:wp',
    diagnosticCall,
    'ui.print',
    'exec.javascript:dp',
  ],
  'print-verb-runs-wp-adapter-dp': [
    'exec.javascript:wp',
    diagnosticCall,
    'ui.print',
    'exec.javascript:dp',
  ],
  'an-adapter-throw-skips-did-print': [
    'exec.javascript:wp',
    'ui.print',
    'exec.javascript:wp',
    'ui.print',
    'exec.javascript:dp',
  ],
  'adapter-uninstalled-mid-print': ['exec.javascript:wp'],
  'an-open-uri-throw-rejects-execute': ['ui.openUri'],
};
const expectedResults: Record<string, unknown[]> = {
  'activate-resolves-the-annotation-tree': [
    triggerResult([ns('click-9', 1, 9)]),
    failedTrigger('no page 99'),
    triggerResult([
      step(
        {
          kind: 'annotation',
          annotation: { kind: 'baseIndex', page: page(1), baseIndex: 2 },
          page: page(1),
        },
        namedNode('click-nm'),
      ),
    ]),
    inert(),
    inert(),
    triggerResult([
      step(
        {
          kind: 'annotation',
          annotation: { kind: 'baseIndex', page: page(1), baseIndex: 3 },
          page: page(1),
        },
        namedNode('click-index'),
      ),
    ]),
    inert(),
    inert(),
    inert(),
    inert(),
    triggerResult([
      step(
        {
          kind: 'annotation',
          annotation: { kind: 'baseIndex', page: page(2), baseIndex: 0 },
          page: page(2),
        },
        namedNode('click-numberless'),
      ),
    ]),
  ],
  'annotation-events-resolve-their-aa-tree': ['E', 'X', 'D', 'U', 'Fo', 'Bl']
    .map((n) => triggerResult([ns(n, 4, 7)]))
    .concat([inert()]),
  'a-source-hint-is-provenance-only': [
    triggerResult([step({ kind: 'link', annotation: ref(1, 7), page: page(1) }, namedNode('e'))]),
  ],
  'a-precedes-aa-u': [inert(), triggerResult([ns('U-2', 1, 2)])],
  'an-incomplete-trigger-tree-is-refused': [triggerResult([incomplete(source(1, 1))], 'refused')],
  'resolution-never-rejects': [
    failedTrigger('read exploded'),
    failedTrigger('no page 77'),
    failedTrigger('no page 77'),
  ],
  'submission-order-is-execution-order': [
    triggerResult([ns('closeA', 1)]),
    triggerResult([ns('openB', 2)]),
  ],
  'page-fan-out-order': [
    triggerResult([ns('pageO', 5), ns('PO-11', 5, 11), ns('PO-12', 5, 12)]),
    triggerResult([ns('PC-11', 5, 11), ns('pageC', 5)]),
    triggerResult([ns('PV-11', 5, 11)]),
    triggerResult([ns('PI-11', 5, 11)]),
    inert(),
  ],
  'a-failed-step-never-skips-its-siblings': [
    triggerResult(
      [
        step(
          source(6, 21),
          scriptNode('boom'),
          dispatchResult(
            'partial',
            [nodeResult('javascript', 'failed', 'boom')],
            [diagnostic('executor-failed', 'javascript: boom')],
          ),
        ),
        ns('pageC', 6),
      ],
      'partial',
    ),
  ],
  'single-tree-lifecycle-annotations': [
    triggerResult([ns('PC-31', 6, 31)]),
    triggerResult([ns('PV-32', 6, 32)]),
    triggerResult([ns('PI-33', 6, 33)]),
  ],
  'an-all-inert-fan-out-is-inert': [
    triggerResult(
      [
        step(
          source(6),
          scriptNode('orphan()'),
          dispatchResult(
            'inert',
            [nodeResult('javascript', 'inert')],
            [
              diagnostic(
                'no-executor',
                'javascript: no executor registered (scripting unavailable)',
              ),
            ],
          ),
        ),
        step(
          source(6, 34),
          namedNode('Print'),
          dispatchResult('inert', [nodeResult('named', 'blocked')]),
        ),
      ],
      'inert',
    ),
  ],
  'deferred-navigation-flushes-per-step': [
    triggerResult([step(source(7), gotoNode(2), done('goto')), ns('PO', 7, 31)]),
  ],
  'lifecycle-cache-and-invalidation': [
    triggerResult([ns('PV-11', 5, 11)]),
    triggerResult([ns('PI-11', 5, 11)]),
    ...Array.from({ length: 4 }, () => triggerResult([ns('PV-11', 5, 11)])),
  ],
  'trigger-gates': [disabled('page'), disabled('annotation'), disabled('document'), inert()],
  'gates-default-open': [],
  'auto-fires-on-adapter-install': [inert(), inert()],
  'auto-fires-ahead-of-the-first-user-dispatch': [triggerResult([ns('click', 2, 9)])],
  'open-sequence-completed-event': [inert(), inert()],
  'auto-a-submit-handler-does-not-arm': [],
  'headless-falls-back-to-the-first-page': [inert()],
  'headless-with-a-report': [inert()],
  'headless-with-no-pages': [inert()],
  'off-releases-the-barrier': [],
  'a-replayed-open-trigger': [triggerResult([incomplete(docSource)], 'refused'), replay()],
  'open-sequence-read-fails': [inert()],
  'no-actions-service': [inert(), replay()],
  'coordinator-diffs': [],
  'cascade-budget': [],
  'cascade-budget-resets-on-user-activity': [triggerResult([ns('focus', 1, 5)])],
  'the-five-catalog-trees': ['ws', 'ds', 'wp', 'dp', 'wc'].map((n) => triggerResult([ds(n)])),
  'missing-trees-are-inert': [inert(), inert()],
  'open-runs-before-the-first-document-event': [
    triggerResult([ds('ws')]),
    replay(),
    triggerResult([ds('ws')]),
  ],
  'a-rejected-catalog-read-is-evicted': [
    triggerResult([], 'inert', [diagnostic('trigger-failed', 'will-save: transient read failure')]),
    triggerResult([ds('ws')]),
    inert(),
  ],
  'document-family-disabled': [disabled('document'), disabled('document')],
  'prepare-close': [triggerResult([ds('wc')])],
  'run-document-verb-serializes': [],
  'run-document-verb-failure-laws': [],
  'run-document-verb-print-holds-the-latch': [done()],
  'print-verb-runs-wp-adapter-dp': [done()],
  'an-adapter-throw-skips-did-print': [done()],
  'adapter-uninstalled-mid-print': [
    dispatchResult('inert', [
      nodeResult('named', 'no-executor', 'UI adapter uninstalled mid-print'),
    ]),
  ],
  'an-open-uri-throw-rejects-execute': [],
};
const error = (message: string) => ({ $error: { name: 'Error', message } });
const rejections: Record<string, unknown[]> = {
  'run-document-verb-failure-laws': [error('save failed'), error('save rejected')],
  'run-document-verb-print-holds-the-latch': [error('printer on fire')],
  'an-adapter-throw-skips-did-print': [error('dialog exploded')],
  'an-open-uri-throw-rejects-execute': [error('popup blocked')],
};
// Run serially: cases own temporary host hooks and may not overlap.
const observations = new Map<string, Awaited<ReturnType<typeof runScenario>>>();
async function all() {
  if (!observations.size)
    for (const entry of owned) observations.set(entry.name, await runScenario(entry));
  return observations;
}
test('all 42 cases assert complete current results, exact callback order and named negative channels', async () => {
  assert.equal(owned.length, 42);
  assert.deepEqual(Object.keys(expectedResults).sort(), owned.map((x) => x.name).sort());
  assert.deepEqual(Object.keys(expectedCalls).sort(), owned.map((x) => x.name).sort());
  for (const [name, observed] of await all()) {
    assert.deepEqual(observed.unmatched, [], name + ' unmatched');
    assert.deepEqual(
      observed.trace.flatMap((s) => (s.outcome && 'threw' in s.outcome ? [s.outcome] : [])),
      [],
      name + ' step throws',
    );
    assert.ok(
      observed.trace.every((s) => s.status !== 'unconstructed'),
      name + ' construction',
    );
    assert.deepEqual(values(observed.trace), expectedResults[name], name + ' full action results');
    assert.deepEqual(shorthand(observed.trace), expectedCalls[name], name + ' callback order');
    assert.deepEqual(
      observed.trace
        .flatMap((s) => s.settled)
        .filter((s) => 'rejected' in s)
        .map((s) => s.rejected),
      rejections[name] ?? [],
      name + ' rejected',
    );
    assert.deepEqual(
      observed.trace.flatMap((s) => s.reports),
      name === 'open-sequence-completed-event' ? [error('listener broke')] : [],
      name + ' reports',
    );
    assert.deepEqual(
      callbacks(observed.trace).filter((c) => c.fn.startsWith('console.')),
      [],
      name + ' console',
    );
    assert.deepEqual(
      observed.trace.flatMap((s) => s.stateChanges),
      [],
      name + ' session state',
    );
  }
});

import { createTestContext } from '@embedpdf/core/testing';
import { DocumentsToken, type DocumentsCapability } from '@embedpdf/core';
import { createActionsController } from '../../../packages/plugin/actions/src/controller';
import {
  ACTIONS_DEFAULTS,
  type ActionsSettings,
  type ActionDiagnosticReportedEvent,
  type ScriptDiagnosticReportedEvent,
} from '../../../packages/plugin/actions/src/contract';
import { registeredSettings } from '../../../packages/plugin/actions/src/settings';
import { createFakeEngine } from '../src/fake-engine';
import * as current from '../src/suites/actions-current-fixtures';

test('M14 executor print distinguishes kernel origin refusal, adapter policy and accepted user print', async () => {
  for (const printer of [false, true]) {
    const calls: { fn: string; args: unknown[] }[] = [];
    const config = current.createCurrentActionsConfig(
      {
        openSequence: 'off',
        javascript: {
          enabled: true,
          sandbox: { builds: [{ run: [{ output: { uiEffects: [{ kind: 'print' }] } }] }] },
        },
      },
      {
        log: (fn, args) => calls.push({ fn, args }),
        token: () => {
          throw new Error('unused token');
        },
        defer: () => {
          throw new Error('unexpected pending callback');
        },
      },
    )!;
    const fake = createFakeEngine(current.engineWith(), (value) => value);
    const doc = await fake.engine.open({ kind: 'bytes', id: 'a', bytes: new Uint8Array() });
    let print: () => Promise<void> = async () => {
      throw new Error('print not connected');
    };
    const documentPort = { canPrint: () => printer, print: () => print() } satisfies Pick<
      DocumentsCapability,
      'canPrint' | 'print'
    >;
    const ctx = createTestContext<void, ActionsSettings>({
      id: 'actions',
      documentId: 'a',
      doc,
      pages: [{ ref: current.pr(1) }],
      settings: { defaults: ACTIONS_DEFAULTS, registered: registeredSettings(config) },
      capabilities: [[DocumentsToken, documentPort]],
    });
    const actions = ctx.connect(createActionsController(ctx, config));
    const log: unknown[] = [];
    print = () => {
      log.push('kernel.print');
      return ctx.print(async () => {
        log.push('dialog');
      });
    };
    actions.setUiAdapter({
      openUri: () => {},
      print: (context) => {
        log.push({ adapter: context ?? null });
      },
    });
    const diagnostics: ActionDiagnosticReportedEvent[] = [];
    const scriptDiagnostics: ScriptDiagnosticReportedEvent[] = [];
    actions.onDiagnosticReported((value) => diagnostics.push(value));
    actions.onScriptDiagnosticReported((value) => scriptDiagnostics.push(value));
    try {
      assert.deepEqual(
        await actions.execute(current.tree(current.js('print()')), current.LIFECYCLE),
        {
          status: 'executed',
          nodes: [{ path: [], type: 'javascript', status: 'executed' }],
          diagnostics: [],
        },
      );
      assert.deepEqual(
        log,
        [],
        'lifecycle executor print never reaches either printer under defaults',
      );
      assert.deepEqual(diagnostics, [
        { code: 'blocked', action: 'javascript', source: { kind: 'api' } },
      ]);
      assert.deepEqual(scriptDiagnostics, []);
      diagnostics.length = 0;
      actions.updateSettings({ policy: { print: { lifecycle: 'adapter' } } });
      assert.deepEqual(
        await actions.execute(current.tree(current.js('print()')), current.LIFECYCLE),
        {
          status: 'executed',
          nodes: [{ path: [], type: 'javascript', status: 'executed' }],
          diagnostics: [],
        },
      );
      assert.deepEqual(
        log,
        printer ? [] : [{ adapter: { origin: 'lifecycle', phase: 'user' } }],
        'kernel origin refusal survives an adapter-allow policy row',
      );
      assert.deepEqual(
        diagnostics,
        printer ? [{ code: 'blocked', action: 'javascript', source: { kind: 'api' } }] : [],
      );
      log.length = 0;
      diagnostics.length = 0;
      actions.updateSettings({ policy: { print: { lifecycle: 'block' } } });
      assert.deepEqual(await actions.execute(current.tree(current.js('print()')), current.USER), {
        status: 'executed',
        nodes: [{ path: [], type: 'javascript', status: 'executed' }],
        diagnostics: [],
      });
      // Drain the detached kernel print wrapper through the same public queue.
      await actions.dispatch(current.docEvent('did-save'));
      assert.deepEqual(
        log,
        printer ? ['kernel.print', 'dialog'] : [{ adapter: { origin: 'user', phase: 'user' } }],
      );
      assert.deepEqual(diagnostics, []);
      log.length = 0;
      // This is the separate form-pipeline surface carry, not an executor policy exemption.
      actions.surfaceScriptResult({
        uiEffects: [{ kind: 'print' }],
        diagnostics: [],
        origin: 'lifecycle',
        phase: 'user',
        realm: 'document',
      });
      assert.deepEqual(log, printer ? [] : [{ adapter: { origin: 'lifecycle', phase: 'user' } }]);
      assert.deepEqual(
        scriptDiagnostics,
        printer
          ? [
              {
                code: 'ui-effect-suppressed',
                message: 'script print request from a lifecycle event suppressed',
              },
            ]
          : [],
      );
      if (printer) {
        diagnostics.length = 0;
        print = async () => {
          throw new Error('kernel printer failed');
        };
        assert.deepEqual(await actions.execute(current.tree(current.js('print()')), current.USER), {
          status: 'executed',
          nodes: [{ path: [], type: 'javascript', status: 'executed' }],
          diagnostics: [],
        });
        await actions.dispatch(current.docEvent('did-save'));
        assert.deepEqual(log, []);
        assert.deepEqual(
          diagnostics,
          [{ code: 'trigger-failed', action: 'javascript', source: { kind: 'api' } }],
          'detached printer failure is an external diagnostic, not execute rejection',
        );
      }
      assert.deepEqual(fake.unmatched, []);
    } finally {
      ctx.dispose();
      await doc.close();
    }
  }
});

const op = (path: string, pon?: number) => ({
  doc: 'a',
  path,
  ...(pon === undefined ? {} : { pon }),
  args: [],
});
const formOp = () => op('forms.list');
const annotOp = (pon: number) => op('page.annotations.list', pon);
const catalogOp = () => op('actions.get');
const annotationReads = (...pons: number[]) => pons.flatMap((pon) => [formOp(), annotOp(pon)]);
const pageReads = (...pons: number[]) => pons.flatMap((pon) => [annotOp(pon), formOp()]);
const engineExpected: Record<string, unknown[]> = {
  'activate-resolves-the-annotation-tree': annotationReads(1, 99, 1, 2, 1, 1, 2, 1, 1, 1, 2),
  'annotation-events-resolve-their-aa-tree': annotationReads(4, 4, 4, 4, 4, 4, 4),
  'a-source-hint-is-provenance-only': annotationReads(1),
  'a-precedes-aa-u': annotationReads(1, 1),
  'an-incomplete-trigger-tree-is-refused': annotationReads(1),
  'resolution-never-rejects': [...annotationReads(1, 77), ...pageReads(77)],
  'submission-order-is-execution-order': pageReads(1, 2),
  'page-fan-out-order': pageReads(5, 6),
  'a-failed-step-never-skips-its-siblings': pageReads(6),
  'single-tree-lifecycle-annotations': pageReads(6),
  'an-all-inert-fan-out-is-inert': pageReads(6),
  'deferred-navigation-flushes-per-step': pageReads(7),
  'lifecycle-cache-and-invalidation': [
    ...pageReads(5, 5, 5),
    op('close'),
    ...pageReads(5),
    op('close'),
  ],
  'trigger-gates': annotationReads(1),
  'gates-default-open': [],
  'auto-fires-on-adapter-install': [catalogOp(), ...pageReads(3)],
  'auto-fires-ahead-of-the-first-user-dispatch': [catalogOp(), ...annotationReads(2)],
  'open-sequence-completed-event': [catalogOp()],
  'auto-a-submit-handler-does-not-arm': [catalogOp()],
  'headless-falls-back-to-the-first-page': [catalogOp(), ...pageReads(8)],
  'headless-with-a-report': [catalogOp(), ...pageReads(1, 3)],
  'headless-with-no-pages': [catalogOp()],
  'off-releases-the-barrier': pageReads(1),
  'a-replayed-open-trigger': [catalogOp()],
  'open-sequence-read-fails': [catalogOp(), ...pageReads(1), catalogOp(), ...pageReads(3)],
  'no-actions-service': [catalogOp(), ...pageReads(1)],
  'coordinator-diffs': pageReads(1, 2),
  'cascade-budget': pageReads(1, 2),
  'cascade-budget-resets-on-user-activity': [
    catalogOp(),
    ...pageReads(1, 2),
    ...annotationReads(1),
  ],
  'the-five-catalog-trees': [catalogOp()],
  'missing-trees-are-inert': [catalogOp()],
  'open-runs-before-the-first-document-event': [catalogOp()],
  'a-rejected-catalog-read-is-evicted': [catalogOp(), catalogOp()],
  'document-family-disabled': [],
  'prepare-close': [catalogOp()],
  'run-document-verb-serializes': [catalogOp()],
  'run-document-verb-failure-laws': [catalogOp()],
  'run-document-verb-print-holds-the-latch': [catalogOp()],
  'print-verb-runs-wp-adapter-dp': [catalogOp()],
  'an-adapter-throw-skips-did-print': [catalogOp()],
  'adapter-uninstalled-mid-print': [catalogOp()],
  'an-open-uri-throw-rejects-execute': [],
};
const diagEvent = (code: string, source: unknown, action: string | null = null) => ({
  code,
  action,
  source,
});
const diagnosticEvents: Record<string, unknown[]> = {
  'an-incomplete-trigger-tree-is-refused': [diagEvent('incomplete-tree', source(1, 1))],
  'resolution-never-rejects': [
    diagEvent('trigger-failed', source(1, 1)),
    diagEvent('trigger-failed', source(77, 1)),
    diagEvent('trigger-failed', source(77)),
  ],
  'trigger-gates': [
    diagEvent('trigger-disabled', source(1)),
    diagEvent('trigger-disabled', source(1, 1)),
    diagEvent('trigger-disabled', docSource),
  ],
  'a-replayed-open-trigger': [
    diagEvent('incomplete-tree', docSource),
    diagEvent('open-sequence-replayed', docSource),
  ],
  'open-sequence-read-fails': [diagEvent('trigger-failed', docSource)],
  'cascade-budget': [diagEvent('cascade-budget', null), diagEvent('cascade-budget', null)],
  'open-runs-before-the-first-document-event': [diagEvent('open-sequence-replayed', docSource)],
  'a-rejected-catalog-read-is-evicted': [diagEvent('trigger-failed', docSource)],
  'run-document-verb-failure-laws': [diagEvent('executor-failed', docSource, 'javascript')],
  'run-document-verb-print-holds-the-latch': Array.from({ length: 3 }, () =>
    diagEvent('reentrant-print', docSource, 'named'),
  ),
  'print-verb-runs-wp-adapter-dp': [diagEvent('reentrant-print', docSource, 'named')],
};
test('every case pins engine arguments, catalog/cache reads, diagnostic event payloads and all promise settlements', async () => {
  assert.deepEqual(Object.keys(engineExpected).sort(), owned.map((x) => x.name).sort());
  for (const [name, observed] of await all()) {
    assert.deepEqual(
      observed.trace
        .flatMap((s) => s.engine)
        .filter(
          (value) =>
            !['open', 'pages.list', 'render.getPolicy'].includes(String(object(value).path)),
        ),
      engineExpected[name],
      name + ' exact engine ops',
    );
    assert.deepEqual(
      callbacks(observed.trace)
        .filter((c) => c.fn === diagnosticCall)
        .map((c) => c.args),
      (diagnosticEvents[name] ?? []).map((value) => [value]),
      name + ' diagnostic payloads',
    );
    const promises = observed.trace
      .flatMap((s) => (s.outcome && 'promise' in s.outcome ? [s.outcome.promise] : []))
      .sort((a, b) => a - b);
    assert.deepEqual(
      observed.trace
        .flatMap((s) => s.settled)
        .map((s) => s.promise)
        .sort((a, b) => a - b),
      promises,
      name + ' exactly once, no dangling promises',
    );
  }
});
test('six annotation events and first-party hints preserve full origin/source/event and execution notifications', async () => {
  const observed = (await all()).get('annotation-events-resolve-their-aa-tree')!;
  const names = ['E', 'X', 'D', 'U', 'Fo', 'Bl'];
  const events = ['cursorEnter', 'cursorExit', 'mouseDown', 'mouseUp', 'focus', 'blur'];
  assert.deepEqual(
    callbacks(observed.trace),
    names.flatMap((name, index) => [
      {
        fn: 'exec.named',
        args: [
          namedNode(name),
          {
            origin: index < 2 ? 'hover' : 'user',
            source: source(4, 7),
            event: { scope: 'annotation', name: events[index] },
          },
        ],
      },
      {
        fn: 'onExecuted',
        args: [{ tree: treeValue(namedNode(name)), result: done(), source: source(4, 7) }],
      },
    ]),
  );
  const hint = { kind: 'link', annotation: ref(1, 7), page: page(1) };
  assert.deepEqual(callbacks((await all()).get('a-source-hint-is-provenance-only')!.trace), [
    {
      fn: 'exec.named',
      args: [
        namedNode('e'),
        { origin: 'hover', source: hint, event: { scope: 'annotation', name: 'cursorEnter' } },
      ],
    },
    { fn: 'onExecuted', args: [{ tree: treeValue(namedNode('e')), result: done(), source: hint }] },
  ]);
});
test('adapter installation coalesces reports and completion observers run once after both document steps', async () => {
  const auto = (await all()).get('auto-fires-on-adapter-install')!;
  assert.deepEqual(
    auto.trace.slice(0, 6).flatMap((s) => s.callbacks),
    [],
  );
  assert.deepEqual(auto.trace[6]!.callbacks, [
    {
      fn: 'exec.goto',
      args: [
        gotoNode(3),
        { origin: 'lifecycle', source: docSource, event: { scope: 'document', name: 'open' } },
      ],
    },
    {
      fn: 'exec.named',
      args: [
        namedNode('OpenAction'),
        { origin: 'lifecycle', source: docSource, event: { scope: 'document', name: 'open' } },
      ],
    },
    {
      fn: 'exec.named',
      args: [
        namedNode('O-3'),
        { origin: 'lifecycle', source: source(3), event: { scope: 'page', name: 'open' } },
      ],
    },
  ]);
  assert.deepEqual(
    auto.trace.slice(7).flatMap((s) => s.callbacks),
    [],
    'replacement adapter never replays',
  );
  const complete = (await all()).get('open-sequence-completed-event')!;
  const folded = triggerResult([
    step(docSource, gotoNode(3), done('goto')),
    step(docSource, namedNode('OpenAction')),
  ]);
  assert.deepEqual(complete.trace[6]!.callbacks.slice(2), [
    { fn: 'completed.bad', args: [{ result: folded }] },
    { fn: 'completed', args: [{ result: folded }] },
  ]);
  assert.deepEqual(complete.trace[6]!.reports, [error('listener broke')]);
  assert.deepEqual(
    complete.trace.slice(7).flatMap((s) => s.callbacks),
    [],
  );
  const submit = (await all()).get('auto-a-submit-handler-does-not-arm')!;
  assert.deepEqual(
    submit.trace.slice(0, 5).flatMap((s) => s.callbacks),
    [],
    'handler and null adapter leave latch armed',
  );
});
test('slow page reads and save operations retain call order; held print observes adapter disposal before settlement', async () => {
  const order = (await all()).get('submission-order-is-execution-order')!;
  assert.deepEqual(order.trace[3]!.engine, pageReads(1));
  assert.deepEqual(order.trace[4]!.engine, [], 'second read waits behind first');
  assert.deepEqual(
    order.trace.slice(2, 5).flatMap((s) => s.callbacks),
    [],
  );
  assert.deepEqual(
    order.trace.slice(2, 5).flatMap((s) => s.settled),
    [],
  );
  assert.deepEqual(shorthand([order.trace[5]!]), ['exec.named:closeA', 'exec.named:openB']);
  assert.deepEqual(order.trace[5]!.engine, pageReads(2));
  assert.deepEqual(
    order.trace[5]!.settled.map((s) => s.promise),
    [2, 3],
  );
  const save = (await all()).get('run-document-verb-serializes')!;
  assert.deepEqual(shorthand([save.trace[3]!]), ['exec.javascript:ws', 'op.save']);
  assert.deepEqual(save.trace[4]!.callbacks, []);
  assert.deepEqual(
    save.trace.slice(2, 5).flatMap((s) => s.settled),
    [],
  );
  assert.deepEqual(shorthand([save.trace[5]!]), [
    'exec.javascript:ds',
    'exec.javascript:ws',
    'op.save',
    'exec.javascript:ds',
  ]);
  assert.deepEqual(save.trace[5]!.settled, [
    { promise: 2, resolved: 'bytes-1' },
    { promise: 3, resolved: 'bytes-2' },
  ]);
  const held = (await all()).get('adapter-uninstalled-mid-print')!;
  assert.deepEqual(held.trace[3]!.outcome, { returned: { $ref: 1 } });
  assert.deepEqual(held.trace[4]!.outcome, { promise: 2 });
  assert.deepEqual(held.trace[4]!.settled, []);
  assert.deepEqual(held.trace[5]!.outcome, { returned: { $undefined: true } });
  assert.deepEqual(held.trace[5]!.settled, []);
  assert.deepEqual(held.trace[6]!.settled, [
    {
      promise: 2,
      resolved: dispatchResult('inert', [
        nodeResult('named', 'no-executor', 'UI adapter uninstalled mid-print'),
      ]),
    },
  ]);
  assert.deepEqual(
    callbacks(held.trace).filter((c) => c.fn === 'ui.print'),
    [],
  );
});
test('cache invalidation and close/reopen own current document subscriptions and drop all old registrations', async () => {
  const observed = (await all()).get('lifecycle-cache-and-invalidation')!;
  assert.deepEqual(
    [3, 4, 6, 8, 10, 14].map((index) => observed.trace[index]!.engine),
    [pageReads(5), [], [], pageReads(5), pageReads(5), pageReads(5)],
  );
  assert.deepEqual(
    [1, 11, 12, 15].map((index) => observed.trace[index]!.engineState),
    [
      { a: { subscribers: 3 } },
      { a: { subscribers: 0 } },
      { a: { subscribers: 3 } },
      { a: { subscribers: 0 } },
    ],
  );
  assert.deepEqual(observed.trace[12]!.outcome, { promise: 8 });
  assert.deepEqual(
    observed.trace[13]!.outcome,
    { returned: { $ref: 1 } },
    'fresh executor registration after reopening',
  );
  assert.deepEqual(shorthand([observed.trace[14]!]), ['exec.named:PV-11']);
  const closed = [
    { event: 'activeChanged', value: { documentId: null, previousDocumentId: 'a' } },
    { event: 'closed', value: { documentId: 'a' } },
  ];
  assert.deepEqual(observed.trace[11]!.documentEvents, closed);
  assert.deepEqual(observed.trace[15]!.documentEvents, closed);
});

test('all registry event channels and canDispatch booleans remain explicit', async () => {
  const pageCounts: Record<string, number> = {
    'annotation-events-resolve-their-aa-tree': 1,
    'a-failed-step-never-skips-its-siblings': 1,
    'single-tree-lifecycle-annotations': 1,
    'an-all-inert-fan-out-is-inert': 1,
    'deferred-navigation-flushes-per-step': 1,
    'off-releases-the-barrier': 1,
    'trigger-gates': 1,
    'auto-fires-ahead-of-the-first-user-dispatch': 1,
    'headless-falls-back-to-the-first-page': 1,
    'headless-with-no-pages': 0,
    'no-actions-service': 1,
  };
  for (const [name, observed] of await all()) {
    const opened = [
      { event: 'activeChanged', value: { documentId: 'a', previousDocumentId: null } },
      {
        event: 'opened',
        value: {
          documentId: 'a',
          info: {
            id: 'a',
            status: 'ready',
            pageCount: pageCounts[name] ?? 2,
            hasUnsavedChanges: false,
          },
        },
      },
    ];
    const closed = [
      { event: 'activeChanged', value: { documentId: null, previousDocumentId: 'a' } },
      { event: 'closed', value: { documentId: 'a' } },
    ];
    assert.deepEqual(
      observed.trace.flatMap((s) => s.documentEvents),
      name === 'lifecycle-cache-and-invalidation'
        ? [...opened, ...closed, ...opened, ...closed]
        : opened,
      name + ' exact registry channels',
    );
    const booleans = observed.trace.flatMap((s) =>
      s.outcome && 'returned' in s.outcome && typeof s.outcome.returned === 'boolean'
        ? [s.outcome.returned]
        : [],
    );
    assert.deepEqual(
      booleans,
      name === 'trigger-gates'
        ? [false, false, false, true]
        : name === 'gates-default-open'
          ? [true, true, true]
          : [],
      name + ' canDispatch',
    );
  }
});

test('prior C2 current 76 complete inputs and exact outcomes remain unchanged', async () => {
  const frozen = JSON.parse(
    readFileSync(
      new URL('../historical/task-12b2c2/current-inputs-runtime.json', import.meta.url),
      'utf8',
    ),
  ) as { file: string; name: string; input: unknown; outcome?: unknown; result?: unknown }[];
  assert.equal(frozen.length, 76);
  for (const saved of frozen) {
    const suite = suites.find((suite) => suite.file === saved.file)!;
    const entry = suite.cases.find((entry) => entry.name === saved.name)!;
    assert.deepEqual(canonical(entry), saved.input, saved.name + ' prior C2 input');
    if (suite.kind === 'scenario') {
      assert.ok('steps' in entry);
      assert.deepEqual(await runScenario(entry), saved.result, saved.name + ' prior C2 runtime');
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
      assert.deepEqual(outcome, saved.outcome, saved.name + ' prior C2 pure');
    }
  }
});

test('all 42 original occurrences remain archived and all current inputs match the C3 mapping', () => {
  const history = new URL('../historical/task-12b2c3/', import.meta.url);
  const read = (name: string) => JSON.parse(readFileSync(new URL(name, history), 'utf8'));
  const original = read('original-runtime.json');
  const prior = JSON.parse(
    readFileSync(
      new URL('../historical/task-12b2c2/deferred-runtime.json', import.meta.url),
      'utf8',
    ),
  ) as { file: string }[];
  assert.deepEqual(
    original,
    prior.filter((entry) => files.includes(entry.file)),
  );
  assert.equal(original.length, 42);
  const recorded = read('current-inputs-runtime.json') as { name: string; input: unknown }[];
  assert.equal(recorded.length, 42);
  const mapping = read('accounting.json');
  assert.equal(mapping.mappings.length, 42);
  assert.equal(mapping.summary.wholeRetirements, 0);
  assert.equal(mapping.summary.additions, 0);
  for (const entry of owned) {
    const found = recorded.filter((saved) => saved.name === entry.name);
    assert.equal(found.length, 1, entry.name);
    assert.deepEqual(canonical(entry), found[0]!.input, entry.name + ' current input mapping');
  }
});
