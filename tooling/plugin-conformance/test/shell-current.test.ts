process.env.TZ = 'UTC';
import assert from 'node:assert/strict';
import test from 'node:test';
import { suites } from '../src/suites/shell';
import { runScenario } from '../src/scenario';
import * as model from '../../../packages/plugin/shell/src/model';
import { createTestContext } from '../../../packages/core/main/src/testing';
import { createShellController } from '../../../packages/plugin/shell/src/controller';
const pure = suites[0];
const live = suites[1];
if (pure.kind !== 'function' || live.kind !== 'scenario') throw Error('shell suites');
const run = (name: string) => runScenario(live.cases.find((c) => c.name === name)!);
const invoke = (name: string) => {
  const c = pure.cases.find((c) => c.name === name)!;
  return (pure.functions[c.fn] as (...args: unknown[]) => unknown)(...c.args);
};
test('pure replay targets actual named shipping transitions with current records', () => {
  assert.equal(pure.cases.length, 40);
  assert.ok(!pure.cases.some((c) => c.fn === 'shellReducer'));
  assert.deepEqual(invoke('open-focus-first'), {
    surfaces: { search: { open: true, exclusive: undefined, props: undefined, focusRequest: 1 } },
    openMenus: [],
  });
});
test('current scenarios execute with exact named lifetime failures and no other diagnostics', async () => {
  for (const c of live.cases) {
    const r = await runScenario(c);
    assert.deepEqual(r.unmatched, [], c.name);
    const started = r.trace.flatMap((t) =>
      t.outcome && 'promise' in t.outcome ? [t.outcome.promise] : [],
    );
    const settled = r.trace.flatMap((t) => t.settled.map((s) => s.promise));
    assert.deepEqual(
      started.filter((id) => !settled.includes(id)),
      [],
      c.name + ' pending',
    );

    const error = (message: string) => ({ $error: { name: 'Error', message } });
    const expected =
      c.name === 'no-shell-without-a-document'
        ? [
            [0, error('Capability "shell" requires an active document.')],
            [2, error('Capability "shell" requires an active document.')],
            [3, error('Capability "shell" unavailable: no document "zzz".')],
          ]
        : c.name === 'per-document-state'
          ? [[19, error('Capability "shell" unavailable: no document "a".')]]
          : [];
    assert.deepEqual(
      r.trace.flatMap((t, i) => (t.outcome && 'threw' in t.outcome ? [[i, t.outcome.threw]] : [])),
      expected,
      c.name,
    );
    for (const t of r.trace) {
      assert.deepEqual(t.reports, [], c.name + ' reports');
      assert.deepEqual(
        t.settled.filter((s) => 'rejected' in s),
        [],
        c.name + ' rejections',
      );
      for (const v of [
        t.outcome && 'returned' in t.outcome ? t.outcome.returned : null,
        ...t.settled,
        ...t.callbacks,
      ])
        assert.ok(!JSON.stringify(v)?.includes('"$error"'), c.name + ' nested diagnostics');
    }
  }
});
test('focus, exclusive closure, replacement props and snapshots have literal state and events', () => {
  const ctx = createTestContext({ id: 'shell', state: model.initialShellState() });
  const api = ctx.connect(createShellController(ctx));
  try {
    const events: unknown[] = [];
    const off = [
      api.onSurfaceOpened((e) => events.push(['open', e])),
      api.onSurfaceClosed((e) => events.push(['close', e])),
    ];
    api.open('free', { focus: true, props: { keep: 1 } });
    const free = api.getSurface('free');
    const list = api.listOpenSurfaces();
    assert.equal(api.listOpenSurfaces(), list);
    api.open('left', { exclusive: 'side', focus: true, props: { tab: 1 } });
    api.open('left', { exclusive: 'side', focus: true, props: { tab: 2 } });
    assert.deepEqual(api.getSurface('left'), {
      id: 'left',
      open: true,
      exclusive: 'side',
      props: { tab: 2 },
      focusRequest: 2,
    });
    api.open('left', { exclusive: 'side', props: { replaced: true } });
    assert.equal(api.getSurface('left')?.focusRequest, 2);
    api.open('right', { exclusive: 'side', focus: true });
    assert.deepEqual(api.getSurface('left'), {
      id: 'left',
      open: false,
      exclusive: 'side',
      props: { replaced: true },
      focusRequest: 0,
    });
    assert.equal(api.getSurface('free'), free);
    const snapshot = api.getSnapshot();
    assert.equal(api.getSnapshot(), snapshot);
    assert.deepEqual(snapshot, {
      surfaces: {
        free: { open: true, exclusive: undefined, props: { keep: 1 } },
        left: { open: false, exclusive: 'side', props: { replaced: true } },
        right: { open: true, exclusive: 'side', props: undefined },
      },
      openMenus: [],
    });
    api.toggle('right', { focus: true, props: { ignored: true } });
    assert.deepEqual(api.getSurface('right'), {
      id: 'right',
      open: false,
      exclusive: 'side',
      props: undefined,
      focusRequest: 0,
    });
    api.updateSurfaceProps('right', { closed: true });
    assert.equal(api.isOpen('right'), false);
    api.toggle('right', { focus: true, props: { fresh: true } });
    assert.deepEqual(api.getSurface('right'), {
      id: 'right',
      open: true,
      exclusive: undefined,
      props: { fresh: true },
      focusRequest: 1,
    });
    api.close('right');
    api.close('right');
    api.close('unknown');
    api.open('right', { focus: true });
    api.closeAll();
    assert.deepEqual(
      [
        api.getSurface('free')?.focusRequest,
        api.getSurface('left')?.focusRequest,
        api.getSurface('right')?.focusRequest,
      ],
      [0, 0, 0],
    );
    api.applySnapshot(snapshot);
    assert.deepEqual(
      api.listOpenSurfaces().map((s) => [s.id, s.focusRequest]),
      [
        ['free', 0],
        ['right', 0],
      ],
    );
    assert.deepEqual(events, [
      ['open', { id: 'free', props: { keep: 1 } }],
      ['open', { id: 'left', props: { tab: 1 } }],
      ['close', { id: 'left' }],
      ['open', { id: 'right', props: undefined }],
      ['close', { id: 'right' }],
      ['open', { id: 'right', props: { fresh: true } }],
      ['close', { id: 'right' }],
      ['open', { id: 'right', props: undefined }],
      ['close', { id: 'free' }],
      ['close', { id: 'right' }],
      ['open', { id: 'free', props: { keep: 1 } }],
      ['open', { id: 'right', props: undefined }],
    ]);
    off.forEach((f) => f());
    api.closeAll();
    assert.equal(events.length, 12);
  } finally {
    ctx.dispose();
  }
});
import { canonical, snapshotHooks } from '../src/canonical';
const base = {
  surfaces: {
    left: { open: true, exclusive: 'side', focusRequest: 0 },
    right: { open: true, exclusive: 'side', props: { tab: 2 }, focusRequest: 0 },
    modal: { open: false, exclusive: 'overlay', focusRequest: 0 },
    '7': { open: true, focusRequest: 0 },
    free: { open: true, exclusive: '', focusRequest: 0 },
  },
  openMenus: ['file', 'edit'],
};
const expectedState = (surfaces: Record<string, unknown>, openMenus = ['file', 'edit']) => ({
  surfaces,
  openMenus,
});
const surfaceChange = (changes: Record<string, unknown>) =>
  expectedState({ ...base.surfaces, ...changes });
const empty = { surfaces: {}, openMenus: [] };
test('every pure occurrence has independently specified complete state including no-op identity', () => {
  const expected: Record<string, unknown> = {
    'initial-state': empty,
    'initial-empty-state': empty,
    'open-new': expectedState({ search: { open: true, focusRequest: 0 } }, []),
    'open-with-props': expectedState(
      {
        style: { open: true, props: { annotation: 'a1', nested: { x: [1, 2] } }, focusRequest: 0 },
      },
      [],
    ),
    'open-exclusive-closes-same-tag': surfaceChange({
      left: { ...base.surfaces.left, open: false },
      right: { ...base.surfaces.right, open: false },
      thumbs: { open: true, exclusive: 'side', focusRequest: 0 },
    }),
    'open-exclusive-keeps-itself': surfaceChange({
      left: { open: true, exclusive: 'side', props: { again: true }, focusRequest: 0 },
      right: { ...base.surfaces.right, open: false },
    }),
    'open-exclusive-other-tag': surfaceChange({
      dialog: { open: true, exclusive: 'overlay', focusRequest: 0 },
    }),
    'open-empty-exclusive-is-not-exclusive': surfaceChange({
      'other-free': { open: true, exclusive: '', focusRequest: 0 },
    }),
    'open-existing-replaces-in-place': surfaceChange({ modal: { open: true, focusRequest: 0 } }),
    'open-integer-like-id': surfaceChange({
      left: { ...base.surfaces.left, open: false },
      right: { ...base.surfaces.right, open: false },
      '3': { open: true, exclusive: 'side', focusRequest: 0 },
    }),
    'open-already-open-is-a-new-state': base,
    'close-open': surfaceChange({ right: { ...base.surfaces.right, open: false } }),
    'close-closed-is-ignored': base,
    'close-unknown-is-ignored': base,
    'close-proto-member-is-ignored': base,
    'toggle-open-closes': surfaceChange({ left: { ...base.surfaces.left, open: false } }),
    'toggle-closed-opens-exclusively': surfaceChange({
      left: { ...base.surfaces.left, open: false },
      right: { ...base.surfaces.right, open: false },
      modal: { open: true, exclusive: 'side', props: { p: 1 }, focusRequest: 0 },
    }),
    'toggle-unknown-opens': surfaceChange({ new: { open: true, focusRequest: 0 } }),
    'toggle-proto-member-opens': surfaceChange({ valueOf: { open: true, focusRequest: 0 } }),
    'open-menu': expectedState(base.surfaces, ['file', 'edit', 'view']),
    'open-menu-again-is-ignored': base,
    'close-menu': expectedState(base.surfaces, ['edit']),
    'close-menu-not-open-is-ignored': base,
    'close-all-menus': expectedState(base.surfaces, []),
    'close-all-menus-none-open-is-ignored': empty,
    'set-props-replaces-them': surfaceChange({
      right: { ...base.surfaces.right, props: { tab: 3, extra: null } },
    }),
    'set-props-on-a-closed-surface': surfaceChange({
      modal: { ...base.surfaces.modal, props: { x: 1 } },
    }),
    'set-props-unknown-is-ignored': base,
    'close-all-surfaces': expectedState({
      left: { open: false, exclusive: 'side', focusRequest: 0 },
      right: { open: false, exclusive: 'side', props: { tab: 2 }, focusRequest: 0 },
      modal: { open: false, exclusive: 'overlay', focusRequest: 0 },
      '7': { open: false, focusRequest: 0 },
      free: { open: false, exclusive: '', focusRequest: 0 },
    }),
    'close-all-surfaces-none-open-is-ignored': expectedState(
      { modal: { open: false, exclusive: 'overlay', focusRequest: 0 } },
      ['file'],
    ),
    'close-all-surfaces-empty-is-ignored': empty,
    'apply-snapshot-replaces-everything': expectedState(
      {
        '5': { open: true, focusRequest: 0 },
        search: { open: false, exclusive: 'side', props: { q: 'x' }, focusRequest: 0 },
      },
      ['help'],
    ),
    'open-focus-first': expectedState({ search: { open: true, focusRequest: 1 } }, []),
    'open-focus-repeated': expectedState(
      {
        search: { open: true, exclusive: 'side', props: { q: 'new' }, focusRequest: 3 },
        free: { open: true, focusRequest: 4, props: { keep: 1 } },
      },
      [],
    ),
    'open-without-focus-keeps-request': expectedState(
      {
        search: { open: true, focusRequest: 2 },
        free: { open: true, focusRequest: 4, props: { keep: 1 } },
      },
      [],
    ),
    'exclusive-open-clears-sibling-focus': expectedState(
      {
        search: { open: false, exclusive: 'side', props: { q: 'old' }, focusRequest: 0 },
        free: { open: true, focusRequest: 4, props: { keep: 1 } },
        other: { open: true, exclusive: 'side', focusRequest: 1 },
      },
      [],
    ),
    'toggle-focus-opens': expectedState({ search: { open: true, focusRequest: 1 } }, []),
    'toggle-focus-closes': expectedState(
      {
        search: { open: false, exclusive: 'side', props: { q: 'old' }, focusRequest: 0 },
        free: { open: true, focusRequest: 4, props: { keep: 1 } },
      },
      [],
    ),
    'snapshot-omits-focus': expectedState(
      {
        search: { open: true, exclusive: 'side', props: { q: 'old' } },
        free: { open: true, props: { keep: 1 } },
      },
      [],
    ),
    'toggle-menu-removes-open': expectedState(base.surfaces, ['edit']),
  };
  assert.equal(Object.keys(expected).length, pure.cases.length);
  const same = new Set([
    'close-closed-is-ignored',
    'close-unknown-is-ignored',
    'close-proto-member-is-ignored',
    'open-menu-again-is-ignored',
    'close-menu-not-open-is-ignored',
    'close-all-menus-none-open-is-ignored',
    'set-props-unknown-is-ignored',
    'close-all-surfaces-none-open-is-ignored',
    'close-all-surfaces-empty-is-ignored',
  ]);
  for (const c of pure.cases) {
    const next = invoke(c.name);
    assert.deepEqual(canonical(next, snapshotHooks), expected[c.name], c.name);
    if (!['initialShellState', 'snapshotOf'].includes(c.fn))
      assert.equal(next === c.args[0], same.has(c.name), c.name + ' identity');
  }
});
const returned = (t: Awaited<ReturnType<typeof run>>['trace'][number]) =>
  t.outcome && 'returned' in t.outcome ? t.outcome.returned : undefined;
test('surfaces, menus, per-document routing and wholesale snapshots have literal successful reads', async () => {
  const surfaces = await run('surfaces');
  assert.deepEqual(returned(surfaces.trace[13]), [
    { id: '0', open: true, focusRequest: 0 },
    { id: '9', open: true, focusRequest: 0 },
    { id: 'right', open: true, exclusive: 'side', props: { tab: 'layers' }, focusRequest: 0 },
  ]);
  assert.deepEqual(returned(surfaces.trace[22]), {
    id: 'right',
    open: false,
    props: { tab: 'outline' },
    focusRequest: 0,
  });
  assert.deepEqual(returned(surfaces.trace[28]), [
    { id: '0', open: true, focusRequest: 0 },
    { id: '2', open: true, focusRequest: 0 },
    { id: '9', open: true, focusRequest: 0 },
    { id: 'modal', open: true, exclusive: '', focusRequest: 0 },
  ]);
  // Keep the shipping inherited-member behavior visible, without inventing a normalized record.
  assert.deepEqual(returned(surfaces.trace[30]), { id: 'toString' });
  const menus = await run('menus');
  assert.deepEqual(
    [8, 12, 18].map((i) => returned(menus.trace[i])),
    [['file', 'edit'], ['edit', 'view'], []],
  );
  const docs = await run('per-document-state');
  assert.deepEqual(
    [5, 10, 14, 20, 22].map((i) => returned(docs.trace[i])),
    [true, false, true, true, false],
  );
  assert.deepEqual(
    [15, 17].map((i) => returned(docs.trace[i])),
    [[], [{ id: 'left', open: true, focusRequest: 0 }]],
  );
  const snap = await run('props-close-all-and-snapshots');
  assert.deepEqual(returned(snap.trace[21]), {
    surfaces: { '4': { open: true, props: { k: 'v' } }, modal: { open: true } },
    openMenus: ['view', 'edit'],
  });
  assert.deepEqual(returned(snap.trace[20]), null);
});
test('state diff events have exact payloads, order and unsubscribe silence', async () => {
  const r = await run('events');
  const cb = (fn: string, args: unknown) => ({ fn, args: [args] });
  assert.deepEqual(
    r.trace.flatMap((t, i) => t.callbacks.map((c) => [i, c])),
    [
      [6, cb('opened', { id: 'left', props: { tab: 1 } })],
      [7, cb('closed', { id: 'left' })],
      [7, cb('opened', { id: 'right' })],
      [10, cb('closed', { id: 'right' })],
      [12, cb('opened', { id: 'modal' })],
      [13, cb('closed', { id: 'modal' })],
      [14, cb('menu-opened', { id: 'file' })],
      [15, cb('menu-opened', { id: 'edit' })],
      [17, cb('opened', { id: 'gone' })],
      [17, cb('menu-closed', { id: 'file' })],
      [17, cb('menu-opened', { id: 'view' })],
      [18, cb('closed', { id: 'gone' })],
      [18, cb('menu-closed', { id: 'edit' })],
      [19, cb('menu-closed', { id: 'view' })],
    ],
  );
  const f = await run('focus-requests');
  assert.deepEqual(
    [7, 9, 11, 12, 14, 16, 21].map((i) => returned(f.trace[i])),
    [
      { id: 'left', open: true, exclusive: 'side', props: { tab: 2 }, focusRequest: 2 },
      { id: 'left', open: true, exclusive: 'side', focusRequest: 2 },
      { id: 'left', open: false, exclusive: 'side', focusRequest: 0 },
      { id: 'free', open: true, props: { keep: 1 }, focusRequest: 1 },
      { id: 'right', open: false, exclusive: 'side', focusRequest: 0 },
      { id: 'right', open: true, props: { new: true }, focusRequest: 1 },
      { id: 'free', open: true, props: { restored: true }, focusRequest: 0 },
    ],
  );
});

import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
type Saved = {
  file: string;
  kind: string;
  name: string;
  input: unknown;
  inputSha256: string;
  outcome?: unknown;
  result?: unknown;
  runnerException?: unknown;
};
const history = new URL('../historical/task-12b2c10/', import.meta.url);
const saved = (p: string): unknown => JSON.parse(readFileSync(new URL(p, history), 'utf8'));
const hash = (v: unknown) => createHash('sha256').update(JSON.stringify(v)).digest('hex');
test('prior i18n642 and actions160 commands79 link37 metadata17 and page-edit19 retain all inputs and every output channel', async () => {
  const available = [
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
  assert.equal(count, 954);
});
test('all current shell inputs and complete outcomes remain independently replayable', async () => {
  const rows = saved('current-inputs-runtime.json') as Saved[];
  assert.equal(rows.length, 47);
  assert.equal(new Set(rows.map((r) => r.file + '/' + r.name)).size, 47);
  for (const row of rows) {
    const s = suites.find((s) => s.file === row.file)!;
    const c = s.cases.find((c) => c.name === row.name)!;
    assert.deepEqual(canonical(c), row.input, row.name);
    assert.equal(hash(row.input), row.inputSha256);
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
});

test('all38 immutable original definitions and outcomes map uniquely with explicit retirement accounting', () => {
  const pins: Record<string, string> = {
    'original-runtime.json': '29d6123109c9f2dd9333890d15ce64ed3c3ffbd69d1d2dfbdf671f3487b2b901',
    'original-shell.ts.txt': '5609b0a83e560df0cf5f70a103c8c1b79a2eaadbbceb5ac9af97d2d5f5b666ee',
    'original-commands-current.test.ts.txt':
      'f776415e0c2aa52aaf1924580ee745780c1610b8c68bb1e3438efe754afe9da8',
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
      originalOutcome: unknown;
      currentFile: string;
      currentIndex: number;
      currentName: string;
      currentInput: unknown;
      currentOutcome: unknown;
    }[];
  };
  assert.deepEqual(account.summary, {
    original: 38,
    originalPure: 32,
    originalLive: 6,
    current: 47,
    currentPure: 40,
    currentLive: 7,
    mapped: 38,
    wholeRetirements: 1,
    survivingOriginals: 37,
    replacementRows: 1,
    totalNewRows: 10,
    partialRetirements: 30,
    additions: 9,
    changedInputs: 33,
  });
  const rows = saved('current-inputs-runtime.json') as Saved[];
  const mapped = new Set<string>();
  assert.equal(old.length, 38);
  for (const [i, o] of old.entries()) {
    const m = account.mappings[i];
    assert.equal(m.originalOccurrence, i);
    assert.equal(m.originalName, o.name);
    assert.deepEqual(m.originalInput, o.input);
    assert.deepEqual(m.originalOutcome, o.outcome ?? o.result ?? o.runnerException);
    const s = suites.find((s) => s.file === m.currentFile)!;
    assert.equal(s.cases[m.currentIndex].name, m.currentName);
    const c = rows.find((c) => c.file === m.currentFile && c.name === m.currentName)!;
    assert.deepEqual(m.currentInput, c.input);
    assert.deepEqual(m.currentOutcome, c.outcome ?? c.result);
    const key = m.currentFile + ':' + m.currentIndex;
    assert.ok(!mapped.has(key));
    mapped.add(key);
  }
  assert.equal(mapped.size, 38);
  assert.deepEqual(saved('task11-current-drift.json'), []);
  // Integrity of C10 artifacts only; future-owned active sources are not guarded here.
  for (const [p, want] of Object.entries(saved('integrity.json') as Record<string, string>))
    assert.equal(
      createHash('sha256')
        .update(readFileSync(new URL(p, history)))
        .digest('hex'),
      want,
      p,
    );
});
