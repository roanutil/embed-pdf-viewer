// Prior reviewed action evidence fixes its script environment to UTC.
process.env.TZ = 'UTC';
import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { suites } from '../src/suites/link';
test('current links replace the removed cache reducer and use the current annotation host', () => {
  const source = readFileSync(new URL('../src/suites/link.ts', import.meta.url), 'utf8');
  assert.doesNotMatch(source, /removedUpstream|from '.\/annotation-fixtures'/);
  assert.match(source, /annotation-live-fixtures/);
  assert.match(source, /resolveLinkTarget/);
  assert.equal(
    suites.filter((s) => s.kind === 'scenario').reduce((n, s) => n + s.cases.length, 0),
    29,
  );
});
import { runScenario, type TraceEntry } from '../src/scenario';
import { canonical, snapshotHooks } from '../src/canonical';
import { pr, objRef, link, doc } from '../src/suites/annotation-live-fixtures';
const object = (v: unknown): Record<string, unknown> => {
  assert.ok(v !== null && typeof v === 'object' && !Array.isArray(v));
  return v as Record<string, unknown>;
};
const scenario = async (name: string) => {
  const s = suites.find((s) => s.kind === 'scenario' && s.cases.some((c) => c.name === name));
  assert.ok(s && s.kind === 'scenario');
  return runScenario(s.cases.find((c) => c.name === name)!);
};
const engineCalls = (t: TraceEntry) => t.engine.map(object);
const firstArg = (call: Record<string, unknown>) => {
  assert.ok(Array.isArray(call.args));
  return call.args[0];
};
const value = (t: TraceEntry): unknown =>
  t.outcome && 'returned' in t.outcome ? t.outcome.returned : undefined;
const ids = (v: unknown) => {
  assert.ok(Array.isArray(v));
  return v.map((x) => object(x).id);
};
const pluginError = (message: string, code: string, cause?: unknown) => ({
  $error: {
    name: 'PluginError',
    message,
    code,
    capability: 'link',
    permission: null,
    ...(cause ? { cause } : {}),
  },
});
const refused = (message = 'refused') =>
  pluginError(message, 'operation-failed', {
    $error: { name: 'EngineError', message, code: 'Refused' },
  });
const noPage = (n: number) => pluginError(`page ${n} is not in this document`, 'not-found');
const actionResult = (type: string) => ({
  status: 'executed',
  nodes: [{ path: [], type, status: 'executed' }],
  diagnostics: [],
});
const noAdapter = {
  status: 'inert',
  nodes: [{ path: [], type: 'uri', status: 'no-executor' }],
  diagnostics: [{ code: 'no-adapter', message: 'uri: no UI adapter installed' }],
};
test('viewer cache filters visible page-space records, keeps refs and copies activation/hover/reply fields', async () => {
  const r = await scenario('viewer-only-page-cache');
  assert.deepEqual(value(r.trace[2]), []);
  const items = value(r.trace[4]);
  assert.deepEqual(ids(items), [
    'obj:1',
    'base:1:2',
    'base:1:3',
    'obj:9',
    'obj:10',
    'obj:11',
    'obj:12',
    'obj:13',
    'obj:14',
    'obj:15',
    'obj:16',
    'obj:17',
  ]);
  assert.ok(Array.isArray(items));
  assert.deepEqual(object(items[0]).bounds, { x: 100, y: 80, width: 100, height: 20 });
  assert.deepEqual(object(items[1]).ref, { kind: 'baseIndex', page: pr(1), baseIndex: 2 });
  assert.deepEqual(object(items[5]).hoverEvents, { enter: true, exit: false });
  assert.deepEqual(object(items[6]).hoverEvents, { enter: false, exit: true });
  assert.equal(object(items[7]).hoverEvents, undefined);
  assert.equal(object(items[9]).attached, true);
  assert.equal(object(items[10]).attached, false);
  assert.deepEqual(object(object(items[4]).activate).root, {
    type: 'uri',
    subtype: 'URI',
    uri: 'https://a.example',
    isMap: false,
    next: [],
  });
  assert.deepEqual(object((value(r.trace[7]) as unknown[])[0]).bounds, {
    x: 20,
    y: 430,
    width: 100,
    height: 50,
  });
  assert.deepEqual(r.trace[5].engine, []);
  assert.deepEqual(
    r.trace
      .flatMap(engineCalls)
      .filter((x) => x.path === 'page.annotations.list')
      .map((x) => [x.pon, x.args]),
    [
      [1, []],
      [2, []],
    ],
  );
});
test('in-flight requests share the engine read and both settle before a refused page retries', async () => {
  const r = await scenario('viewer-only-loads-in-flight');
  assert.deepEqual(r.trace[2].settled, []);
  assert.deepEqual(r.trace[3].settled, []);
  assert.deepEqual(r.trace[3].engine, []);
  assert.deepEqual(r.trace[4].settled, [
    { promise: 2, resolved: { $undefined: true } },
    { promise: 3, resolved: { $undefined: true } },
  ]);
  assert.deepEqual(ids(value(r.trace[5])), ['obj:1', 'obj:2']);
  assert.deepEqual(value(r.trace[7]), []);
  assert.deepEqual(ids(value(r.trace[9])), ['obj:20']);
  assert.deepEqual(
    r.trace
      .flatMap(engineCalls)
      .filter((x) => x.path === 'page.annotations.list')
      .map((x) => x.pon),
    [1, 2, 2],
  );
});
test('six named read refusals expose error then retry to ready with typed link records', async () => {
  const r = await scenario('viewer-only-malformed-lists');
  for (let n = 1; n <= 6; n++) {
    const at = 2 + (n - 1) * 5;
    assert.equal(value(r.trace[at + 1]), 'error');
    assert.equal(value(r.trace[at + 3]), 'ready');
    assert.deepEqual(ids(value(r.trace[at + 4])), [`obj:${n}`]);
  }
});
test('current annotation events refresh loaded pages in order and document close removes subscribers', async () => {
  const r = await scenario('viewer-only-refetch-on-events');
  assert.deepEqual(
    r.trace.map((t) =>
      engineCalls(t)
        .filter((x) => x.path === 'page.annotations.list')
        .map((x) => x.pon),
    ),
    [[], [], [1], [1], [1], [1], [1], [1], [], [], [], [], []],
  );
  assert.deepEqual(value(r.trace[11]), []);
  assert.deepEqual(r.trace.at(-1)?.engineState, { a: { subscribers: 0 } });
});
test('annotation-backed links follow remote facts and confirmed local apply updates and deletions without page reads', async () => {
  const r = await scenario('annotation-backed-source');
  assert.deepEqual(ids(value(r.trace[2])), ['obj:1', 'obj:3']);
  assert.deepEqual(ids(value(r.trace[7])), ['obj:1', 'obj:3', 'obj:5']);
  assert.equal(
    object(object((value(r.trace[9]) as unknown[])[0]).target).uri,
    'https://updated.example',
  );
  assert.deepEqual(ids(value(r.trace[11])), ['obj:3', 'obj:5']);
  assert.deepEqual(ids(value(r.trace[13])), ['obj:3']);
  assert.equal(
    r.trace.flatMap(engineCalls).filter((x) => x.path === 'page.annotations.list').length,
    0,
  );
  const writes = r.trace.flatMap(engineCalls).filter((x) => x.path === 'apply');
  assert.equal(writes.length, 2);
  assert.deepEqual(object(firstArg(writes[0])).ops, [
    {
      type: 'annotations.update',
      ref: objRef(1),
      patch: { target: { kind: 'uri', uri: 'https://updated.example' } },
    },
  ]);
  assert.deepEqual(object(firstArg(writes[1])).ops, [
    { type: 'annotations.delete', ref: objRef(1) },
  ]);
});
test('navigation ownership follows real link/pan tools and behavior matching', async () => {
  const r = await scenario('link-nav-behavior');
  for (const [i, want] of [
    [2, true],
    [4, false],
    [5, true],
    [6, true],
    [7, null],
    [9, false],
    [10, false],
    [11, null],
    [13, true],
    [14, true],
  ] as const)
    assert.deepEqual(value(r.trace[i]), want, String(i));
  assert.deepEqual(value(r.trace[3]), {
    id: 'link-nav',
    matches: { $ref: 0 },
    engaged: { $ref: 1 },
  });
  const simple = await scenario('engaged-follows-the-active-tool');
  assert.deepEqual(
    [2, 5, 7].map((i) => value(simple.trace[i])),
    [true, false, true],
  );
});
test('classic activations and current events hand every non-navigation target to the embedder', async () => {
  const r = await scenario('activate-without-a-stage');
  const expected = [
    { outcome: 'destination', destination: { kind: 'xyz', page: pr(2), x: 10, y: 20, zoom: 3 } },
    { outcome: 'uri', uri: 'https://example.com/x' },
    { outcome: 'named', name: 'LastPage' },
    ...['javascript', 'goto-remote', 'launch', 'unsupported'].map((kind, i) => ({
      outcome: 'reported',
      target: [{ kind }, { kind, file: 'other.pdf' }, { kind, path: 'calc.exe' }, { kind }][i],
    })),
    { outcome: 'destination', destination: { kind: 'fit', page: pr(1) } },
  ];
  assert.deepEqual(r.trace.slice(2).map(value), expected);
  for (const t of r.trace.slice(2)) {
    assert.equal(t.callbacks.length, 1);
    const e = object(t.callbacks[0].args[0]);
    assert.deepEqual(e.activation, value(t));
    assert.deepEqual(Object.keys(e), ['target', 'activation']);
  }
  const silent = await scenario('activate-without-a-listener');
  assert.deepEqual(silent.trace.slice(2).map(value), [
    { outcome: 'uri', uri: 'https://example.com' },
    { outcome: 'named', name: 'FirstPage' },
  ]);
  assert.deepEqual(
    silent.trace.flatMap((t) => t.callbacks),
    [],
  );
});
test('stage navigation uses page destinations and configured smooth motion rather than PDF inversion', async () => {
  const r = await scenario('activate-reveals-through-the-stage');
  assert.equal(value(r.trace[5]), 1);
  assert.deepEqual(value(r.trace[4]), { zoom: 2, x: 112, y: 978 });
  assert.deepEqual(value(r.trace[7]), value(r.trace[4]));
  for (const i of [3, 6, 8, 10, 12]) assert.deepEqual(value(r.trace[i]), { outcome: 'revealed' });
  assert.deepEqual(value(r.trace[14]), {
    outcome: 'reported',
    target: { kind: 'goto', destination: { kind: 'xyz', page: pr(77), x: 0, y: 0, zoom: 1 } },
  });
  const smooth = await scenario('activate-reveals-smoothly');
  assert.equal(value(smooth.trace[4]), true);
  assert.equal(value(smooth.trace[6]), false);
  assert.deepEqual(value(smooth.trace[7]), { zoom: 2, x: 112, y: 978 });
  const zero = await scenario('activate-destination-that-cannot-invert');
  assert.deepEqual(zero.trace.slice(3).map(value), [
    { outcome: 'revealed' },
    { outcome: 'revealed' },
  ]);
  assert.equal(zero.trace.flatMap((t) => t.callbacks).length, 2);
  const listener = await scenario('a-throwing-listener-throws-out-of-activate');
  assert.deepEqual(value(listener.trace[3]), { outcome: 'revealed' });
  assert.equal(value(listener.trace[4]), 1);
  assert.deepEqual(listener.trace[3].reports, [
    { $error: { name: 'Error', message: 'listener broke' } },
  ]);
});
test('actions dispatch resolves goto, handles URI success and preserves exact link ref/page provenance', async () => {
  const r = await scenario('activate-dispatches-through-actions');
  assert.deepEqual(r.trace[4].settled, [{ promise: 2, resolved: actionResult('goto') }]);
  assert.equal(value(r.trace[5]), 1);
  assert.deepEqual(r.trace[6].settled, [{ promise: 3, resolved: noAdapter }]);
  assert.deepEqual(value(r.trace[7]), { outcome: 'revealed' });
  assert.equal(value(r.trace[8]), 0);
  const hit = await scenario('activate-at-dispatches-with-the-page');
  for (const i of [4, 5]) {
    const t = hit.trace[i];
    assert.deepEqual(t.settled, [{ promise: i - 2, resolved: actionResult('uri') }]);
    assert.deepEqual(
      t.callbacks.map((c) => c.fn),
      ['onActivate', 'ui.openUri', 'executed'],
    );
    assert.deepEqual(t.callbacks[1].args, ['https://a.example', { isMap: false, origin: 'user' }]);
    const e = object(t.callbacks[2].args[0]);
    assert.deepEqual(e.source, { kind: 'link', annotation: objRef(1), page: pr(1) });
    assert.deepEqual(e.result, actionResult('uri'));
    assert.deepEqual(object(t.callbacks[0].args[0]).activation, {
      outcome: 'dispatched',
      dispatch: '[promise]',
    });
  }
});
test('hit tests include edges, choose smallest area with stable ties and miss absent links', async () => {
  const r = await scenario('hit-testing-and-lookup');
  assert.deepEqual(
    [3, 4, 5, 8].map((i) => object(value(r.trace[i])).id),
    ['obj:2', 'obj:1', 'obj:1', 'obj:3'],
  );
  for (const i of [6, 7, 9, 12]) assert.equal(value(r.trace[i]), null);
  assert.deepEqual(value(r.trace[10]), { outcome: 'named', name: 'NextPage' });
  assert.deepEqual(value(r.trace[11]), { outcome: 'uri', uri: 'https://example.com' });
  const area = await scenario('hit-testing-by-area');
  assert.equal(object(value(area.trace[3])).id, 'obj:1');
  assert.equal(object(value(area.trace[4])).id, 'obj:3');
  assert.deepEqual(area.trace.slice(5).map(value), [null, null]);
});
test('labels and pure resolution return current destinations, URI/name and exact reported targets', async () => {
  const r = await scenario('labels-and-resolution');
  assert.deepEqual(r.trace.slice(2, 7).map(value), [
    'https://l.example',
    'https://example.com',
    'Go to destination',
    'FirstPage',
    'Link',
  ]);
  assert.deepEqual(value(r.trace[7]), {
    kind: 'destination',
    destination: { kind: 'xyz', page: pr(2), x: 60, y: 300, zoom: 2 },
  });
  assert.deepEqual(value(r.trace[8]), {
    kind: 'destination',
    destination: { kind: 'fit', page: pr(1) },
  });
  assert.deepEqual(value(r.trace[9]), {
    kind: 'reported',
    target: { kind: 'goto', destination: { kind: 'xyz', page: pr(77), x: 0, y: 0, zoom: 1 } },
  });
  const suite = suites.find((s) => s.kind === 'function')!;
  assert.equal(suite.kind, 'function');
  if (suite.kind !== 'function') throw Error();
  const expected = [
    { kind: 'destination', destination: { kind: 'xyz', page: pr(1), x: 10, y: 20, zoom: 2 } },
    {
      kind: 'reported',
      target: { kind: 'goto', destination: { kind: 'xyz', page: pr(77), x: 0, y: 0, zoom: 1 } },
    },
    { kind: 'uri', uri: 'https://example.com' },
    { kind: 'named', name: 'NextPage' },
    ...['javascript', 'goto-remote', 'launch', 'unsupported'].map((kind, i) => ({
      kind: 'reported',
      target: [{ kind }, { kind, file: 'other.pdf' }, { kind, path: 'calc.exe' }, { kind }][i],
    })),
  ];
  for (const [i, c] of suite.cases.entries())
    assert.deepEqual(
      (suite.functions[c.fn] as (...args: unknown[]) => unknown)(...c.args),
      expected[i],
    );
});
test('listAllLinks preserves page order, retries failed pages and emits onLoaded only for successful loads', async () => {
  const r = await scenario('list-all-links-and-loaded-events');
  assert.equal(value(r.trace[3]), false);
  assert.equal(value(r.trace[5]), true);
  assert.equal(value(r.trace[6]), false);
  assert.deepEqual(
    r.trace[7].settled.map((v) => ids(v.resolved)),
    [['obj:1', 'obj:20']],
  );
  assert.deepEqual(ids(value(r.trace[9])), ['obj:1', 'obj:2']);
  assert.deepEqual(
    r.trace.flatMap((t) => t.callbacks).map((c) => c.args),
    [[{ page: pr(1) }], [{ page: pr(2) }], [{ page: pr(1) }]],
  );
  const anno = await scenario('list-all-links-annotation-backed');
  assert.deepEqual(
    [2, 3].map((i) => value(anno.trace[i])),
    [true, false],
  );
  assert.deepEqual(
    anno.trace[4].settled.map((v) => ids(v.resolved)),
    [['obj:1', 'obj:20']],
  );
  assert.equal(
    anno.trace.flatMap(engineCalls).some((x) => x.path === 'page.annotations.list'),
    false,
  );
});
test('all nine former reducer occurrences execute their surviving page-mirror ownership with explicit retired state identity', async () => {
  const initial = await scenario('initial-state');
  assert.deepEqual(initial.trace.slice(2).map(value), [false, 'idle', []]);
  const first = await scenario('set-a-first-page');
  assert.deepEqual(ids(value(first.trace[3])), ['obj:1']);
  assert.equal(value(first.trace[4]), 'ready');
  assert.deepEqual(object((value(first.trace[3]) as unknown[])[0]).bounds, {
    x: 1,
    y: 10,
    width: 20,
    height: 5,
  });
  assert.deepEqual(object((value(first.trace[3]) as unknown[])[0]).target, {
    kind: 'uri',
    uri: 'https://example.com/1',
  });
  const another = await scenario('set-another-page');
  assert.deepEqual(
    [5, 6].map((i) => ids(value(another.trace[i]))),
    [['obj:1'], ['obj:2']],
  );
  const carried = object((value(another.trace[6]) as unknown[])[0]);
  assert.deepEqual(carried.bounds, { x: 2, y: 10, width: 20, height: 5 });
  assert.deepEqual(carried.hoverEvents, { enter: true, exit: false });
  assert.deepEqual(object(carried.activate).root, {
    type: 'uri',
    subtype: 'URI',
    uri: 'u',
    isMap: false,
    next: [],
  });
  assert.deepEqual(value(another.trace[7]), []);
  const replace = await scenario('set-replaces-a-page');
  assert.deepEqual(ids(value(replace.trace[5])), ['obj:4', 'obj:5']);
  assert.equal(object((value(replace.trace[5]) as unknown[])[1]).attached, true);
  assert.deepEqual(value(replace.trace[6]), []);
  const empty = await scenario('set-an-empty-page');
  assert.deepEqual(value(empty.trace[11]), []);
  assert.equal(value(empty.trace[12]), true);
  const drop = await scenario('drop-a-page');
  assert.deepEqual(value(drop.trace[5]), []);
  assert.equal(value(drop.trace[6]), 'idle');
  assert.deepEqual(value(drop.trace[7]), []);
  assert.equal(value(drop.trace[8]), true);
  const dropEmpty = await scenario('drop-an-empty-page');
  assert.deepEqual(value(dropEmpty.trace[5]), []);
  assert.equal(value(dropEmpty.trace[6]), false);
  assert.deepEqual(ids(value(dropEmpty.trace[7])), ['obj:1']);
  const absent = await scenario('drop-an-absent-page');
  assert.deepEqual(value(absent.trace[6]), []);
  assert.equal(value(absent.trace[7]), 'idle');
  assert.deepEqual(
    absent.trace
      .flatMap(engineCalls)
      .filter((x) => x.path === 'page.annotations.list')
      .map((x) => x.pon),
    [1, 3],
  );
  assert.deepEqual(ids(value(absent.trace[5])), ['obj:1']);
  assert.deepEqual(
    [8, 9].map((i) => value(absent.trace[i])),
    [true, true],
  );
  const dropFromEmpty = await scenario('drop-from-empty');
  assert.deepEqual(value(dropFromEmpty.trace[3]), []);
  assert.deepEqual(dropFromEmpty.trace.at(-1)?.engineState, { a: { subscribers: 0 } });
});
test('every owned throw, rejection, callback report and nested action diagnostic has an exact independently specified owner', async () => {
  const expected: Record<string, { step: number; channel: string; value: unknown }[]> = {
    'viewer-only-page-cache': [{ step: 8, channel: 'rejected', value: noPage(99) }],
    'viewer-only-malformed-lists': Array.from({ length: 6 }, (_, i) => ({
      step: 2 + i * 5,
      channel: 'rejected',
      value: refused(`refused page ${i + 1}`),
    })),
    'viewer-only-loads-in-flight': [{ step: 6, channel: 'rejected', value: refused() }],
    'annotation-backed-source': [{ step: 5, channel: 'rejected', value: noPage(3) }],
    'a-throwing-listener-throws-out-of-activate': [
      {
        step: 3,
        channel: 'reports',
        value: { $error: { name: 'Error', message: 'listener broke' } },
      },
    ],
    'activate-dispatches-through-actions': [
      { step: 6, channel: 'action-result', value: noAdapter },
    ],
    'list-all-links-and-loaded-events': [{ step: 4, channel: 'rejected', value: refused() }],
  };
  for (const s of suites) {
    if (s.kind !== 'scenario') continue;
    for (const c of s.cases) {
      const r = await runScenario(c);
      assert.deepEqual(r.unmatched, [], c.name);
      const negatives: unknown[] = [];
      for (const [step, t] of r.trace.entries()) {
        if (t.outcome && 'threw' in t.outcome)
          negatives.push({ step, channel: 'threw', value: t.outcome.threw });
        for (const v of t.settled) {
          if ('rejected' in v) negatives.push({ step, channel: 'rejected', value: v.rejected });
          const o = v.resolved;
          if (
            o &&
            typeof o === 'object' &&
            'diagnostics' in o &&
            Array.isArray(o.diagnostics) &&
            o.diagnostics.length
          )
            negatives.push({ step, channel: 'action-result', value: o });
        }
        for (const v of t.reports) negatives.push({ step, channel: 'reports', value: v });
        assert.equal(
          t.callbacks.some((cb) => cb.fn.startsWith('console.')),
          false,
          c.name + ' console',
        );
      }
      assert.deepEqual(negatives, expected[c.name] ?? [], c.name);
    }
  }
});
import { createKernel } from '../../../packages/core/main/src/index';
import { interactionPlugin } from '../../../packages/plugin/interaction/src/index';
import { linkPlugin } from '../../../packages/plugin/link/src/link.plugin';
import { LinkToken } from '../../../packages/plugin/link/src/host-contract';
import { createFakeEngine } from '../src/fake-engine';
test('URI openers stack and unsubscribe, event listener failures preserve siblings, lifetime drops pending loads', async () => {
  const reports: unknown[] = [];
  const engine = createFakeEngine(
    {
      documents: {
        a: doc([
          {
            path: 'page.annotations.list',
            pon: 1,
            defer: 'late',
            value: { annotations: [link(1)] },
          },
        ]),
      },
    },
    (v) => v,
  );
  const kernel = createKernel({
    engine: engine.engine,
    plugins: [interactionPlugin(), linkPlugin()],
    report: (error) => reports.push(error),
  });
  await kernel.start();
  await kernel.documents.open({ kind: 'bytes', id: 'a', bytes: new Uint8Array() });
  const api = kernel.capability(LinkToken, 'a');
  const seen: unknown[] = [];
  const off = api.onActivated((e) => seen.push(e));
  const first = api.registerUriOpener((uri) => {
    seen.push(['first', uri]);
    return true;
  });
  const second = api.registerUriOpener((uri) => {
    seen.push(['second', uri]);
    return false;
  });
  assert.deepEqual(api.activate({ kind: 'uri', uri: 'https://test.example' }), {
    outcome: 'reported',
    target: { kind: 'uri', uri: 'https://test.example' },
  });
  first();
  assert.deepEqual(api.activate({ kind: 'uri', uri: 'https://test.example' }), {
    outcome: 'reported',
    target: { kind: 'uri', uri: 'https://test.example' },
  });
  second();
  second();
  assert.deepEqual(api.activate({ kind: 'uri', uri: 'https://test.example' }), {
    outcome: 'uri',
    uri: 'https://test.example',
  });
  assert.equal(seen.filter((v) => Array.isArray(v) && v[0] === 'first').length, 0);
  assert.equal(seen.filter((v) => Array.isArray(v) && v[0] === 'second').length, 2);
  off();
  const before = seen.length;
  api.onActivated(() => {
    throw Error('listener-isolated');
  });
  const survivor: unknown[] = [];
  api.onActivated((e) => survivor.push(e));
  assert.deepEqual(api.activate({ kind: 'named', name: 'NextPage' }), {
    outcome: 'named',
    name: 'NextPage',
  });
  assert.equal(seen.length, before);
  assert.deepEqual(survivor, [
    {
      target: { kind: 'named', name: 'NextPage' },
      activation: { outcome: 'named', name: 'NextPage' },
    },
  ]);
  assert.deepEqual(
    reports.map((e) => object(canonical(e, snapshotHooks))),
    [{ $error: { name: 'Error', message: 'listener-isolated' } }],
  );
  const loaded: unknown[] = [];
  api.onLoaded((e) => loaded.push(e));
  const pending = api.ensureLoaded(pr(1));
  const pendingOutcome = pending.then(
    () => 'resolved',
    (e) => object(e).code,
  );
  assert.equal(api.getStatus(pr(1)), 'loading');
  await kernel.documents.close('a');
  assert.equal(await pendingOutcome, 'resolved');
  assert.deepEqual(engine.snapshot(), { a: { subscribers: 0 } });
  assert.equal(engine.settle('late', { value: { annotations: [link(1)] } }), true);
  await Promise.resolve();
  await Promise.resolve();
  assert.deepEqual(loaded, []);
  assert.deepEqual(engine.unmatched, []);
  await kernel.destroy();
});
test('caller cancellation rejects immediately while the shared page load can complete for other readers', async () => {
  const engine = createFakeEngine(
    {
      documents: {
        a: doc([
          {
            path: 'page.annotations.list',
            pon: 1,
            defer: 'shared',
            value: { annotations: [link(1)] },
          },
        ]),
      },
    },
    (v) => v,
  );
  const kernel = createKernel({
    engine: engine.engine,
    plugins: [interactionPlugin(), linkPlugin()],
  });
  await kernel.start();
  await kernel.documents.open({ kind: 'bytes', id: 'a', bytes: new Uint8Array() });
  const api = kernel.capability(LinkToken, 'a');
  const abort = new AbortController();
  const pending = api.ensureLoaded(pr(1), { signal: abort.signal });
  const rejected = assert.rejects(pending, {
    name: 'PluginError',
    code: 'operation-cancelled',
    capability: 'link',
    message: 'operation cancelled',
  });
  abort.abort('caller-cancelled');
  await rejected;
  assert.equal(api.getStatus(pr(1)), 'loading');
  engine.settle('shared', { value: { annotations: [link(1)] } });
  await api.ensureLoaded(pr(1));
  assert.equal(api.getStatus(pr(1)), 'ready');
  assert.deepEqual(
    api.listLinks(pr(1)).map((l) => l.id),
    ['obj:1'],
  );
  assert.deepEqual(engine.unmatched, []);
  await kernel.destroy();
});
import { createHash } from 'node:crypto';
const history = new URL('../historical/task-12b2c6/', import.meta.url);
const read = (name: string): unknown => JSON.parse(readFileSync(new URL(name, history), 'utf8'));
const sha = (v: unknown) => createHash('sha256').update(JSON.stringify(v)).digest('hex');
type Saved = {
  file: string;
  name: string;
  input: unknown;
  inputSha256?: string;
  outcome?: unknown;
  result?: unknown;
};
test('all29 immutable original occurrences map one-to-one to complete current cases and eight explicit additions', () => {
  const original = read('original-runtime.json') as Saved[];
  const account = object(read('accounting.json'));
  const mappings = account.mappings as {
    originalOccurrence: number;
    file: string;
    name: string;
    originalInput: unknown;
    originalInputSha256: string;
    currentInputSha256: string;
    currentKind: string;
    currentFile: string;
  }[];
  const current = read('current-inputs-runtime.json') as Saved[];
  assert.equal(original.length, 29);
  assert.equal(mappings.length, 29);
  assert.equal(current.length, 37);
  assert.deepEqual(account.summary, {
    original: 29,
    current: 37,
    mapped: 29,
    wholeRetirements: 0,
    partialRetirements: 18,
    additions: 8,
    changedInputs: 29,
  });
  for (const [i, o] of original.entries()) {
    const m = mappings[i];
    assert.equal(m.originalOccurrence, i);
    assert.equal(m.name, o.name);
    assert.equal(m.file, o.file);
    assert.deepEqual(m.originalInput, o.input);
    assert.equal(sha(o.input), m.originalInputSha256);
    const c = current.find((c) => c.file === m.currentFile && c.name === o.name)!;
    assert.equal(sha(c.input), m.currentInputSha256);
    assert.equal(m.currentKind, 'scenario');
  }
  assert.equal(new Set(current.map((c) => c.file + '/' + c.name)).size, 37);
  assert.deepEqual(read('task11-current-drift.json'), []);
});
test('all37 current definitions and complete outputs preserve every return, promise, callback, event, engine call and report', async () => {
  for (const row of read('current-inputs-runtime.json') as Saved[]) {
    const s = suites.find((s) => s.file === row.file)!;
    const c = s.cases.find((c) => c.name === row.name)!;
    assert.deepEqual(canonical(c), row.input, row.name + ' complete input');
    assert.equal(sha(canonical(c)), row.inputSha256);
    if (s.kind === 'scenario') {
      assert.ok('steps' in c);
      assert.deepEqual(await runScenario(c), row.result, row.name + ' complete outcome');
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
        row.name,
      );
    }
  }
});
test('prior current actions160 and commands79 keep their complete actual inputs and every output channel', async () => {
  const { suites: actions } = await import('../src/suites/actions');
  const { suites: commands } = await import('../src/suites/commands');
  const available = [...actions, ...commands];
  let count = 0;
  for (const task of ['task-12b2c2', 'task-12b2c3', 'task-12b2c4', 'task-12b2c5']) {
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
  assert.equal(count, 239);
});
test('immutable evidence and the one-time prior-tooling audit manifest remain exact', () => {
  const manifest = object(read('preservation.json'));
  assert.equal(manifest.preservedCount, 460);
  // Active-byte preservation is the account.py audit, not another guard over future owners.
  for (const [p, h] of Object.entries(read('integrity.json') as Record<string, string>)) {
    assert.equal(
      createHash('sha256')
        .update(readFileSync(new URL(p, history)))
        .digest('hex'),
      h,
      p,
    );
  }
  for (const p of object(read('source-evidence.json')).currentOwners as {
    path: string;
    sha256: string;
  }[]) {
    assert.equal(
      createHash('sha256')
        .update(readFileSync(new URL('../../../' + p.path, import.meta.url)))
        .digest('hex'),
      p.sha256,
      p.path,
    );
  }
});

test('original link29 and all progressed source guard authorities have fixed immutable hashes', () => {
  assert.equal(
    createHash('sha256')
      .update(readFileSync(new URL('original-runtime.json', history)))
      .digest('hex'),
    '4c0944fcbc1b405d03534dc88f9f9df1de3ba9518603676cffb0f5ede075eaa7',
  );
  assert.equal(
    createHash('sha256')
      .update(readFileSync(new URL('original-link.ts.txt', history)))
      .digest('hex'),
    '1b93c7d7597cf50ee503b2be67a1769502880260456b353a666f69a9ed3bb29e',
  );
  assert.equal(
    createHash('sha256')
      .update(readFileSync(new URL('base-acrojs-current.test.ts.txt', history)))
      .digest('hex'),
    '3417d507c25b7104ee93c031f2278c8528a84d42ded2a5e67d64499a5cfe3b10',
  );
  assert.equal(
    createHash('sha256')
      .update(readFileSync(new URL('base-actions-current.test.ts.txt', history)))
      .digest('hex'),
    'd1f1f8256d9efc7e6bf33f9443a1541ab3a021abfded473a323909c1ab706f0a',
  );
  assert.equal(
    createHash('sha256')
      .update(readFileSync(new URL('base-commands-current.test.ts.txt', history)))
      .digest('hex'),
    '98819b93b3948551321c7e89c69a2cb492d563ea06285bd8ed7d37dceb3bfe53',
  );
});

test('adding or replacing a page retains loaded page1 and cached-empty page3', async () => {
  const item1 = {
    id: 'obj:1',
    bounds: { x: 1, y: 10, width: 20, height: 5 },
    target: { kind: 'uri', uri: 'https://example.com/1' },
    ref: objRef(1),
    attached: false,
  };
  const empty = await scenario('set-an-empty-page');
  assert.deepEqual(empty.trace[5] && value(empty.trace[5]), [item1], 'retain complete page1 item');
  assert.deepEqual(
    [6, 7].map((i) => value(empty.trace[i])),
    [true, 'ready'],
  );
  assert.deepEqual(value(empty.trace[8]), []);
  assert.deepEqual(
    [9, 10].map((i) => value(empty.trace[i])),
    [true, 'ready'],
  );
  assert.deepEqual(value(empty.trace[11]), []);
  assert.deepEqual(
    [12, 13].map((i) => value(empty.trace[i])),
    [true, 'ready'],
  );
  assert.deepEqual(
    empty.trace
      .flatMap(engineCalls)
      .filter((c) => c.path === 'page.annotations.list')
      .map((c) => c.pon),
    [1, 3, 2],
  );
  const another = await scenario('set-another-page');
  assert.deepEqual(value(another.trace[5]), [item1]);
  assert.deepEqual(value(another.trace[7]), []);
  assert.deepEqual(
    [8, 9].map((i) => value(another.trace[i])),
    [true, 'ready'],
  );
  const replace = await scenario('set-replaces-a-page');
  assert.deepEqual(value(replace.trace[6]), []);
  assert.deepEqual(
    [7, 8].map((i) => value(replace.trace[i])),
    [true, 'ready'],
  );
});
