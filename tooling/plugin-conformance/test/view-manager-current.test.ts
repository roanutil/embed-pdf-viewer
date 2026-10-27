process.env.TZ = 'UTC';
import assert from 'node:assert/strict';
import test from 'node:test';
import { suites } from '../src/suites/view-manager';
import { runScenario } from '../src/scenario';
const pure = suites[0];
const live = suites[1];
if (pure.kind !== 'function' || live.kind !== 'scenario') throw Error('view manager suites');
const invoke = (name: string) => {
  const c = pure.cases.find((c) => c.name === name)!;
  return (pure.functions[c.fn] as (...args: unknown[]) => unknown)(...c.args);
};
test('creation replays generated ids and focus through the direct model transition', () => {
  assert.equal(pure.cases.find((c) => c.name === 'create')!.fn, 'createPane');
  assert.deepEqual(invoke('create-on-empty'), {
    panes: { 'pane-1': { id: 'pane-1', documentIds: [], activeDocumentId: null } },
    order: ['pane-1'],
    focusedPaneId: 'pane-1',
    seq: 1,
  });
});
test('unmodified workspace plugin installs and exposes empty selectors before start', async () => {
  const c = live.cases.find((c) => c.name === 'a-kernel-installs-the-shipping-plugin')!;
  const r = await runScenario(c);
  assert.deepEqual(
    r.trace.map((t) => t.outcome),
    [{ returned: [] }, { returned: [] }, { returned: null }],
  );
});

import * as model from '../../../packages/plugin/view-manager/src/model';
import { createViewManagerController } from '../../../packages/plugin/view-manager/src/controller';
import { createTestContext } from '../../../packages/core/main/src/testing';
import { DocumentsToken } from '@embedpdf/core';
import { canonical, snapshotHooks } from '../src/canonical';
// Independent literal oracle: changing focus, neighbour choice, clamping or partitioning breaks a row.
test('all82 pure rows pin complete states and ignored transition identities independently', () => {
  const pane = (
    id: string,
    documentIds: string[],
    activeDocumentId: string | null = documentIds[0] ?? null,
  ) => ({ id, documentIds, activeDocumentId });
  const p1 = pane('pane-1', ['a', 'b', 'c'], 'b'),
    p2 = pane('pane-2', ['d']),
    p3 = pane('pane-3', []);
  const state = (
    rows = [p1, p2, p3],
    focusedPaneId: string | null = 'pane-2',
    seq = 3,
    order = rows.map((p) => p.id),
  ) => ({ panes: Object.fromEntries(rows.map((p) => [p.id, p])), order, focusedPaneId, seq });
  const base = state();
  const one = (ids: string[], active: string | null = 'b') =>
    state([pane('pane-1', ids, active), p2, p3]);
  const added = (order = ['pane-1', 'pane-2', 'pane-3', 'pane-4']) =>
    state([p1, p2, p3, pane('pane-4', [])], 'pane-4', 4, order);
  const expected: Record<string, unknown> = {
    'initial-state': state([], null, 0),
    create: added(),
    'create-on-empty': state([pane('pane-1', [])], 'pane-1', 1),
    'remove-unknown-is-ignored': base,
    'remove-focused-focuses-the-last': state([p1, p3], 'pane-3'),
    'remove-unfocused-keeps-focus': state([p2, p3]),
    'remove-the-only-view': state([], null, 1),
    reorder: state([p1, p2, p3], 'pane-2', 3, ['pane-3', 'pane-1', 'pane-2']),
    'set-focused': state([p1, p2, p3], 'pane-3'),
    'set-focused-null': state([p1, p2, p3], null),
    'set-focused-same-is-ignored': base,
    'set-active': one(['a', 'b', 'c'], 'c'),
    'set-active-null': one(['a', 'b', 'c'], null),
    'set-active-unknown-view': base,
    'set-active-foreign-document': base,
    'add-appends': one(['a', 'b', 'c', 'e']),
    'add-at-index': one(['a', 'e', 'b', 'c']),
    'add-at-zero': one(['e', 'a', 'b', 'c']),
    'add-negative-clamps': one(['e', 'a', 'b', 'c']),
    'add-past-the-end-clamps': one(['a', 'b', 'c', 'e']),
    'add-fractional-truncates': one(['a', 'b', 'e', 'c']),
    'add-nan-lands-first': one(['e', 'a', 'b', 'c']),
    'add-infinity-appends': one(['a', 'b', 'c', 'e']),
    'add-negative-infinity-lands-first': one(['e', 'a', 'b', 'c']),
    'add-to-empty-activates': state([p1, p2, pane('pane-3', ['e'])]),
    'add-present-is-ignored': base,
    'add-unknown-view-is-ignored': base,
    'add-moves-from-its-holder': state([
      pane('pane-1', ['d', 'a', 'b', 'c'], 'b'),
      pane('pane-2', []),
      p3,
    ]),
    'add-moves-into-an-empty-pane': state([
      pane('pane-1', ['a', 'c'], 'c'),
      p2,
      pane('pane-3', ['b']),
    ]),
    'add-takes-the-first-holder-in-pane-order': state(
      [pane('0', ['y']), pane('1', ['x']), pane('b', ['x'])],
      '0',
    ),
    'remove-doc-inactive': one(['b', 'c']),
    'remove-doc-active-picks-the-next': one(['a', 'c'], 'c'),
    'remove-doc-active-last-picks-the-previous': state(
      [pane('pane-1', ['a', 'b'], 'b')],
      'pane-1',
      1,
    ),
    'remove-doc-only-leaves-null': state([p1, pane('pane-2', []), p3]),
    'remove-doc-absent-is-ignored': base,
    'remove-doc-unknown-view': base,
    'move-within': one(['b', 'c', 'a']),
    'move-within-to-front': one(['c', 'a', 'b']),
    'move-within-clamps': one(['b', 'c', 'a']),
    'move-within-nan': one(['c', 'a', 'b']),
    'move-within-absent': base,
    'move-within-unknown-view': base,
    'move-between': state([pane('pane-1', ['a', 'c'], 'c'), pane('pane-2', ['d', 'b'], 'b'), p3]),
    'move-between-at-index': state([
      pane('pane-1', ['b', 'c'], 'b'),
      pane('pane-2', ['a', 'd'], 'a'),
      p3,
    ]),
    'move-between-into-empty': state([p1, pane('pane-2', []), pane('pane-3', ['d'])], 'pane-3'),
    'move-between-same-view': one(['b', 'c', 'a']),
    'move-between-same-view-at-index': one(['c', 'a', 'b']),
    'move-between-unknown-from': base,
    'move-between-unknown-to': base,
    'move-between-absent-document': base,
    'reconcile-prunes-closed': state([pane('pane-1', ['a']), p2, p3]),
    'reconcile-prunes-the-active': state([pane('pane-1', ['a', 'c'], 'a'), p2, p3]),
    'reconcile-assigns-to-preferred': state([p1, p2, pane('pane-3', ['x', 'y'])]),
    'reconcile-assigns-to-focused': state([p1, pane('pane-2', ['d', 'x'], 'd'), p3]),
    'reconcile-assigns-to-first': state(
      [pane('pane-1', ['a', 'b', 'c', 'x'], 'b'), p2, p3],
      'gone',
    ),
    'reconcile-assigns-to-first-no-focus': state(
      [pane('pane-1', ['x']), pane('pane-2', []), p3],
      null,
    ),
    'reconcile-creates-a-default-pane': state([pane('pane-1', ['a', 'b'])], 'pane-1', 1),
    'reconcile-creates-and-keeps-a-stale-focus': state([pane('pane-5', ['a'])], 'x', 5),
    'reconcile-nothing-open': state([pane('pane-1', []), pane('pane-2', []), p3]),
    'reconcile-nothing-open-no-views': state([], null, 0),
    'reconcile-empty-active-is-replaced': state([pane('pane-1', ['a', 'b'])], 'pane-1', 1),
    'reconcile-duplicate-assignments': state(
      [pane('pane-1', ['a']), pane('pane-2', ['a', 'b'], 'b')],
      'pane-1',
      2,
    ),
    'reconcile-keeps-other-members': base,
    'next-pane-id': 'pane-4',
    'create-at-front': added(['pane-4', 'pane-1', 'pane-2', 'pane-3']),
    'create-at-fraction': added(['pane-1', 'pane-4', 'pane-2', 'pane-3']),
    'create-at-negative': added(['pane-4', 'pane-1', 'pane-2', 'pane-3']),
    'create-at-infinity': added(),
    'create-at-nan': added(['pane-4', 'pane-1', 'pane-2', 'pane-3']),
    'create-sequence-survives-removal': state([pane('pane-9', [])], 'pane-9', 9),
    'move-pane-negative': state([p1, p2, p3], 'pane-2', 3, ['pane-2', 'pane-1', 'pane-3']),
    'move-pane-infinity': state([p1, p2, p3], 'pane-2', 3, ['pane-2', 'pane-3', 'pane-1']),
    'move-pane-fractional': state([p1, p2, p3], 'pane-2', 3, ['pane-1', 'pane-3', 'pane-2']),
    'move-pane-nan': state([p1, p2, p3], 'pane-2', 3, ['pane-3', 'pane-1', 'pane-2']),
    'move-pane-unknown': base,
    'move-pane-same-index': base,
    'rename-active': one(['a', 'real', 'c'], 'real'),
    'rename-inactive': one(['real', 'b', 'c']),
    'rename-absent': base,
    'pane-of-document': 'pane-2',
    'pane-of-absent-document': null,
    'holder-follows-pane-order-not-property-order': state(
      [pane('0', ['x', 'y']), pane('1', []), pane('b', ['x'])],
      '0',
      3,
      ['1', '0', 'b'],
    ),
  };
  assert.equal(pure.cases.length, 82);
  assert.equal(Object.keys(expected).length, 82);
  const same = new Set([
    'remove-unknown-is-ignored',
    'set-focused-same-is-ignored',
    'set-active-unknown-view',
    'set-active-foreign-document',
    'add-present-is-ignored',
    'add-unknown-view-is-ignored',
    'remove-doc-absent-is-ignored',
    'remove-doc-unknown-view',
    'move-within-absent',
    'move-within-unknown-view',
    'move-between-unknown-from',
    'move-between-unknown-to',
    'move-between-absent-document',
    'move-pane-unknown',
    'rename-absent',
  ]);
  for (const c of pure.cases) {
    const actual = invoke(c.name);
    assert.deepEqual(actual, expected[c.name], c.name);
    if (!['initialViewManagerState', 'nextPaneId', 'paneOfDocument'].includes(c.fn))
      assert.equal(actual === c.args[0], same.has(c.name), c.name + ' identity');
  }
});
const run = (name: string) => runScenario(live.cases.find((c) => c.name === name)!);
const returned = (t: Awaited<ReturnType<typeof run>>['trace'][number]) =>
  t.outcome && 'returned' in t.outcome ? t.outcome.returned : undefined;
const p = (
  id: string,
  documentIds: string[],
  activeDocumentId: string | null = documentIds[0] ?? null,
) => ({ id, documentIds, activeDocumentId });
test('all13 scenarios have exact refusal/report channels and every promise settles', async () => {
  assert.equal(live.cases.length, 13);
  for (const c of live.cases) {
    const r = await runScenario(c);
    assert.deepEqual(r.unmatched, [], c.name);
    const started = r.trace.flatMap((t) =>
        t.outcome && 'promise' in t.outcome ? [t.outcome.promise] : [],
      ),
      settled = r.trace.flatMap((t) => t.settled.map((s) => s.promise));
    assert.deepEqual(
      started.filter((id) => !settled.includes(id)),
      [],
      c.name + ' pending',
    );
    assert.deepEqual(
      r.trace.flatMap((t, i) => (t.outcome && 'threw' in t.outcome ? [[i, t.outcome.threw]] : [])),
      c.name === 'destroy'
        ? [
            [
              3,
              {
                $error: {
                  name: 'Error',
                  message: '[kernel] capability("view-manager") on a destroyed kernel',
                },
              },
            ],
          ]
        : [],
      c.name,
    );
    assert.deepEqual(
      r.trace.flatMap((t, i) => t.reports.map((v) => [i, v])),
      c.name === 'events'
        ? [6, 7, 8, 11, 12].map((i) => [
            i,
            { $error: { name: 'Error', message: 'listener broke' } },
          ])
        : [],
      c.name + ' reports',
    );
    assert.deepEqual(
      r.trace.flatMap((t, i) => t.settled.filter((s) => 'rejected' in s).map((s) => [i, s])),
      c.name === 'a-failed-open'
        ? [
            [
              2,
              {
                promise: 2,
                rejected: {
                  $error: {
                    name: 'PluginError',
                    message: 'bad file',
                    code: 'operation-failed',
                    capability: 'documents',
                    permission: null,
                    cause: { $error: { name: 'EngineError', message: 'bad file', code: 'Format' } },
                  },
                },
              },
            ],
          ]
        : [],
      c.name + ' rejections',
    );
    for (const t of r.trace) {
      for (const value of [
        returned(t),
        ...t.callbacks,
        ...t.settled.filter((s) => !('rejected' in s)),
      ])
        assert.ok(!JSON.stringify(value)?.includes('"$error"'), c.name + ' hidden error');
    }
  }
});
test('live pane/tab placement, removals, active fallback and registry order have literal reads', async () => {
  const first = await run('first-open-makes-a-pane');
  assert.deepEqual(returned(first.trace[13]), [p('pane-1', ['a', 'b', 'c'])]);
  assert.deepEqual(first.trace[12].settled, []);
  assert.equal(first.trace[16].settled.length, 1);
  const close = await run('closing-prunes');
  assert.deepEqual(returned(close.trace[6]), [p('pane-1', ['a', 'c'])]);
  assert.deepEqual(returned(close.trace[10]), [p('pane-1', [])]);
  const panes = await run('panes');
  assert.deepEqual(returned(panes.trace[14]), ['pane-2', 'pane-1', 'pane-3']);
  assert.deepEqual(returned(panes.trace[19]), [p('pane-2', ['b']), p('pane-3', ['a', 'c'], 'c')]);
  assert.equal(returned(panes.trace[28]), 'ghost');
  const tabs = await run('tabs');
  assert.deepEqual(returned(tabs.trace[20]), [
    p('pane-1', ['b'], null),
    p('pane-2', ['c', 'a'], 'a'),
  ]);
  assert.equal(returned(tabs.trace[22]), 'pane-1');
  const reorder = await run('reordering-tabs-reconciles');
  assert.deepEqual(returned(reorder.trace[5]), [p('pane-1', ['a', 'b'])]);
  const fail = await run('a-failed-open');
  assert.deepEqual(returned(fail.trace[3]), [p('pane-1', ['a', 'x'])]);
  assert.deepEqual(returned(fail.trace[7]), [p('pane-1', ['a'])]);
  const populated = await run('create-populated-and-split');
  assert.deepEqual(returned(populated.trace[5]), [
    p('pane-1', ['b']),
    p('pane-2', ['c', 'a', 'zzz']),
  ]);
  assert.deepEqual(returned(populated.trace[13]), p('pane-4', []));
  const remove = await run('remove-pane-targets');
  assert.deepEqual(returned(remove.trace[12]), [
    p('pane-1', []),
    p('pane-2', ['a', 'c', 'd'], 'd'),
    p('pane-3', ['b']),
  ]);
  assert.deepEqual(returned(remove.trace[17]), [
    p('pane-1', ['b']),
    p('pane-2', ['a', 'c', 'd'], 'd'),
  ]);
  assert.deepEqual(returned(remove.trace[21]), [p('pane-2', ['a', 'c', 'd', 'b'], 'b')]);
  assert.deepEqual(
    [26, 31, 35, 37].map((i) => returned(remove.trace[i])),
    [[p('pane-5', [])], [p('pane-5', [])], [], null],
  );
  const rename = await run('pending-rename-preserves-split-and-tab-position');
  assert.deepEqual(returned(rename.trace[3]), [p('pane-1', []), p('pane-2', ['pending:1'])]);
  assert.deepEqual(returned(rename.trace[7]), [p('pane-1', []), p('pane-2', ['real'])]);
  assert.deepEqual(returned(rename.trace[13]), [
    p('pane-1', []),
    p('pane-4', ['b']),
    p('pane-2', ['real']),
    p('pane-3', []),
  ]);
  assert.equal(returned(rename.trace[15]), 'pane-4');
});
test('event diff preserves exact callback order and isolates the throwing listener', async () => {
  const r = await run('events');
  const cb = (fn: string, value: unknown) => ({ fn, args: [value] });
  const moved = (documentId: string, fromPaneId: string | null, toPaneId: string | null) =>
    ['moved', 'moved-2'].map((fn) => cb(fn, { documentId, fromPaneId, toPaneId }));
  assert.deepEqual(
    r.trace.flatMap((t, i) => t.callbacks.map((c) => [i, c])),
    [
      ...[
        cb('created', { paneId: 'pane-1' }),
        cb('focus', { paneId: 'pane-1' }),
        ...moved('a', null, 'pane-1'),
      ].map((c) => [6, c]),
      ...moved('b', null, 'pane-1').map((c) => [7, c]),
      ...[
        cb('created', { paneId: 'pane-2' }),
        cb('focus', { paneId: 'pane-2' }),
        ...moved('b', 'pane-1', 'pane-2'),
      ].map((c) => [8, c]),
      ...[
        cb('focus', { paneId: 'pane-1' }),
        ...moved('b', 'pane-2', 'pane-1'),
        cb('removed', { paneId: 'pane-2' }),
      ].map((c) => [11, c]),
      ...moved('a', 'pane-1', null).map((c) => [12, c]),
    ],
  );
});
test('controller memoization, rename, reconciliation and disposal preserve public semantics', async () => {
  let order = ['a', 'pending'];
  const ctx = createTestContext({
    id: 'view-manager',
    state: model.initialViewManagerState(),
    doc: null,
    capabilities: [[DocumentsToken, { getOrder: () => order }]],
  });
  const api = ctx.connect(createViewManagerController(ctx));
  const list = api.listPanes(),
    pane = api.getPane('pane-1');
  assert.equal(api.listPanes(), list);
  assert.equal(api.getPane('pane-1'), pane);
  const events: unknown[] = [];
  api.onDocumentMoved((e) => events.push(e));
  api.splitPane('pending');
  const sibling = api.getPane('pane-2');
  api.setActiveDocument('pane-1', null);
  assert.equal(api.getPane('pane-2'), sibling);
  assert.deepEqual(api.getPane('pane-2'), p('pane-2', ['pending']));
  const afterActive = api.getPane('pane-1');
  api.setFocusedPane('pane-1');
  assert.equal(api.getPane('pane-1'), afterActive);
  order = ['a', 'real'];
  ctx.notify();
  assert.deepEqual(api.getPane('pane-2'), p('pane-2', ['real']));
  assert.deepEqual(events, [
    { documentId: 'pending', fromPaneId: 'pane-1', toPaneId: 'pane-2' },
    { documentId: 'pending', fromPaneId: 'pane-2', toPaneId: null },
    { documentId: 'real', fromPaneId: null, toPaneId: 'pane-2' },
  ]);
  order = ['real'];
  ctx.notify();
  assert.deepEqual(api.getPane('pane-1'), p('pane-1', []));
  const atDispose = ctx.state.get();
  await ctx.dispose();
  order = ['after'];
  ctx.notify();
  assert.equal(ctx.state.get(), atDispose);
});
test('complete live snapshots retain pane records, pane/document order, focus and monotonic sequence', async () => {
  const state = (
    rows: ReturnType<typeof p>[],
    focusedPaneId: string | null,
    seq: number,
    order = rows.map((p) => p.id),
  ) => ({ panes: Object.fromEntries(rows.map((p) => [p.id, p])), order, focusedPaneId, seq });
  const expected: Record<string, unknown> = {
    'selectors-before-start': state([], null, 0),
    'first-open-makes-a-pane': state([p('pane-1', ['a', 'b', 'c'])], 'pane-1', 1),
    'closing-prunes': state([p('pane-1', [])], 'pane-1', 1),
    panes: state([p('pane-2', ['b']), p('pane-3', ['a'])], 'ghost', 3),
    tabs: state([p('pane-1', ['b'], null), p('pane-2', ['c', 'a'], 'a')], 'pane-1', 2),
    'reordering-tabs-reconciles': state([p('pane-1', ['a', 'b'])], 'pane-1', 1),
    'a-failed-open': state([p('pane-1', ['a'])], 'pane-1', 1),
    'create-populated-and-split': state(
      [p('pane-1', []), p('pane-2', ['c', 'a', 'zzz']), p('pane-3', ['b']), p('pane-4', [])],
      'pane-4',
      4,
    ),
    'remove-pane-targets': state([], null, 6),
    'a-kernel-installs-the-shipping-plugin': state([], null, 0),
    'pending-rename-preserves-split-and-tab-position': state(
      [p('pane-1', []), p('pane-2', ['real']), p('pane-3', []), p('pane-4', ['b'])],
      'pane-4',
      4,
      ['pane-1', 'pane-4', 'pane-2', 'pane-3'],
    ),
  };
  for (const [name, want] of Object.entries(expected)) {
    const r = await run(name);
    const state = r.trace.at(-1)!.state as { plugins: Record<string, unknown> };
    assert.deepEqual(state.plugins['view-manager'], want, name);
  }
  const destroyed = await run('destroy');
  assert.deepEqual(destroyed.trace[2].documentEvents, [
    { event: 'activeChanged', value: { documentId: null, previousDocumentId: 'a' } },
    { event: 'closed', value: { documentId: 'a' } },
  ]);
  assert.deepEqual(destroyed.trace[2].engineState, { a: { subscribers: 0 } });
  assert.deepEqual(destroyed.trace[3].state, { plugins: {} });
});

import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
type Saved = {
  file: string;
  kind: string;
  name: string;
  input: unknown;
  inputSha256: string;
  outcomeSha256?: string;
  outcome?: unknown;
  result?: unknown;
};
test('prior shell47 i18n642 and actions160 commands79 link37 metadata17 and page-edit19 retain all inputs and every output channel', async () => {
  const available = [
    ...(await import('../src/suites/shell')).suites,
    ...(await import('../src/suites/i18n')).suites,
    ...(await import('../src/suites/actions')).suites,
    ...(await import('../src/suites/commands')).suites,
    ...(await import('../src/suites/link')).suites,
    ...(await import('../src/suites/metadata')).suites,
    ...(await import('../src/suites/page-edit')).suites,
  ];
  let count = 0;
  for (const task of [
    'task-12b2c2',
    'task-12b2c3',
    'task-12b2c4',
    'task-12b2c5',
    'task-12b2c6',
    'task-12b2c7',
    'task-12b2c8',
    'task-12b2c9',
    'task-12b2c10',
  ]) {
    const rows = JSON.parse(
      readFileSync(
        new URL('../historical/' + task + '/current-inputs-runtime.json', import.meta.url),
        'utf8',
      ),
    ) as Saved[];
    for (const row of rows) {
      count++;
      const s = available.find((s) => s.file === row.file)!;
      const c = s.cases.find((c) => c.name === row.name)!;
      assert.deepEqual(canonical(c), row.input, row.name);
      if (s.kind === 'scenario') {
        assert.ok('steps' in c);
        assert.deepEqual(await runScenario(c), row.result, row.name);
      } else {
        assert.ok('fn' in c);
        assert.deepEqual(
          {
            returned: canonical(
              (s.functions[c.fn] as (...a: unknown[]) => unknown)(...c.args),
              snapshotHooks,
            ),
          },
          row.outcome,
          row.name,
        );
      }
    }
  }
  assert.equal(count, 1001);
});

const history = new URL('../historical/task-12b2c11/', import.meta.url);
const saved = (p: string): unknown => JSON.parse(readFileSync(new URL(p, history), 'utf8'));
const hash = (v: unknown) => createHash('sha256').update(JSON.stringify(v)).digest('hex');
test('all95 current full definitions and outcome channels replay exactly', async () => {
  const rows = saved('current-inputs-runtime.json') as Saved[];
  assert.equal(rows.length, 95);
  assert.equal(new Set(rows.map((r) => r.file + '/' + r.name)).size, 95);
  for (const r of rows) {
    const s = suites.find((s) => s.file === r.file)!;
    const c = s.cases.find((c) => c.name === r.name)!;
    assert.deepEqual(canonical(c), r.input, r.name);
    assert.equal(hash(r.input), r.inputSha256);
    assert.equal(hash(r.outcome ?? r.result), r.outcomeSha256);
    if (s.kind === 'scenario') {
      assert.ok('steps' in c);
      assert.deepEqual(await runScenario(c), r.result, r.name);
    } else {
      assert.ok('fn' in c);
      assert.deepEqual(
        {
          returned: canonical(
            (s.functions[c.fn] as (...a: unknown[]) => unknown)(...c.args),
            snapshotHooks,
          ),
        },
        r.outcome,
        r.name,
      );
    }
  }
});
test('all76 immutable original occurrences map one-to-one with exact partial retirements', () => {
  const pins: Record<string, string> = {
    'original-runtime.json': '4e9022d68b118f65d04ef820051a7c9834c3c94cc2c9aaca3de40804aa1bc4bd',
    'original-view-manager.ts.txt':
      'fc25627a0587d3fe6ad18d90d75424c04e42df2f500fa2db1752bd5c3709ef7e',
    'original-commands-current.test.ts.txt':
      'ebdae5bfb70dcb30be961ea79e6ae63367322f01cb6a89d466cacb9bce7cdb36',
  };
  for (const [p, want] of Object.entries(pins))
    assert.equal(
      createHash('sha256')
        .update(readFileSync(new URL(p, history)))
        .digest('hex'),
      want,
      p,
    );
  const old = saved('original-runtime.json') as Saved[];
  const account = saved('accounting.json') as {
    summary: unknown;
    mappings: {
      originalOccurrence: number;
      originalName: string;
      originalInput: unknown;
      originalInputSha256: string;
      originalOutcome: unknown;
      currentName: string;
      currentIndex: number;
      currentFile: string;
      currentInput: unknown;
      currentInputSha256: string;
      currentOutcome: unknown;
      removedPaneMessageContract: string | null;
    }[];
  };
  assert.equal(old.length, 76);
  assert.equal(account.mappings.length, 76);
  assert.deepEqual(account.summary, {
    original: 76,
    originalPure: 64,
    originalLive: 12,
    current: 95,
    currentPure: 82,
    currentLive: 13,
    mapped: 76,
    wholeRetirements: 1,
    survivingOriginals: 75,
    replacementRows: 1,
    totalNewRows: 20,
    partialRetirements: 62,
    removedPaneMessageOccurrences: 3,
    additions: 19,
    changedInputs: 75,
  });
  const cur = saved('current-inputs-runtime.json') as (Saved & { index: number })[];
  const seen = new Set();
  for (const [i, o] of old.entries()) {
    const m = account.mappings[i];
    assert.equal(m.originalOccurrence, i);
    assert.equal(m.originalName, o.name);
    assert.equal(hash(o.input), o.inputSha256);
    assert.equal(m.originalInputSha256, o.inputSha256);
    assert.deepEqual(m.originalInput, o.input);
    assert.deepEqual(m.originalOutcome, o.outcome ?? o.result);
    const c = cur.find((c) => c.file === m.currentFile && c.index === m.currentIndex)!;
    assert.equal(c.name, m.currentName);
    assert.deepEqual(m.currentInput, c.input);
    assert.equal(m.currentInputSha256, c.inputSha256);
    assert.deepEqual(m.currentOutcome, c.outcome ?? c.result);
    const key = c.file + '/' + c.index;
    assert.ok(!seen.has(key));
    seen.add(key);
  }
  assert.deepEqual(
    account.mappings.filter((m) => m.removedPaneMessageContract).map((m) => m.originalName),
    ['create', 'create-on-empty', 'reorder'],
  );
  assert.deepEqual(saved('task11-current-drift.json'), []);
  for (const [p, want] of Object.entries(saved('integrity.json') as Record<string, string>))
    assert.equal(
      createHash('sha256')
        .update(readFileSync(new URL(p, history)))
        .digest('hex'),
      want,
      p,
    );
});
