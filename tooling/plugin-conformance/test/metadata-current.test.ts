process.env.TZ = 'UTC';
import assert from 'node:assert/strict';
import test from 'node:test';
import { suites } from '../src/suites/metadata';
import { runScenario } from '../src/scenario';
test('metadata opens idle, loads both mirrors on demand and returns complete current fields', async () => {
  const suite = suites[0];
  assert.ok(suite.kind === 'scenario');
  const result = await runScenario(suite.cases.find((c) => c.name === 'seeds-from-a-read')!);
  assert.deepEqual(result.unmatched, []);
  assert.deepEqual(
    result.trace
      .flatMap((t) => t.engine)
      .filter((c) => (c as { path?: string }).path?.startsWith('metadata.'))
      .map((c) => (c as { path: string }).path),
    ['metadata.get', 'metadata.custom.get'],
  );
  assert.deepEqual(result.trace.at(-1)?.outcome, { returned: { Dept: 'Ops' } });
});
import type { ScenarioCase, TraceEntry } from '../src/scenario';
const standard = (title: string | null = null) => ({
  title,
  author: null,
  subject: null,
  keywords: null,
  producer: null,
  creator: null,
  createdAt: null,
  modifiedAt: null,
  trapped: 'unknown',
});
const local = { kind: 'local', sessionId: 's1', sub: null, ts: 0, serverId: null };
const remote = { kind: 'remote', sessionId: 's2', sub: 'u-9', ts: 0, serverId: null };
const value = (t: TraceEntry) =>
  t.outcome && 'returned' in t.outcome ? t.outcome.returned : undefined;
const scenario = async (name: string) => {
  const suite = suites[0];
  assert.ok(suite.kind === 'scenario');
  return runScenario(suite.cases.find((c) => c.name === name)!);
};
const engine = (trace: TraceEntry[]) =>
  trace
    .flatMap((t) => t.engine)
    .filter((c) => (c as { path?: string }).path?.startsWith('metadata.'));
test('lazy load stays pending until both mirrors resync and loaded ensureLoaded does no extra work', async () => {
  const { trace: t } = await scenario('seeds-from-a-read');
  assert.deepEqual(t.slice(2, 6).map(value), [null, 'idle', null, 'idle']);
  assert.deepEqual(
    t
      .slice(0, 8)
      .flatMap((t) => t.engine)
      .filter((c) => (c as { path?: string }).path?.startsWith('metadata.')),
    [],
  );
  assert.deepEqual(t[8].settled, []);
  assert.deepEqual(t[10].settled, []);
  assert.deepEqual(t[10].callbacks, [
    { fn: 'onResynced', args: [{ metadata: standard('Report') }] },
  ]);
  assert.deepEqual(value(t[11]), standard('Report'));
  assert.equal(value(t[12]), null);
  assert.deepEqual(t[13].callbacks, [
    { fn: 'custom.onResynced', args: [{ custom: { Dept: 'Ops' } }] },
  ]);
  assert.deepEqual(t[13].settled, [{ promise: 2, resolved: { $undefined: true } }]);
  assert.deepEqual([value(t[14]), value(t[15])], ['ready', 'ready']);
  assert.deepEqual(engine(t), [
    { doc: 'a', path: 'metadata.get', args: [] },
    { doc: 'a', path: 'metadata.custom.get', args: [] },
  ]);
});
test('standard write has an exact patch/result and confirmed previous/keys/local origin', async () => {
  const { trace: t } = await scenario('update-writes-through-and-sets');
  assert.deepEqual(t[4].engine, [
    { doc: 'a', path: 'metadata.update', args: [{ title: 'New', author: null }] },
  ]);
  assert.deepEqual(t[4].settled, [{ promise: 3, resolved: { metadata: standard('New') } }]);
  assert.deepEqual(t[4].callbacks, [
    {
      fn: 'onUpdated',
      args: [
        { metadata: standard('New'), previous: standard(), changedKeys: ['title'], origin: local },
      ],
    },
  ]);
  assert.deepEqual(value(t[5]), standard('New'));
  assert.equal(engine(t).length, 3);
});
test('custom deferred write retains old value until confirmation and leaves standard data intact', async () => {
  const { trace: t } = await scenario('custom-write-confirms-before-settlement');
  assert.deepEqual(t[4].engine, [
    { doc: 'a', path: 'metadata.custom.update', args: [{ A: '', B: null }] },
  ]);
  assert.deepEqual(t[4].settled, []);
  assert.deepEqual(value(t[5]), { A: '1', B: '2', C: '3' });
  assert.deepEqual(t[6].callbacks, [
    {
      fn: 'custom.onUpdated',
      args: [
        {
          custom: { A: '', C: '3' },
          previous: { A: '1', B: '2', C: '3' },
          changedKeys: ['A', 'B'],
          origin: local,
        },
      ],
    },
  ]);
  assert.deepEqual(t[6].settled, [{ promise: 3, resolved: { custom: { A: '', C: '3' } } }]);
  assert.deepEqual(value(t[7]), { A: '', C: '3' });
  assert.deepEqual(value(t[8]), standard('Kept'));
});
test('unconfirmed write results cannot mutate the mirror or emit updated', async () => {
  const { trace: t } = await scenario('standard-write-does-not-project-unconfirmed-result');
  assert.deepEqual(t[4].settled, [{ promise: 3, resolved: { metadata: standard('Unconfirmed') } }]);
  assert.deepEqual(t[4].callbacks, []);
  assert.deepEqual(value(t[5]), standard('Kept'));
  assert.deepEqual(value(t[7]), standard('Confirmed'));
  assert.deepEqual(t[6].callbacks, [
    {
      fn: 'onUpdated',
      args: [
        {
          metadata: standard('Confirmed'),
          previous: standard('Kept'),
          changedKeys: ['title'],
          origin: local,
        },
      ],
    },
  ]);
});
test('loads emit only resynced; facts keep both origins, shallow keys, and unsubscribe silence', async () => {
  const { trace: t } = await scenario('on-updated-reports-each-confirmed-change');
  assert.deepEqual(t[6].callbacks, [
    { fn: 'resynced', args: [{ metadata: standard('Seed') }] },
    { fn: 'customResynced', args: [{ custom: { A: '1', B: '2' } }] },
  ]);
  assert.deepEqual(t[7].callbacks, [
    {
      fn: 'updated',
      args: [
        {
          metadata: standard('Local'),
          previous: standard('Seed'),
          changedKeys: ['title'],
          origin: local,
        },
      ],
    },
  ]);
  assert.deepEqual(t[8].callbacks, [
    {
      fn: 'customUpdated',
      args: [
        {
          custom: { B: '2', A: '1' },
          previous: { A: '1', B: '2' },
          changedKeys: [],
          origin: remote,
        },
      ],
    },
  ]);
  assert.deepEqual(t[9].callbacks, [
    {
      fn: 'updated',
      args: [
        {
          metadata: standard('Local'),
          previous: standard('Local'),
          changedKeys: [],
          origin: remote,
        },
      ],
    },
  ]);
  assert.deepEqual(t[10].callbacks, [
    {
      fn: 'customUpdated',
      args: [
        {
          custom: { B: '3', C: '' },
          previous: { B: '2', A: '1' },
          changedKeys: ['B', 'A', 'C'],
          origin: remote,
        },
      ],
    },
  ]);
  assert.deepEqual(
    t.slice(12).flatMap((t) => t.callbacks),
    [],
  );
  assert.deepEqual(value(t[16]), standard('Unheard'));
  assert.deepEqual(value(t[17]), { Unheard: '1' });
});
test('custom comparison preserves equal/changed values and witnesses added and removed keys', async () => {
  const { trace: t } = await scenario('changed-keys-beyond-the-info-dict');
  assert.deepEqual(t[4].callbacks, [
    {
      fn: 'updated',
      args: [
        {
          custom: { pages: '3', ratio: '2.5', extra: 'object', list: 'array' },
          previous: { pages: '3', ratio: '1.5', extra: 'object', list: 'array' },
          changedKeys: ['ratio'],
          origin: local,
        },
      ],
    },
  ]);
  assert.deepEqual(t[5].callbacks, [
    {
      fn: 'updated',
      args: [
        {
          custom: { pages: '4', ratio: '2.5', extra: 'x', list: 'y' },
          previous: { pages: '3', ratio: '2.5', extra: 'object', list: 'array' },
          changedKeys: ['pages', 'extra', 'list'],
          origin: local,
        },
      ],
    },
  ]);
  assert.deepEqual(t[6].callbacks, [
    {
      fn: 'updated',
      args: [
        {
          custom: { ratio: '2.5', extra: '', added: 'yes' },
          previous: { pages: '4', ratio: '2.5', extra: 'x', list: 'y' },
          changedKeys: ['pages', 'extra', 'list', 'added'],
          origin: remote,
        },
      ],
    },
  ]);
});
test('concurrent loads deduplicate and explicit in-flight refresh queues exactly one re-read', async () => {
  const { trace: t } = await scenario('status-while-the-seed-read-is-in-flight');
  assert.equal(value(t[2]), 'idle');
  assert.equal(value(t[4]), 'loading');
  assert.deepEqual(t[5].engine, []);
  assert.deepEqual(t[6].engine, []);
  assert.deepEqual(engine(t), [
    { doc: 'a', path: 'metadata.get', args: [] },
    { doc: 'a', path: 'metadata.custom.get', args: [] },
    { doc: 'a', path: 'metadata.get', args: [] },
  ]);
  assert.deepEqual(
    t.slice(3, 11).flatMap((t) => t.settled),
    [],
  );
  assert.deepEqual(t[11].settled, [
    { promise: 4, resolved: { $undefined: true } },
    { promise: 2, resolved: { $undefined: true } },
    { promise: 3, resolved: { $undefined: true } },
  ]);
  assert.deepEqual(value(t[12]), standard('Refreshed'));
  assert.equal(value(t[13]), 'ready');
});
test('both newer confirmed values win over stale load answers', async () => {
  const { trace: t } = await scenario('an-event-during-the-seed-read-wins');
  assert.deepEqual(
    [value(t[7]), value(t[8]), value(t[9]), value(t[10])],
    [standard('Event'), 'ready', { Dept: 'New' }, 'ready'],
  );
  assert.equal(engine(t).length, 2);
});
test('close suppresses late reads and all callbacks; reopen starts fresh idle mirrors', async () => {
  const { trace: t } = await scenario('closing-during-the-seed-read');
  assert.deepEqual(
    t.flatMap((t) => t.callbacks),
    [],
  );
  assert.deepEqual(t[5].settled, [
    { promise: 2, resolved: { $undefined: true } },
    { promise: 3, resolved: { $undefined: true } },
  ]);
  assert.deepEqual(t.slice(9).map(value), [null, null, 'idle', 'idle']);
  const live = await scenario('live-updates-from-the-event-stream');
  assert.deepEqual(
    [
      value(live.trace[4]),
      value(live.trace[6]),
      value(live.trace[8]),
      value(live.trace[11]),
      value(live.trace[12]),
      value(live.trace[14]),
    ],
    [
      standard('Remote edit'),
      { Dept: 'Ops' },
      standard('Remote edit'),
      null,
      'idle',
      standard('Seed'),
    ],
  );
  assert.deepEqual(live.trace[9].documentEvents, [
    { event: 'activeChanged', value: { documentId: null, previousDocumentId: 'a' } },
    { event: 'closed', value: { documentId: 'a' } },
  ]);
});
test('permissions are synchronous selectors and local refusal never reaches either write service', async () => {
  const allowed = await scenario('can-edit');
  assert.deepEqual(allowed.trace.slice(4).map(value), [true, false, true]);
  assert.deepEqual(engine(allowed.trace), []);
  const { trace: t } = await scenario('update-without-the-capability-is-refused-locally');
  assert.equal(engine(t).length, 2);
  assert.deepEqual(t.slice(5).map(value), [standard('Kept'), 'ready', { Dept: 'Ops' }, 'ready']);
});
test('failed reads/refusals preserve snapshots and refresh recovers each independent mirror', async () => {
  const failed = await scenario('a-failed-seed-read-is-ignored');
  assert.deepEqual(failed.trace.slice(3).map(value), [null, 'error', 'ready']);
  const denied = await scenario('a-forbidden-seed-read');
  assert.deepEqual(
    [
      value(denied.trace[3]),
      value(denied.trace[4]),
      value(denied.trace[6]),
      value(denied.trace[8]),
      value(denied.trace[9]),
    ],
    ['forbidden', null, 'error', 'ready', standard('Back')],
  );
  const refused = await scenario('update-refused');
  assert.deepEqual(refused.trace.slice(4).map(value), [standard('Kept'), 'ready']);
  const reload = await scenario('reload');
  assert.deepEqual(
    [
      value(reload.trace[5]),
      value(reload.trace[7]),
      value(reload.trace[8]),
      value(reload.trace[9]),
    ],
    [standard('Second'), standard('Second'), 'error', 'ready'],
  );
  assert.deepEqual(reload.trace[4].callbacks, [
    { fn: 'onResynced', args: [{ metadata: standard('Second') }] },
  ]);
  const custom = await scenario('custom-refresh-and-refusal');
  assert.deepEqual(
    [
      value(custom.trace[5]),
      value(custom.trace[7]),
      value(custom.trace[8]),
      value(custom.trace[9]),
    ],
    [{ A: '2' }, { A: '2' }, 'ready', standard('Kept')],
  );
  assert.deepEqual(custom.trace[4].callbacks, [
    { fn: 'custom.onResynced', args: [{ custom: { A: '2' } }] },
  ]);
});
const pluginError = (
  message: string,
  code: string,
  cause?: unknown,
  permission: string | null = null,
  details?: unknown,
) => ({
  $error: {
    name: 'PluginError',
    message,
    code,
    capability: 'metadata',
    permission,
    ...(cause ? { cause } : {}),
    ...(details ? { details } : {}),
  },
});
const engineError = (message: string, code: string) => ({
  $error: { name: 'EngineError', message, code },
});
const forbidden = (required: string) =>
  pluginError(
    `permission denied (local): ${required}`,
    'permission-denied',
    {
      $error: {
        name: 'PermissionDenied',
        message: `permission denied (local): ${required}`,
        required,
        context: 'local',
        code: 'Forbidden',
        details: { required, context: 'local' },
      },
    },
    required,
    { required, context: 'local' },
  );
const negatives: Record<string, readonly [number, unknown][]> = {
  'a-failed-seed-read-is-ignored': [
    [2, pluginError('closed', 'operation-failed', engineError('closed', 'DocClosed'))],
  ],
  'update-refused': [[3, forbidden('doc.metadata.modify')]],
  reload: [[6, pluginError('gone', 'operation-failed', engineError('gone', 'DocClosed'))]],
  'update-without-the-capability-is-refused-locally': [
    [
      3,
      pluginError(
        'metadata.update requires doc.metadata.modify',
        'permission-denied',
        undefined,
        'doc.metadata.modify',
      ),
    ],
    [
      4,
      pluginError(
        'metadata.custom.update requires doc.metadata.modify',
        'permission-denied',
        undefined,
        'doc.metadata.modify',
      ),
    ],
  ],
  'a-forbidden-seed-read': [
    [2, forbidden('doc.metadata.read')],
    [5, pluginError('gone', 'operation-failed', engineError('gone', 'Unknown'))],
  ],
  'custom-refresh-and-refusal': [
    [6, pluginError('gone', 'operation-failed', engineError('gone', 'Unknown'))],
  ],
};
test('every negative channel is an exact named rejection; all other throws, reports and error data fail', async () => {
  const suite = suites[0];
  assert.ok(suite.kind === 'scenario');
  for (const c of suite.cases) {
    const r = await runScenario(c);
    assert.deepEqual(r.unmatched, [], c.name);
    const actual: unknown[] = [];
    for (const [i, t] of r.trace.entries()) {
      assert.ok(!t.outcome || !('threw' in t.outcome), c.name + ' throw ' + i);
      assert.deepEqual(t.reports, [], c.name + ' reports ' + i);
      for (const s of t.settled) if ('rejected' in s) actual.push([i, s.rejected]);
      assert.doesNotMatch(
        JSON.stringify([
          t.outcome && 'returned' in t.outcome ? t.outcome.returned : null,
          t.callbacks,
          t.settled.filter((s) => 'resolved' in s).map((s) => s.resolved),
        ]),
        /"\$error"|"diagnostics":\[[^\]]|"errors":\[[^\]]|"threw":|"rejected":/,
        c.name + ' returned/settled/callback diagnostic ' + i,
      );
    }
    assert.deepEqual(actual, negatives[c.name] ?? [], c.name + ' exact negatives');
    const returnedPromises = r.trace.flatMap((t) =>
      t.outcome && 'promise' in t.outcome ? [t.outcome.promise] : [],
    );
    const settledPromises = r.trace.flatMap((t) => t.settled.map((s) => s.promise));
    assert.deepEqual(
      returnedPromises.sort((left, right) => left - right),
      settledPromises.sort((left, right) => left - right),
      c.name + ' every promise settles exactly once',
    );
  }
});
import { createKernel } from '@embedpdf/core';
import { metadataPlugin, MetadataToken } from '../../../packages/plugin/metadata/src/index';
import { createFakeEngine } from '../src/fake-engine';
import {
  doc,
  reads,
  meta,
  result,
  customResult,
  fact,
  customFact,
  local as typedLocal,
} from '../src/suites/metadata-current-fixtures';
const harness = async (responses: Parameters<typeof doc>[0], allows: '*' | string[] = '*') => {
  const fake = createFakeEngine(
    { documents: { a: doc(responses, { security: { allows } }) } },
    (v) => v,
  );
  const reports: unknown[] = [];
  const kernel = createKernel({
    engine: fake.engine,
    plugins: [metadataPlugin()],
    report: (e) => reports.push(e),
  });
  await kernel.start();
  await kernel.documents.open({ kind: 'bytes', id: 'a', bytes: new Uint8Array() });
  return { fake, kernel, reports, api: kernel.capability(MetadataToken, 'a') };
};
test('confirmed standard/custom callbacks see new snapshots before their immediate/deferred writes resolve', async () => {
  const h = await harness([
    ...reads(),
    {
      path: 'metadata.update',
      args: [{ title: 'New' }],
      value: result(meta({ title: 'New' })),
      events: [fact(meta({ title: 'New' }), typedLocal)],
    },
    {
      path: 'metadata.custom.update',
      args: [{ A: '1' }],
      defer: 'write',
      events: [customFact({ A: '1' }, typedLocal)],
    },
  ]);
  try {
    await h.api.ensureLoaded();
    const order: string[] = [];
    h.api.onUpdated((e) => {
      assert.deepEqual(h.api.getSnapshot(), standard('New'));
      assert.deepEqual(e, {
        metadata: standard('New'),
        previous: standard(),
        changedKeys: ['title'],
        origin: local,
      });
      order.push('standard-event');
    });
    h.api.custom.onUpdated((e) => {
      assert.deepEqual(h.api.custom.getSnapshot(), { A: '1' });
      assert.deepEqual(e, { custom: { A: '1' }, previous: {}, changedKeys: ['A'], origin: local });
      order.push('custom-event');
    });
    assert.deepEqual(
      await h.api.update({ title: 'New' }).then((r) => {
        order.push('standard-resolved');
        return r;
      }),
      { metadata: standard('New') },
    );
    const pending = h.api.custom.update({ A: '1' }).then((r) => {
      order.push('custom-resolved');
      return r;
    });
    assert.deepEqual(h.api.custom.getSnapshot(), {});
    assert.deepEqual(order, ['standard-event', 'standard-resolved']);
    h.fake.settle('write', { value: customResult({ A: '1' }) });
    assert.deepEqual(await pending, { custom: { A: '1' } });
    assert.deepEqual(order, [
      'standard-event',
      'standard-resolved',
      'custom-event',
      'custom-resolved',
    ]);
    assert.deepEqual(h.reports, []);
    assert.deepEqual(h.fake.unmatched, []);
  } finally {
    await h.kernel.destroy();
  }
});
test('pre-aborted writes refuse locally and permission refusal precedes cancellation for both surfaces', async () => {
  const abort = new AbortController();
  abort.abort();
  for (const allows of ['*', []] as const) {
    const h = await harness([], allows === '*' ? '*' : []);
    try {
      for (const [verb, write] of [
        ['metadata.update', () => h.api.update({ title: 'x' }, { signal: abort.signal })],
        ['metadata.custom.update', () => h.api.custom.update({ A: 'x' }, { signal: abort.signal })],
      ] as const) {
        await assert.rejects(write, {
          name: 'PluginError',
          capability: 'metadata',
          code: allows === '*' ? 'operation-cancelled' : 'permission-denied',
          message:
            allows === '*' ? `${verb} was cancelled` : `${verb} requires doc.metadata.modify`,
        });
      }
      assert.deepEqual(
        h.fake.log.filter((c) => 'path' in c && c.path.startsWith('metadata.')),
        [],
      );
      assert.equal(h.api.getStatus(), 'idle');
      assert.equal(h.api.custom.getStatus(), 'idle');
      assert.deepEqual(h.reports, []);
    } finally {
      await h.kernel.destroy();
    }
  }
});
test('cancelled lazy waiter rejects while a second waiter receives both eventual reads', async () => {
  const h = await harness([
    { path: 'metadata.get', args: [], defer: 'standard' },
    { path: 'metadata.custom.get', args: [], defer: 'custom' },
  ]);
  try {
    const abort = new AbortController();
    const cancelled = h.api.ensureLoaded({ signal: abort.signal });
    const kept = h.api.ensureLoaded();
    const rejection = assert.rejects(cancelled, {
      name: 'PluginError',
      capability: 'metadata',
      code: 'operation-cancelled',
      message: 'operation cancelled',
    });
    abort.abort();
    await rejection;
    assert.equal(h.api.getStatus(), 'loading');
    assert.equal(h.api.custom.getStatus(), 'loading');
    h.fake.settle('standard', { value: meta({ title: 'Still loaded' }) });
    h.fake.settle('custom', { value: { A: '1' } });
    await kept;
    assert.deepEqual(h.api.getSnapshot(), standard('Still loaded'));
    assert.deepEqual(h.api.custom.getSnapshot(), { A: '1' });
    assert.deepEqual(
      h.fake.log.filter((c) => 'path' in c && c.path.startsWith('metadata.')),
      [
        { doc: 'a', path: 'metadata.get', args: [] },
        { doc: 'a', path: 'metadata.custom.get', args: [] },
      ],
    );
    assert.deepEqual(h.reports, []);
  } finally {
    await h.kernel.destroy();
  }
});
test('caller cancellation rejects both deferred writes without projecting their unconfirmed results', async () => {
  const h = await harness([
    ...reads(meta({ title: 'Kept' }), { A: '1' }),
    { path: 'metadata.update', args: [{ title: 'Late' }], defer: 'standard' },
    { path: 'metadata.custom.update', args: [{ A: '2' }], defer: 'custom' },
  ]);
  try {
    await h.api.ensureLoaded();
    const events: unknown[] = [];
    h.api.onUpdated((e) => events.push(e));
    h.api.custom.onUpdated((e) => events.push(e));
    for (const [label, write, value] of [
      [
        'standard',
        (signal: AbortSignal) => h.api.update({ title: 'Late' }, { signal }),
        result(meta({ title: 'Late' })),
      ],
      [
        'custom',
        (signal: AbortSignal) => h.api.custom.update({ A: '2' }, { signal }),
        customResult({ A: '2' }),
      ],
    ] as const) {
      const abort = new AbortController();
      const rejection = assert.rejects(write(abort.signal), {
        name: 'PluginError',
        capability: 'metadata',
        code: 'operation-cancelled',
        message: 'operation cancelled',
      });
      abort.abort();
      await rejection;
      h.fake.settle(label, { value });
    }
    assert.deepEqual(h.api.getSnapshot(), standard('Kept'));
    assert.deepEqual(h.api.custom.getSnapshot(), { A: '1' });
    assert.deepEqual(events, []);
    assert.deepEqual(h.reports, []);
    assert.deepEqual(h.fake.unmatched, []);
  } finally {
    await h.kernel.destroy();
  }
});
test('closing rejects in-flight writes and releases subscriptions before late confirmations', async () => {
  const h = await harness([
    ...reads(),
    {
      path: 'metadata.update',
      args: [{ title: 'Late' }],
      defer: 'write',
      events: [fact(meta({ title: 'Late' }), typedLocal)],
    },
  ]);
  try {
    await h.api.ensureLoaded();
    const events: unknown[] = [];
    h.api.onUpdated((e) => events.push(e));
    const rejection = assert.rejects(h.api.update({ title: 'Late' }), {
      name: 'PluginError',
      capability: 'metadata',
      code: 'instance-closed',
      message: /document instance a#\d+ was closed/,
    });
    await h.kernel.documents.close('a');
    await rejection;
    assert.equal(h.fake.snapshot().a.subscribers, 0);
    h.fake.settle('write', { value: result(meta({ title: 'Late' })) });
    await Promise.resolve();
    assert.deepEqual(events, []);
    assert.deepEqual(h.reports, []);
  } finally {
    await h.kernel.destroy();
  }
});

import { canonical, snapshotHooks } from '../src/canonical';
import { readFileSync } from 'node:fs';
type Saved = {
  file: string;
  name: string;
  input: unknown;
  inputSha256: string;
  result?: unknown;
  outcome?: unknown;
};
test('prior current actions160, commands79 and link37 keep their complete actual inputs and every output channel', async () => {
  const { suites: actions } = await import('../src/suites/actions');
  const { suites: commands } = await import('../src/suites/commands');
  const { suites: links } = await import('../src/suites/link');
  const available = [...actions, ...commands, ...links];
  let count = 0;
  for (const task of ['task-12b2c2', 'task-12b2c3', 'task-12b2c4', 'task-12b2c5', 'task-12b2c6']) {
    const prior = JSON.parse(
      readFileSync(
        new URL('../historical/' + task + '/current-inputs-runtime.json', import.meta.url),
        'utf8',
      ),
    ) as Saved[];
    for (const row of prior) {
      count++;
      const s = available.find((s) => s.file === row.file)!;
      const c = s.cases.find((c) => c.name === row.name)!;
      assert.deepEqual(canonical(c), row.input, row.name + ' prior input');
      if (s.kind === 'scenario') {
        assert.ok('steps' in c);
        assert.deepEqual(await runScenario(c), row.result, row.name + ' prior full outcome');
      } else {
        assert.ok('fn' in c);
        assert.deepEqual(
          {
            returned: canonical(
              (s.functions[c.fn] as (...args: unknown[]) => unknown)(...c.args),
              snapshotHooks,
            ),
          },
          row.outcome,
          row.name + ' prior pure',
        );
      }
    }
  }
  assert.equal(count, 276);
});

import { createHash } from 'node:crypto';
const history = new URL('../historical/task-12b2c7/', import.meta.url);
const read = (p: string): unknown => JSON.parse(readFileSync(new URL(p, history), 'utf8'));
const hash = (v: unknown) => createHash('sha256').update(JSON.stringify(v)).digest('hex');
test('all current metadata inputs and complete schema2 outcomes remain exact', async () => {
  const suite = suites[0];
  assert.ok(suite.kind === 'scenario');
  const saved = read('current-inputs-runtime.json') as Saved[];
  assert.equal(saved.length, 17);
  for (const row of saved) {
    const definition: ScenarioCase = suite.cases.find((entry) => entry.name === row.name)!;
    assert.deepEqual(canonical(definition), row.input, row.name + ' input');
    assert.equal(hash(row.input), row.inputSha256);
    assert.deepEqual(await runScenario(definition), row.result, row.name + ' complete outcome');
  }
});
test('fourteen immutable occurrences retain full definitions, outcomes and one current mapping each', () => {
  const old = read('original-runtime.json') as Saved[];
  const account = read('accounting.json') as {
    summary: unknown;
    mappings: {
      originalOccurrence: number;
      name: string;
      originalInput: unknown;
      originalOutcome: unknown;
      originalInputSha256: string;
      currentInputSha256: string;
    }[];
  };
  const current = read('current-inputs-runtime.json') as Saved[];
  assert.deepEqual(account.summary, {
    original: 14,
    current: 17,
    mapped: 14,
    wholeRetirements: 0,
    partialRetirements: 10,
    additions: 3,
    changedInputs: 14,
  });
  assert.equal(old.length, 14);
  assert.equal(account.mappings.length, 14);
  for (const [i, row] of old.entries()) {
    const m = account.mappings[i];
    assert.equal(m.originalOccurrence, i);
    assert.equal(m.name, row.name);
    assert.deepEqual(m.originalInput, row.input);
    assert.equal(m.originalInputSha256, hash(row.input));
    assert.equal(m.currentInputSha256, hash(current.find((c) => c.name === row.name)!.input));
    assert.deepEqual(
      m.originalOutcome,
      'result' in row
        ? row.result
        : (row as unknown as { runnerException: unknown }).runnerException,
    );
  }
  assert.equal(new Set(current.map((c) => c.name)).size, 17);
  assert.deepEqual(read('task11-current-drift.json'), []);
});
test('immutable source and runtime evidence retain fixed hashes without guarding future active owners', () => {
  assert.equal(
    createHash('sha256')
      .update(readFileSync(new URL('original-runtime.json', history)))
      .digest('hex'),
    '35194be8e51fe096943d643861e83d4b42a00de7f66b2b3d8b3016e3f7471370',
  );
  for (const [path, want] of Object.entries(read('integrity.json') as Record<string, string>))
    assert.equal(
      createHash('sha256')
        .update(readFileSync(new URL(path, history)))
        .digest('hex'),
      want,
      path,
    );
  assert.equal(
    createHash('sha256')
      .update(readFileSync(new URL('original-metadata.ts.txt', history)))
      .digest('hex'),
    '0c0d6ea232daf0b94904b8191dfd4f315fde19455cf7ee127fbfec11037c7d26',
  );
  assert.equal(
    createHash('sha256')
      .update(readFileSync(new URL('base-commands-current.test.ts.txt', history)))
      .digest('hex'),
    '2fc0a915a29e6d381eb3521dc65804f7e061b9ab4bc0418e3a3e9bc9c468a0af',
  );
});
