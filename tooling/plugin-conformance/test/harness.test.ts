import assert from 'node:assert/strict';
import { test } from 'node:test';
import '../src/probe';
import { runScenario, type ScenarioCase, type TraceEntry } from '../src/scenario';
import { suites } from '../src/suites/kernel';
import type { DocumentEvent } from '@embedpdf/engine-core/runtime';
import { createFakeEngine } from '../src/fake-engine';

const start = { op: 'call', target: 'kernel', method: 'start' } as const;
const open = {
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id: 'a', bytes: { $bytes: '' } }],
} as const;

test('the runner records committed state, ignores identity transitions, and retains construction callbacks', async () => {
  const result = await runScenario({
    name: 'state-cell',
    covers: 'Current public state cells and construction observability.',
    plugins: [
      {
        factory: 'probe',
        config: { id: 'p', token: 'p', state: { n: 0 }, connect: { watchState: true } },
      },
    ],
    steps: [
      start,
      { op: 'call', target: { token: 'p' }, method: 'replaceState', args: [{ n: 1 }] },
      { op: 'call', target: { token: 'p' }, method: 'sameState' },
      { op: 'call', target: 'kernel', method: 'destroy' },
    ],
  });
  assert.deepEqual(result.unmatched, []);
  assert.ok(result.trace[0].callbacks.some((c) => c.fn === 'p.create'));
  assert.deepEqual(result.trace[1].state, {
    core: { activeId: null, documents: {}, order: [], pending: {} },
    plugins: { p: { n: 1 } },
  });
  assert.deepEqual(result.trace[1].stateChanges, [
    { slice: 'p', previous: { n: 0 }, next: { n: 1 } },
  ]);
  assert.deepEqual(result.trace[2].stateChanges, []);
});

test('document connect failures roll back a real open and close the acquired handle', async () => {
  const result = await runScenario({
    name: 'rollback',
    covers: 'Transactional bring-up.',
    plugins: [
      {
        factory: 'probe',
        config: {
          id: 'p',
          token: 'p',
          scope: 'document',
          connect: { throws: { message: 'connect broke' } },
        },
      },
    ],
    engine: { documents: { a: { pages: [] } } },
    steps: [start, open],
  });
  assert.deepEqual(result.unmatched, []);
  assert.match(JSON.stringify(result.trace[1].settled), /connect broke/);
  assert.ok(result.trace[1].engine.some((call) => (call as { path?: string }).path === 'close'));
  assert.equal(
    (result.trace[1].state as { core: { pending: Record<string, { status: string }> } }).core
      .pending.a.status,
    'error',
  );
});

test('confirmed engine events reach the kernel before an immediate or deferred mutation settles', async () => {
  for (const deferred of [false, true]) {
    const event: DocumentEvent = {
      type: 'pages.reordered',
      origin: { kind: 'local', sessionId: 's', sub: null, ts: 0, serverId: null },
      layout: { pageCount: 0, pages: [], namedPages: [] },
      pages: [],
      meta: { affectedPages: [], cacheDelta: null, opId: 'write', undoable: true },
    };
    const result = await runScenario({
      name: 'publish-before-settle',
      covers: 'Event publication ordering.',
      plugins: [
        {
          factory: 'probe',
          config: { id: 'p', token: 'p', scope: 'document', connect: { listenDoc: true } },
        },
      ],
      engine: {
        documents: {
          a: {
            responses: [
              {
                path: 'pages.reorder',
                value: {},
                events: [event],
                ...(deferred ? { defer: 'mutation' } : {}),
              },
            ],
          },
        },
      },
      steps: [
        start,
        open,
        {
          op: 'call',
          target: { token: 'p', doc: 'a' },
          method: 'docCall',
          args: ['pages.reorder', [], 0],
          meter: 20,
        },
        ...(deferred ? [{ op: 'settle' as const, label: 'mutation', value: {}, meter: 20 }] : []),
      ],
    });
    assert.deepEqual(result.unmatched, []);
    const callbacks = result.trace.at(-1)!.callbacks;
    const eventIndex = callbacks.findIndex((c) => c.fn === 'p.docEvent');
    assert.ok(eventIndex >= 0);
    assert.ok(callbacks.findIndex((c) => c.fn === '@@settled') > eventIndex);
    assert.equal(
      (result.trace.at(-1)!.state as { core: { documents: Record<string, { revision: number }> } })
        .core.documents.a.revision,
      1,
    );
  }
});

test('a missing page rejects its service call while page(ref) remains a synchronous address', async () => {
  const fake = createFakeEngine({ documents: { a: { pages: [] } } }, (v) => v);
  const doc = await fake.engine.open({ kind: 'bytes', id: 'a', bytes: new Uint8Array() });
  const page = doc.page({ kind: 'objectNumber', objectNumber: 9 });
  assert.equal(page.ref.objectNumber, 9);
  await assert.rejects(
    Promise.resolve(page.text.get()),
    (e: { code: string }) => e.code === 'NotFound',
  );
});

const scenarioNamed = (name: string): ScenarioCase => {
  const scenario = suites
    .flatMap((suite) => (suite.kind === 'scenario' ? suite.cases : []))
    .find((c) => c.name === name);
  assert.ok(scenario, name);
  return scenario;
};
const runNamed = (name: string) => runScenario(scenarioNamed(name));
const stateOf = (entry: TraceEntry) =>
  entry.state as {
    core: {
      documents: Record<string, { revision: number }>;
      pending: Record<string, { status: string }>;
      order: string[];
    };
    plugins: Record<string, unknown>;
  };
const returned = (entry: TraceEntry) => {
  assert.ok(entry.outcome && 'returned' in entry.outcome);
  return entry.outcome.returned;
};

test('locked and failed opens retain their intended refusals, and a valid retry reaches ready', async () => {
  const { trace } = await runNamed('password-locked-then-unlocked');
  assert.deepEqual(trace[1].callbacks, []);
  assert.deepEqual(
    trace[1].documentEvents.map((e) => e.event),
    ['activeChanged', 'locked'],
  );
  assert.deepEqual(
    returned(trace[6]),
    returned(trace[10]),
    'wrong password leaves the locked tab unchanged',
  );
  assert.deepEqual(
    trace[9].settled.map((s) => (s.rejected as { $error: { code: string } }).$error.code),
    ['permission-denied'],
  );
  assert.deepEqual(
    trace[11].callbacks.map((c) => c.fn),
    ['alpha.create', 'alpha.connect'],
  );
  assert.equal(stateOf(trace[11]).core.pending.a, undefined);
  assert.ok(stateOf(trace[11]).core.documents.a);
  assert.deepEqual(
    trace[11].documentEvents.map((e) => e.event),
    ['opened'],
  );

  const failed = await runNamed('open-failure-parks-an-error-tab');
  assert.equal(stateOf(failed.trace[1]).core.pending.a.status, 'error');
  assert.deepEqual(failed.trace[1].callbacks, []);
  assert.deepEqual(
    failed.trace[1].documentEvents.map((e) => e.event),
    ['activeChanged', 'openFailed'],
  );
  assert.ok(
    stateOf(failed.trace.at(-1)!).core.documents.a,
    'close and reopen after failure succeeds',
  );
});

test('rollback releases earlier plugins, closes the handle, and never publishes a ready document', async () => {
  for (const name of ['bring-up-failure-rolls-back', 'connection-setup-failure-rolls-back']) {
    const { trace } = await runNamed(name);
    const failed = trace[1];
    assert.equal(stateOf(failed).core.pending.a.status, 'error');
    assert.deepEqual(stateOf(failed).plugins, {});
    assert.deepEqual(failed.engineState, { a: { subscribers: 0 } });
    assert.equal(failed.engine.filter((c) => (c as { path?: string }).path === 'close').length, 1);
    assert.equal(failed.documentEvents.filter((e) => e.event === 'opened').length, 0);
    if (name === 'bring-up-failure-rolls-back') {
      assert.deepEqual(
        failed.callbacks.map((c) => c.fn),
        ['alpha.create', 'alpha.connect', 'beta.create', 'alpha.teardown'],
      );
    }
  }
});

test('destroy waits for LIFO document teardowns before releasing the workspace', async () => {
  const { trace } = await runNamed('destroy-closes-everything');
  assert.equal(trace[4].status, 'destroying');
  assert.deepEqual(trace[4].settled, []);
  assert.deepEqual(
    trace[4].callbacks.map((c) => c.fn),
    ['alpha.cleanup', 'beta.teardown', 'beta.teardown'],
  );
  assert.deepEqual(
    trace[6].callbacks.map((c) => c.fn),
    ['alpha.teardown'],
  );
  assert.deepEqual(
    trace[7].callbacks.map((c) => c.fn),
    ['alpha.teardown', 'ws.teardown'],
  );
  assert.equal(trace[7].status, 'destroyed');
  assert.deepEqual(trace[7].settled, [{ promise: 3, resolved: { $undefined: true } }]);
  assert.deepEqual(trace[7].engineState, { a: { subscribers: 0 }, b: { subscribers: 0 } });
  assert.deepEqual(stateOf(trace[7]).plugins, {});
});

test('bring-up resolution and page reads exercise current public semantics', async () => {
  const bringup = await runNamed('try-get-during-bring-up');
  assert.equal(returned(bringup.trace[2]), null);
  assert.ok(
    bringup.trace[3].callbacks.some(
      (c) => c.fn === 'alpha.tryGet' && c.args[0] === 'beta' && c.args[1] === true,
    ),
  );
  assert.equal(returned(bringup.trace[4]), true);
  assert.equal(returned(bringup.trace[5]), 'a');
  const pages = await runNamed('controller-pages');
  assert.deepEqual(
    returned(pages.trace[3]),
    returned(pages.trace[4]),
    'index and ref name the same page',
  );
  assert.equal(returned(pages.trace[11]), null, 'deleted ref is no longer readable');
  const identity = await runNamed('invalid-page-identities');
  const valid = returned(identity.trace[7]) as {
    pdfCropBox: { top: number };
    size: { width: number };
  };
  assert.equal(valid.pdfCropBox.top, 0);
  assert.equal(valid.size.width, 100);
  assert.deepEqual(returned(identity.trace[7]), returned(identity.trace[8]));
});

test('the turn meter pins lifecycle settlement and cleanup ordering', async () => {
  const expected: Record<string, readonly string[]> = {
    'open-settles-on-its-turn': ['1:12:alpha.create', '1:12:alpha.connect', '1:18:@@settled'],
    'a-failed-open-rejects-on-its-turn': ['1:11:@@settled'],
    'a-throwing-thunk-rejects-on-its-turn': ['1:-1:throwing', '1:6:@@settled'],
    'unlock-settles-on-its-turn': [
      '2:10:@@settled',
      '3:12:alpha.create',
      '3:12:alpha.connect',
      '3:19:@@settled',
    ],
    'retry-settles-on-its-turn': ['2:12:alpha.create', '2:12:alpha.connect', '2:19:@@settled'],
    'close-joins-a-thunk-open-on-its-turn': [
      '2:-1:source',
      '3:7:@@settled',
      '3:9:closed',
      '3:11:@@settled',
    ],
    'destroy-joins-start-on-its-turn': [
      '0:-1:ws.create',
      '0:-1:ws.connect',
      '1:2:ws.teardown',
      '1:17:@@settled',
    ],
    'close-joins-an-open-on-its-turn': ['3:7:@@settled', '3:12:closed', '3:14:@@settled'],
    'close-joins-a-pending-policy-on-its-turn': ['4:7:@@settled', '4:12:closed', '4:14:@@settled'],
    'close-of-a-ready-document-on-its-turn': [
      '2:-1:alpha.create',
      '2:-1:alpha.connect',
      '3:1:alpha.teardown',
      '3:13:closed',
      '3:15:@@settled',
    ],
    'destroy-closes-a-document-on-its-turn': [
      '0:-1:ws.create',
      '0:-1:ws.connect',
      '2:-1:alpha.create',
      '2:-1:alpha.connect',
      '3:3:alpha.teardown',
      '3:15:closed',
      '3:17:ws.teardown',
      '3:32:@@settled',
    ],
    'bring-up-awaits-the-render-policy-on-its-turn': [
      '1:12:alpha.create',
      '1:12:alpha.connect',
      '1:18:@@settled',
    ],
  };
  for (const [name, callbacks] of Object.entries(expected)) {
    const { trace } = await runNamed(name);
    const measured = trace.flatMap((entry, step) => {
      let turn = -1;
      return entry.callbacks.flatMap((callback) => {
        if (callback.fn === '@@turn') {
          turn = callback.args[0] as number;
          return [];
        }
        return [`${step}:${turn}:${callback.fn}`];
      });
    });
    assert.deepEqual(measured, callbacks, name);
  }
});

test('the fake exposes current service methods and isolates listeners while advancing the cursor', async () => {
  const reports: unknown[] = [];
  const fake = createFakeEngine(
    { documents: { a: {} } },
    (v) => v,
    (error) => reports.push(error),
  );
  const doc = await fake.engine.open({ kind: 'bytes', id: 'a', bytes: new Uint8Array() });
  assert.equal((doc.metadata as unknown as Record<string, unknown>).read, undefined);
  assert.equal((doc.forms as unknown as Record<string, unknown>).applyEffects, undefined);
  assert.equal((doc.pages as unknown as Record<string, unknown>).move, undefined);
  assert.equal(typeof doc.metadata.get, 'function');
  assert.equal(typeof doc.forms.setSignatureAppearance, 'function');
  assert.equal(
    typeof doc.page({ kind: 'objectNumber', objectNumber: 1 }).measure.listViewports,
    'function',
  );
  let delivered = 0;
  doc.events.subscribe(() => {
    throw new Error('listener broke');
  });
  doc.events.on('pages.reordered', () => {
    delivered += 1;
  });
  for (const serverId of [9, 3])
    fake.emit('a', {
      type: 'pages.reordered',
      origin: { kind: 'remote', sessionId: 's', sub: null, ts: 0, serverId },
      layout: { pageCount: 0, pages: [], namedPages: [] },
      pages: [],
      meta: { affectedPages: [], cacheDelta: null, opId: 'write', undoable: true },
    });
  assert.equal(delivered, 2);
  assert.equal(reports.length, 2);
  assert.equal(doc.events.lastServerId(), 9);
});
