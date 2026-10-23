process.env.TZ = 'UTC';
import assert from 'node:assert/strict';
import test from 'node:test';
import { suites } from '../src/suites/page-edit';
import { runScenario } from '../src/scenario';
test('relative rotation uses current refs and stages an actual apply transaction', async () => {
  const suite = suites[0];
  assert.equal(suite.kind, 'scenario');
  if (suite.kind !== 'scenario') throw Error('scenario');
  const r = await runScenario(suite.cases.find((c) => c.name === 'rotate-by')!);
  assert.deepEqual(r.unmatched, []);
  assert.deepEqual(ops(r)[0], { ops: [{ type: 'pages.rotate', pages: [ref(10)], rotation: 90 }] });
  for (const c of apply(r)) assert.match((c.args[1] as { opId: string }).opId, /^[0-9a-f-]{36}$/);
});
import type { ScenarioCase, TraceEntry } from '../src/scenario';
const suite = suites[0];
if (suite.kind !== 'scenario') throw Error('scenario');
const cases = suite.cases;
const run = (name: string) => runScenario(cases.find((c) => c.name === name)!);
const val = (t: TraceEntry) =>
  t.outcome && 'returned' in t.outcome ? t.outcome.returned : undefined;
const ref = (objectNumber: number) => ({ kind: 'objectNumber', objectNumber });
type ScenarioResult = Awaited<ReturnType<typeof runScenario>>;
const apply = (r: ScenarioResult) =>
  r.trace
    .flatMap((t) => t.engine)
    .filter(
      (c): c is { doc: string; path: string; args: unknown[] } =>
        typeof c === 'object' && c !== null && 'path' in c && c.path === 'apply',
    );
const ops = (r: ScenarioResult) => apply(r).map((c) => c.args[0]);
const summary = (t: TraceEntry) => {
  const p = val(t) as {
    ref: { objectNumber: number };
    rotation: number;
    index: number;
    size: unknown;
  }[];
  return p.map((p) => [p.ref.objectNumber, p.index, p.rotation]);
};
test('permission selectors keep assemble/download independent and wildcard grants both', async () => {
  const r = await run('can-edit');
  assert.deepEqual(r.trace.filter((t) => typeof val(t) === 'boolean').map(val), [
    true,
    true,
    false,
    true,
    true,
    true,
  ]);
  assert.equal(apply(r).length, 0);
});
test('relative turns see confirmed rotations and remote layouts', async () => {
  const r = await run('rotate-by');
  assert.deepEqual(
    ops(r),
    [90, 0, 180, 0, 270].map((rotation, i) => ({
      ops: [{ type: 'pages.rotate', pages: [ref([10, 10, 20, 30, 30][i])], rotation }],
    })),
  );
  assert.deepEqual(summary(r.trace[3]), [
    [10, 0, 90],
    [20, 1, 90],
    [30, 2, 270],
  ]);
  assert.deepEqual(summary(r.trace.at(-1)!), [
    [10, 0, 0],
    [20, 1, 180],
    [30, 2, 270],
  ]);
  const remote = await run('rotate-by-follows-the-registry');
  assert.deepEqual(ops(remote), [
    { ops: [{ type: 'pages.rotate', pages: [ref(10)], rotation: 270 }] },
  ]);
  assert.deepEqual(summary(remote.trace.at(-1)!), [
    [10, 0, 270],
    [20, 1, 90],
    [30, 2, 270],
  ]);
});
test('absolute rotate, reorder and delete expose actual successful order/count and void results', async () => {
  const r = await run('passthroughs');
  assert.deepEqual(ops(r), [
    { ops: [{ type: 'pages.rotate', pages: [ref(10), ref(20)], rotation: 180 }] },
    { ops: [{ type: 'pages.reorder', pages: [ref(20), ref(30)], position: 'start' }] },
    { ops: [{ type: 'pages.delete', pages: [ref(30)] }] },
  ]);
  assert.deepEqual(summary(r.trace[4]), [
    [20, 0, 180],
    [30, 1, 270],
    [10, 2, 180],
  ]);
  assert.deepEqual(summary(r.trace[6]), [
    [20, 0, 180],
    [10, 1, 180],
  ]);
  for (const i of [2, 3, 5])
    assert.deepEqual(r.trace[i].settled, [
      { promise: i === 5 ? 4 : i, resolved: { $undefined: true } },
    ]);
});
test('deferred changes send in staging order and deliver confirmed document event before promise settlement', async () => {
  const r = await run('deferred-and-aborted');
  assert.equal(r.trace[2].engine.length, 1);
  assert.equal(r.trace[3].engine.length, 1);
  assert.deepEqual(summary(r.trace[4]), [
    [10, 0, 0],
    [20, 1, 90],
    [30, 2, 270],
  ]);
  assert.deepEqual(r.trace[5].engine, []);
  assert.deepEqual(r.trace[5].settled, [{ promise: 2, resolved: { $undefined: true } }]);
  assert.deepEqual(summary(r.trace[6]), [
    [10, 0, 90],
    [20, 1, 90],
    [30, 2, 270],
  ]);
  assert.deepEqual(summary(r.trace[8]), [
    [10, 0, 90],
    [30, 1, 270],
  ]);
  assert.deepEqual(r.trace[7].settled, [{ promise: 3, resolved: { $undefined: true } }]);
  for (const i of [5, 7])
    assert.equal(r.trace[i].documentEvents.filter((e) => e.event === 'pagesChanged').length, 1);
});
test('blank defaults use actual neighbour size, preserve explicit count/size and return new refs', async () => {
  const r = await run('add-blank');
  assert.deepEqual(ops(r), [
    {
      ops: [
        {
          type: 'pages.insertBlank',
          spec: { size: { width: 500, height: 600 }, count: 1 },
          position: 'end',
        },
      ],
    },
    {
      ops: [
        {
          type: 'pages.insertBlank',
          spec: { size: { width: 500, height: 600 }, count: 1 },
          position: 'end',
        },
      ],
    },
    {
      ops: [
        {
          type: 'pages.insertBlank',
          spec: { size: { width: 300, height: 400 }, count: 1 },
          position: { after: ref(20) },
        },
      ],
    },
    {
      ops: [
        {
          type: 'pages.insertBlank',
          spec: { size: { width: 612, height: 792 }, count: 3 },
          position: { after: ref(10) },
        },
      ],
    },
  ]);
  assert.deepEqual(r.trace[5].settled, [
    { promise: 5, resolved: { pages: [ref(96), ref(95), ref(94)] } },
  ]);
  assert.deepEqual(
    summary(r.trace.at(-1)!).map((p) => p[0]),
    [10, 96, 95, 94, 20, 97, 30, 99, 98],
  );
});
test('bytes stay byte-exact; insert positions name current refs and public result names pages', async () => {
  const r = await run('insert');
  assert.deepEqual(ops(r), [
    { ops: [{ type: 'pages.insert', source: { $bytes: 'AQID' }, position: 'end' }] },
    { ops: [{ type: 'pages.insert', source: { $bytes: '' }, position: { before: ref(30) } }] },
  ]);
  assert.deepEqual(r.trace[2].settled, [{ promise: 2, resolved: { pages: [ref(77)] } }]);
  assert.deepEqual(
    summary(r.trace.at(-1)!).map((p) => p[0]),
    [10, 20, 78, 30, 77],
  );
});
test('empty registry inserts Letter; missing refs never become implicit zero rotation', async () => {
  const r = await run('an-empty-registry');
  assert.deepEqual(ops(r)[0], {
    ops: [
      {
        type: 'pages.insertBlank',
        spec: { size: { width: 612, height: 792 }, count: 1 },
        position: 'end',
      },
    ],
  });
  assert.deepEqual(
    summary(r.trace.at(-1)!).map((p) => p[0]),
    [77, 99],
  );
});
test('mixed rotations group in first-seen order within one transaction and return void', async () => {
  for (const name of ['rotate-by-groups-and-empty', 'rotate-by-a-page-without-a-rotation']) {
    const r = await run(name);
    assert.equal(apply(r).length, 1);
    const grouped = name === 'rotate-by-groups-and-empty';
    assert.deepEqual(ops(r), [
      {
        ops: [
          { type: 'pages.rotate', pages: grouped ? [ref(10), ref(40)] : [ref(10)], rotation: 90 },
          { type: 'pages.rotate', pages: [ref(20)], rotation: 180 },
          { type: 'pages.rotate', pages: [ref(30)], rotation: 0 },
        ],
      },
    ]);
    assert.deepEqual(
      summary(r.trace.at(-1)!),
      grouped
        ? [
            [10, 0, 90],
            [20, 1, 180],
            [30, 2, 0],
            [40, 3, 90],
          ]
        : [
            [10, 0, 90],
            [20, 1, 180],
            [30, 2, 0],
          ],
    );
  }
});
test('reorder retains neighbour positions and final order', async () => {
  const r = await run('move-to-the-end');
  assert.deepEqual(
    ops(r),
    ['end', { after: ref(10) }, { before: ref(30) }].map((position, i) => ({
      ops: [{ type: 'pages.reorder', pages: [ref(i === 1 ? 30 : 10)], position }],
    })),
  );
  assert.deepEqual(
    summary(r.trace.at(-1)!).map((p) => p[0]),
    [20, 10, 30],
  );
});
test('subset insertion uses scratch full scope, selected order, extraction bytes and guaranteed close', async () => {
  const r = await run('insert-a-subset-of-another-pdf');
  const calls = r.trace[2].engine;
  assert.deepEqual(calls.slice(0, 3), [
    {
      doc: null,
      path: 'open',
      args: [
        { kind: 'bytes', id: 'page-edit-source-0', bytes: { $bytes: 'AQID' } },
        { scope: ['*'] },
      ],
    },
    { doc: 'src', path: 'pages.list', args: [] },
    { doc: 'src', path: 'pages.extract', args: [[ref(7), ref(5)]] },
  ]);
  assert.deepEqual(calls[3], { doc: 'src', path: 'close', args: [] });
  assert.deepEqual(ops(r), [
    { ops: [{ type: 'pages.insert', source: { $bytes: 'BAUG' }, position: { after: ref(10) } }] },
  ]);
  assert.deepEqual(r.trace[2].settled, [{ promise: 2, resolved: { pages: [ref(77), ref(78)] } }]);
  assert.deepEqual(r.trace[3].engine, []);
  assert.deepEqual(
    r.trace[4].engine.map((c) => (c as { path?: string }).path),
    ['open', 'pages.list', 'close'],
  );
  assert.deepEqual(r.trace[5].engine, [
    {
      doc: null,
      path: 'open',
      args: [
        { kind: 'bytes', id: 'page-edit-source-0', bytes: { $bytes: 'ERET' } },
        { scope: ['*'] },
      ],
    },
  ]);
});
test('clock-named scratch sources close for each malformed source index', async () => {
  const r = await run('insert-a-subset-names-the-missing-index');
  const id = 'page-edit-source-' + (1234567890).toString(36);
  for (const t of r.trace.slice(3)) {
    assert.deepEqual((t.engine[0] as { args: unknown[] }).args, [
      { kind: 'bytes', id, bytes: { $bytes: 'AQID' } },
      { scope: ['*'] },
    ]);
    assert.deepEqual(
      (t.engine.at(-1) as { path?: string })?.path === 'apply' ? t.engine.at(-2) : t.engine.at(-1),
      { doc: 'src', path: 'close', args: [] },
    );
  }
});
test('cross-document refs resolve in source registry, bytes insert in destination and only source extracts', async () => {
  const r = await run('insert-from-another-document');
  assert.deepEqual(r.trace[3].engine[0], { doc: 'b', path: 'pages.extract', args: [[ref(20)]] });
  assert.deepEqual(r.trace[4].engine[0], { doc: 'b', path: 'pages.extract', args: [[ref(20)]] });
  assert.deepEqual(
    ops(r),
    ['end', 'start'].map((position) => ({
      ops: [{ type: 'pages.insert', source: { $bytes: 'BwgJ' }, position }],
    })),
  );
  assert.deepEqual(r.trace[5].engine, []);
  assert.deepEqual(r.trace[6].engine, []);
});
test('duplicate copies selected order after last page and extraction returns original bytes', async () => {
  const r = await run('duplicate');
  assert.deepEqual(r.trace[2].engine[0], {
    doc: 'a',
    path: 'pages.extract',
    args: [[ref(10), ref(20)]],
  });
  assert.deepEqual(
    ops(r),
    [{ after: ref(20) }, 'end'].map((position) => ({
      ops: [{ type: 'pages.insert', source: { $bytes: 'CgsM' }, position }],
    })),
  );
  assert.deepEqual(r.trace[6].settled, [{ promise: 6, resolved: { $bytes: 'CgsM' } }]);
  assert.deepEqual(
    summary(r.trace.at(-1)!).map((p) => p[0]),
    [10, 20, 77, 78, 30, 79],
  );
});
test('scope absence and captured authority after close are distinct; selection resolves indexes and deduplicates', async () => {
  const r = await run('state-and-scope');
  assert.deepEqual(
    [val(r.trace[5]), val(r.trace[7]), val(r.trace[9])],
    [[], [ref(10), ref(20)], []],
  );
  const c = await run('a-captured-capability-outlives-its-document');
  assert.equal(val(c.trace[5]), true);
  assert.equal(apply(c).length, 0);
  assert.deepEqual(c.trace[3].engineState, { a: { subscribers: 0 } });
});
import { createKernel, definePlugin, type PendingChange } from '@embedpdf/core';
import { pageEditPlugin, PageEditToken } from '../../../packages/plugin/page-edit/src/index';
import { createFakeEngine, type ResponseSpec } from '../src/fake-engine';
import {
  doc,
  three,
  page,
  pr,
  answer,
  rotated,
  reordered,
  deleted,
  inserted,
  result,
  fact,
  meta,
} from '../src/suites/page-edit-current-fixtures';
const tick = () => new Promise<void>((resolve) => setImmediate(resolve));
const harness = async (responses: ResponseSpec[] = [], allows: '*' | string[] = '*') => {
  const staged: PendingChange[] = [];
  const reports: unknown[] = [];
  const observer = definePlugin({
    id: 'page-edit-witness',
    scope: 'document',
    create: (ctx) => ({
      api: {},
      connect: () => {
        ctx.listen(ctx.changes.onStaged, (c) => staged.push(c));
      },
    }),
  });
  const fake = createFakeEngine(
    { documents: { a: doc(responses, { security: { allows } }) } },
    (v) => v,
  );
  const kernel = createKernel({
    engine: fake.engine,
    plugins: [pageEditPlugin(), observer],
    report: (e) => reports.push(e),
  });
  await kernel.start();
  await kernel.documents.open({ kind: 'bytes', id: 'a', bytes: new Uint8Array() });
  return { kernel, fake, staged, reports, api: kernel.capability(PageEditToken, 'a') };
};
const compact = (h: Awaited<ReturnType<typeof harness>>) =>
  h.kernel.documents.listPages('a').map((p) => [p.ref.objectNumber, p.rotation]);
const expectError = (code: string, message: string, permission: string | null = null) => ({
  name: 'PluginError',
  code,
  message,
  capability: 'page-edit',
  permission,
});
const reject = async (
  work: Promise<unknown>,
  code: string,
  message: string,
  permission: string | null = null,
) =>
  assert.rejects(work, (e) => {
    assert.ok(e instanceof Error);
    for (const [k, v] of Object.entries(expectError(code, message, permission)))
      assert.equal(Reflect.get(e, k), v, k);
    return true;
  });
test('two quick turns see pending rotation, retain undo predictions and label one history step each', async () => {
  const first = rotated([pr(10)], 90, [page(10, 0, 90), three[1], three[2]]);
  const second = rotated([pr(10)], 180, [page(10, 0, 180), three[1], three[2]]);
  const h = await harness([
    { path: 'apply', times: 1, defer: 'one', events: [fact(first)] },
    { path: 'apply', times: 1, defer: 'two', events: [fact(second)] },
  ]);
  try {
    const one = h.api.rotateBy([0], 90);
    const two = h.api.rotateBy([pr(10)], 90);
    await tick();
    assert.deepEqual(
      h.staged.map((c) => ({ label: c.label, shows: c.shows, undo: c.undo })),
      [
        {
          label: { key: 'pages.rotate', count: 1 },
          shows: [{ type: 'pages.rotate', pages: [pr(10)], rotation: 90 }],
          undo: [{ type: 'pages.rotate', pages: [pr(10)], rotation: 0 }],
        },
        {
          label: { key: 'pages.rotate', count: 1 },
          shows: [{ type: 'pages.rotate', pages: [pr(10)], rotation: 180 }],
          undo: [{ type: 'pages.rotate', pages: [pr(10)], rotation: 90 }],
        },
      ],
    );
    assert.deepEqual(compact(h), [
      [10, 0],
      [20, 90],
      [30, 270],
    ]);
    const order: string[] = [];
    h.kernel.documents.onPagesChanged(() => {
      order.push('fact:' + h.kernel.documents.listPages('a')[0].rotation);
    });
    one.then(() => order.push('first-resolved'));
    two.then(() => order.push('second-resolved'));
    assert.equal(h.fake.settle('one', { value: result(first) }), true);
    await one;
    await tick();
    assert.equal(h.fake.settle('two', { value: result(second) }), true);
    await two;
    await tick();
    assert.deepEqual(order, ['fact:90', 'first-resolved', 'fact:180', 'second-resolved']);
    assert.deepEqual(compact(h), [
      [10, 180],
      [20, 90],
      [30, 270],
    ]);
    assert.deepEqual(h.reports, []);
    assert.deepEqual(h.fake.unmatched, []);
  } finally {
    await h.kernel.destroy();
  }
});
test('extract waits for earlier changes to settle; labels/counts distinguish all write verbs', async () => {
  const turned = rotated([pr(10)], 90, [page(10, 0, 90), three[1], three[2]]);
  const h = await harness([
    { path: 'apply', times: 1, defer: 'turn', events: [fact(turned)] },
    { path: 'pages.extract', value: new Uint8Array([4, 5, 6]) },
    answer(inserted('pages.insert', [77], [page(10, 0, 90), three[1], three[2], page(77, 3, 90)])),
    answer(
      inserted(
        'pages.insertBlank',
        [78],
        [page(10, 0, 90), three[1], three[2], page(77, 3, 90), page(78, 4)],
      ),
    ),
    answer(
      deleted([pr(20)], [page(10, 0, 90), { ...three[2], index: 1 }, page(77, 2, 90), page(78, 3)]),
    ),
  ]);
  try {
    const turning = h.api.rotateBy([pr(10)], 90);
    const copying = h.api.duplicate([pr(10)]);
    await tick();
    assert.equal(h.fake.log.filter((c) => 'path' in c && c.path === 'pages.extract').length, 0);
    h.fake.settle('turn', { value: result(turned) });
    await turning;
    assert.deepEqual(await copying, { pages: [pr(77)] });
    await h.api.insertBlank();
    await h.api.delete([pr(20)]);
    assert.deepEqual(
      h.staged.map((c) => c.label),
      [
        { key: 'pages.rotate', count: 1 },
        { key: 'pages.duplicate', count: 1 },
        { key: 'pages.insertBlank', count: 1 },
        { key: 'pages.delete', count: 1 },
      ],
    );
    assert.deepEqual(compact(h), [
      [10, 90],
      [30, 270],
      [77, 90],
      [78, 0],
    ]);
    assert.deepEqual(h.fake.unmatched, []);
    assert.deepEqual(h.reports, []);
  } finally {
    await h.kernel.destroy();
  }
});
test('blank positions choose each anchor or edge size and explicit count/size wins', async () => {
  const rows = [
    {
      options: { position: 'start' as const },
      size: { width: 100, height: 200 },
      count: 1,
      after: [page(99, 0), ...three.map((p) => ({ ...p, index: p.index + 1 }))],
    },
    {
      options: { position: { before: pr(10) } },
      size: { width: 100, height: 200 },
      count: 1,
      after: [page(99, 0), ...three.map((p) => ({ ...p, index: p.index + 1 }))],
    },
    {
      options: { position: { before: pr(30) } },
      size: { width: 500, height: 600 },
      count: 1,
      after: [three[0], three[1], page(99, 2, 0, 500, 600), { ...three[2], index: 3 }],
    },
    {
      options: { position: { after: pr(10) } },
      size: { width: 100, height: 200 },
      count: 1,
      after: [three[0], page(99, 1), { ...three[1], index: 2 }, { ...three[2], index: 3 }],
    },
    {
      options: { count: 2 },
      size: { width: 500, height: 600 },
      count: 2,
      after: [...three, page(99, 3, 0, 500, 600), page(98, 4, 0, 500, 600)],
    },
    {
      options: { size: { width: 1, height: 2 } },
      size: { width: 1, height: 2 },
      count: 1,
      after: [...three, page(99, 3, 0, 1, 2)],
    },
  ];
  for (const row of rows) {
    const h = await harness([
      answer(inserted('pages.insertBlank', row.count === 2 ? [99, 98] : [99], row.after)),
    ]);
    try {
      assert.deepEqual(await h.api.insertBlank(row.options), {
        pages: row.count === 2 ? [pr(99), pr(98)] : [pr(99)],
      });
      assert.deepEqual(h.staged[0].shows, [
        {
          type: 'pages.insertBlank',
          spec: { size: row.size, count: row.count },
          position: 'position' in row.options ? row.options.position : 'end',
        },
      ]);
      assert.deepEqual(h.staged[0].label, { key: 'pages.insertBlank', count: row.count });
      assert.deepEqual(h.kernel.documents.listPages('a'), row.after);
      assert.deepEqual(h.reports, []);
      assert.deepEqual(h.fake.unmatched, []);
    } finally {
      await h.kernel.destroy();
    }
  }
});
test('pre-aborted calls refuse before staging, permission refusal wins and duplicate separately requires download', async () => {
  const h = await harness();
  const abort = new AbortController();
  abort.abort();
  try {
    for (const [method, work] of [
      ['rotateBy', () => h.api.rotateBy([pr(10)], 90, { signal: abort.signal })],
      ['setRotation', () => h.api.setRotation([pr(10)], 90, { signal: abort.signal })],
      ['reorder', () => h.api.reorder([pr(10)], 'end', { signal: abort.signal })],
      ['delete', () => h.api.delete([pr(10)], { signal: abort.signal })],
      ['insertBlank', () => h.api.insertBlank({ signal: abort.signal })],
      ['insertFromBytes', () => h.api.insertFromBytes(new Uint8Array(), { signal: abort.signal })],
      [
        'insertFromDocument',
        () => h.api.insertFromDocument('missing', [pr(10)], { signal: abort.signal }),
      ],
      ['duplicate', () => h.api.duplicate([pr(10)], { signal: abort.signal })],
      ['extract', () => h.api.extract([pr(10)], { signal: abort.signal })],
    ] as const)
      await reject(work(), 'operation-cancelled', `pageEdit.${method} was cancelled`);
    assert.deepEqual(h.staged, []);
    assert.deepEqual(h.fake.unmatched, []);
    assert.equal(h.fake.log.length, 3);
  } finally {
    await h.kernel.destroy();
  }
  const denied = await harness([], ['doc.pages.assemble']);
  try {
    await reject(
      denied.api.duplicate([pr(10)]),
      'permission-denied',
      'pageEdit.duplicate requires doc.download',
      'doc.download',
    );
    await reject(
      denied.api.extract([pr(10)], { signal: abort.signal }),
      'permission-denied',
      'pageEdit.extract requires doc.download',
      'doc.download',
    );
    assert.deepEqual(denied.staged, []);
  } finally {
    await denied.kernel.destroy();
  }
});
test('cancelling staged write rejects caller while the change still confirms; deleting all pages is refused', async () => {
  const turned = rotated([pr(10)], 90, [page(10, 0, 90), three[1], three[2]]);
  const h = await harness([{ path: 'apply', defer: 'turn', events: [fact(turned)] }]);
  try {
    await reject(h.api.delete([0, 1, 2]), 'invalid-input', 'a document keeps at least one page');
    const abort = new AbortController();
    const turn = h.api.rotateBy([pr(10)], 90, { signal: abort.signal });
    const check = reject(turn, 'operation-cancelled', 'operation cancelled');
    await tick();
    abort.abort();
    await check;
    assert.deepEqual(compact(h), [
      [10, 0],
      [20, 90],
      [30, 270],
    ]);
    assert.equal(h.staged.length, 1);
    h.fake.settle('turn', { value: result(turned) });
    await tick();
    assert.deepEqual(compact(h), [
      [10, 90],
      [20, 90],
      [30, 270],
    ]);
    assert.equal(h.fake.log.filter((c) => 'abort' in c).length, 0);
    assert.deepEqual(h.fake.unmatched, []);
    assert.deepEqual(h.reports, []);
  } finally {
    await h.kernel.destroy();
  }
});
test('closing rejects pending writes, removes subscribers and leaves late confirmation silent', async () => {
  const turned = rotated([pr(10)], 90, [page(10, 0, 90), three[1], three[2]]);
  const h = await harness([{ path: 'apply', defer: 'turn', events: [fact(turned)] }]);
  try {
    const turn = h.api.rotateBy([pr(10)], 90);
    const check = reject(turn, 'instance-closed', 'the document closed');
    await tick();
    const events: unknown[] = [];
    h.kernel.documents.onPagesChanged((e) => events.push(e));
    await h.kernel.documents.close('a');
    await check;
    assert.deepEqual(h.fake.snapshot(), { a: { subscribers: 0 } });
    h.fake.settle('turn', { value: result(turned) });
    await tick();
    assert.deepEqual(events, []);
    assert.deepEqual(h.reports, []);
    assert.deepEqual(h.fake.unmatched, []);
  } finally {
    await h.kernel.destroy();
  }
});
test('selection drops deleted pages by registry read and remains stable for identical resolved selections', async () => {
  const h = await harness([answer(deleted([pr(20)], [three[0], { ...three[2], index: 1 }]))]);
  try {
    h.api.select([0, 1, 1]);
    const a = h.api.getSelected();
    h.api.select([pr(10), pr(20)]);
    assert.equal(h.api.getSelected(), a);
    await h.api.delete([1]);
    assert.deepEqual(h.api.getSelected(), [pr(10)]);
    assert.deepEqual(h.reports, []);
    assert.deepEqual(h.fake.unmatched, []);
  } finally {
    await h.kernel.destroy();
  }
});
const pluginError = (
  code: string,
  message: string,
  permission: string | null = null,
  extra: Record<string, unknown> = {},
) => ({
  $error: { name: 'PluginError', message, code, capability: 'page-edit', permission, ...extra },
});
const notFound = (n: string | number) =>
  pluginError('not-found', `page ${n} is not in this document`);
const noPages = pluginError('invalid-input', 'no pages given');
const engineError = (name: string, message: string, required: string) =>
  pluginError('permission-denied', message, required, {
    details: { required, context: 'local' },
    cause: {
      $error: { name, message, code: 'Forbidden', details: { required, context: 'local' } },
    },
  });
const expectedRejections: Record<string, Record<number, unknown>> = {
  'rotate-by': { 8: notFound(99), 9: notFound(99) },
  passthroughs: { 7: noPages },
  'refused-by-the-engine': Object.fromEntries([
    ...[
      'rotateBy',
      'setRotation',
      'reorder',
      'delete',
      'insertBlank',
      'insertFromBytes',
      'extract',
      'duplicate',
    ].map((method, i) => [
      i + 3,
      pluginError(
        'permission-denied',
        `pageEdit.${method} requires ${method === 'extract' ? 'doc.download' : 'doc.pages.assemble'}`,
        method === 'extract' ? 'doc.download' : 'doc.pages.assemble',
      ),
    ]),
    [12, engineError('EngineError', 'write refused', 'doc.pages.assemble')],
  ]),
  'add-blank': { 6: notFound(199), 7: notFound(199) },
  'add-blank-fractional-index': {
    2: notFound('1.5'),
    3: notFound('NaN'),
    4: notFound('Infinity'),
    5: notFound('1.5'),
  },
  insert: { 4: notFound(5) },
  'a-captured-capability-outlives-its-document': {
    6: pluginError('instance-closed', 'the document closed'),
    7: pluginError('instance-closed', 'the document closed'),
  },
  'an-empty-registry': { 2: notFound(10), 5: notFound(10) },
  'rotate-by-groups-and-empty': { 3: noPages },
  'move-to-the-end': { 5: notFound(99) },
  'insert-a-subset-of-another-pdf': {
    3: noPages,
    4: pluginError('not-found', 'the PDF has no page 3'),
    5: engineError('EngineError', 'source open refused', 'doc.open'),
  },
  'insert-a-subset-names-the-missing-index': Object.fromEntries(
    [
      '-1',
      '1.5',
      '-0.5',
      'Infinity',
      '-Infinity',
      'NaN',
      '100000000000000000000',
      '1e+21',
      '1e-7',
    ].map((n, i) => [i + 4, pluginError('not-found', 'the PDF has no page ' + n)]),
  ),
  'insert-from-another-document': {
    5: pluginError('not-found', "document 'zzz' is not open"),
    6: pluginError('not-found', "document 'b' has no such page"),
    7: engineError('PermissionDenied', 'source download refused', 'doc.download'),
  },
  duplicate: { 4: notFound(99), 5: noPages },
};
test('every negative channel is exact and named; no unused promises, unmatched calls, diagnostics or reports are hidden', async () => {
  let negatives = 0;
  let throws = 0;
  for (const c of cases) {
    const r = await runScenario(c);
    assert.deepEqual(r.unmatched, [], c.name);
    const expected = expectedRejections[c.name] ?? {};
    const actual: Record<number, unknown> = {};
    const pending: number[] = [];
    const settled: number[] = [];
    for (const [i, t] of r.trace.entries()) {
      assert.deepEqual(t.reports, [], c.name);
      assert.deepEqual(t.callbacks, [], c.name + ' no plugin events');
      if (t.outcome && 'threw' in t.outcome) {
        throws++;
        assert.ok(
          (c.name === 'state-and-scope' && [0, 2].includes(i)) ||
            (c.name === 'a-captured-capability-outlives-its-document' && i === 4),
        );
        assert.deepEqual(t.outcome, {
          threw: {
            $error: {
              name: 'Error',
              message: 'Capability "page-edit" unavailable: no document "a".',
            },
          },
        });
      }
      if (t.outcome && 'promise' in t.outcome) pending.push(t.outcome.promise);
      if (t.outcome && 'returned' in t.outcome)
        assert.ok(
          !JSON.stringify(t.outcome.returned).includes('$error'),
          c.name + ' returned error',
        );
      for (const s of t.settled) {
        settled.push(s.promise);
        if ('rejected' in s) {
          negatives++;
          assert.ok(!(i in actual));
          actual[i] = s.rejected;
        } else
          assert.ok(
            !JSON.stringify(s.resolved).includes('$error'),
            c.name + ' resolved diagnostic',
          );
      }
    }
    assert.deepEqual(actual, expected, c.name + ' exact rejected steps/payloads');
    assert.deepEqual(
      pending.sort((a, b) => a - b),
      settled.sort((a, b) => a - b),
      c.name + ' all promises settle once',
    );
  }
  assert.equal(negatives, 42);
  assert.equal(throws, 3);
});
import { canonical, snapshotHooks } from '../src/canonical';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
type Saved = {
  file: string;
  name: string;
  kind: string;
  input: unknown;
  inputSha256: string;
  result?: unknown;
  outcome?: unknown;
};
test('prior actions160, commands79, link37 and metadata17 retain complete inputs and every output channel', async () => {
  const available = [
    ...(await import('../src/suites/actions')).suites,
    ...(await import('../src/suites/commands')).suites,
    ...(await import('../src/suites/link')).suites,
    ...(await import('../src/suites/metadata')).suites,
  ];
  let count = 0;
  for (const task of [
    'task-12b2c2',
    'task-12b2c3',
    'task-12b2c4',
    'task-12b2c5',
    'task-12b2c6',
    'task-12b2c7',
  ]) {
    const saved = JSON.parse(
      readFileSync(
        new URL('../historical/' + task + '/current-inputs-runtime.json', import.meta.url),
        'utf8',
      ),
    ) as Saved[];
    for (const row of saved) {
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
  assert.equal(count, 293);
});
test('reorder and whole-PDF insert label the exact history scope, and returned answers alone cannot change registry', async () => {
  const h = await harness([
    {
      path: 'apply',
      times: 1,
      value: result(
        reordered([pr(10)], [{ ...three[1], index: 0 }, { ...three[2], index: 1 }, page(10, 2)]),
      ),
    },
    {
      path: 'apply',
      times: 1,
      value: result(inserted('pages.insert', [77], [...three, page(77, 3)])),
    },
  ]);
  try {
    await h.api.reorder([0], 'end');
    assert.deepEqual(await h.api.insertFromBytes(new Uint8Array([1, 2, 3])), { pages: [pr(77)] });
    assert.deepEqual(
      h.staged.map((c) => c.label),
      [{ key: 'pages.reorder', count: 1 }, { key: 'pages.insert' }],
    );
    assert.deepEqual(h.kernel.documents.listPages('a'), three);
    assert.deepEqual(h.reports, []);
    assert.deepEqual(h.fake.unmatched, []);
  } finally {
    await h.kernel.destroy();
  }
});
const history = new URL('../historical/task-12b2c8/', import.meta.url);
const saved = (p: string): unknown => JSON.parse(readFileSync(new URL(p, history), 'utf8'));
const hash = (v: unknown) => createHash('sha256').update(JSON.stringify(v)).digest('hex');
test('nineteen current inputs and complete schema2 outcomes remain exact and replayable', async () => {
  const rows = saved('current-inputs-runtime.json') as Saved[];
  assert.equal(rows.length, 19);
  for (const row of rows) {
    const c = cases.find((c) => c.name === row.name)!;
    assert.deepEqual(canonical(c), row.input, row.name + ' exact input');
    assert.equal(hash(row.input), row.inputSha256);
    assert.deepEqual(await runScenario(c), row.result, row.name + ' exact complete outcome');
  }
});
test('nineteen immutable original occurrences retain full definitions/outcomes, source hashes and exact mappings', () => {
  const sources = {
    'original-page-edit.ts.txt': '36ed23435b8c209296c44a5db67c5bad1c64591f086e405ac533540d383f91cc',
    'original-runtime.json': 'fc37efb7759a30f6119dc3a03d4d98a2cabfb6b18fcefe13803fc7fe7240cd81',
    'original-commands-current.test.ts.txt':
      '4ae13e5f77960d5134fccc2739a03b7f14c6cd85598cc0e86d34f5f7d2a85877',
  };
  for (const [p, want] of Object.entries(sources))
    assert.equal(
      createHash('sha256')
        .update(readFileSync(new URL(p, history)))
        .digest('hex'),
      want,
      p,
    );
  const old = saved('original-runtime.json') as (Saved & { runnerException?: unknown })[];
  const account = saved('accounting.json') as {
    summary: unknown;
    mappings: {
      originalOccurrence: number;
      name: string;
      originalInput: unknown;
      originalOutcome: unknown;
      currentIndex: number;
    }[];
  };
  assert.deepEqual(account.summary, {
    original: 19,
    current: 19,
    mapped: 19,
    wholeRetirements: 0,
    partialRetirements: 16,
    additions: 0,
    changedInputs: 19,
  });
  for (const [i, o] of old.entries()) {
    const m = account.mappings[i];
    assert.equal(m.originalOccurrence, i);
    assert.equal(m.name, o.name);
    assert.deepEqual(m.originalInput, o.input);
    assert.deepEqual(m.originalOutcome, o.result ?? o.runnerException);
    assert.equal(cases[m.currentIndex].name, o.name);
  }
  assert.deepEqual(saved('task11-current-drift.json'), []);
  // Immutable historical artifacts only; no permanent future-source byte guard.
  for (const [p, want] of Object.entries(saved('integrity.json') as Record<string, string>))
    assert.equal(
      createHash('sha256')
        .update(readFileSync(new URL(p, history)))
        .digest('hex'),
      want,
      p,
    );
});
test('each staged fixture answer/event names its exact current request opId and full transaction order', async () => {
  for (const c of cases) {
    const r = await runScenario(c);
    const calls = apply(r);
    const answers = c.engine?.documents?.a?.responses?.filter((r) => r.path === 'apply') ?? [];
    for (const [index, a] of answers.entries()) {
      const id = (calls[index].args[1] as { opId: string }).opId;
      if (a.value) {
        const value = a.value as { meta: { opId: string }; items: { meta: { opId: string } }[] };
        assert.equal(value.meta.opId, id);
        for (const item of value.items) assert.equal(item.meta.opId, id);
      }
      for (const [i, e] of (a.events ?? []).entries()) {
        assert.ok('meta' in e && 'origin' in e);
        assert.equal(e.meta.opId, id);
        assert.deepEqual(e.origin, {
          kind: 'local',
          sessionId: 's1',
          sub: null,
          ts: 0,
          serverId: null,
          tx: { id, index: i, count: a.events!.length },
        });
      }
    }
  }
});
