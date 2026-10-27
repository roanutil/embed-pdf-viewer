import assert from 'node:assert/strict';
import { readdir } from 'node:fs/promises';
import { test } from 'node:test';
import type { Suite, FunctionCase } from '../src/vectors';
import { canonical, snapshotHooks } from '../src/canonical';
import { runScenario, type TraceEntry, type ScenarioCase } from '../src/scenario';
import { createFakeEngine } from '../src/fake-engine';
import { expectedRefusals } from './current-suite-refusals';
import type { PageGeometrySnapshot } from '@embedpdf/engine-core/runtime';

const owned = new Set([
  'geometry.ts',
  'selection.ts',
  'search.ts',
  'search-reducer.ts',
  'stage-plugin.ts',
  'stage-pure.ts',
]);
const suites: Suite[] = [];
for (const file of (await readdir(new URL('../src/suites/', import.meta.url)))
  .filter((f) => f.endsWith('.ts'))
  .sort()) {
  const module = (await import(new URL(`../src/suites/${file}`, import.meta.url).href)) as {
    suites?: Suite[];
  };
  if (owned.has(file)) suites.push(...(module.suites ?? []));
}
const singular = new Set([
  'invert/singular-zero',
  'invert/singular-rank-one',
  'invert/underflowing-det',
  'pageTransform/singular-throws',
]);
function decode(v: unknown): unknown {
  if (Array.isArray(v)) return v.map(decode);
  if (!v || typeof v !== 'object') return v;
  const o = v as Record<string, unknown>;
  if ('$number' in o) return Number(o.$number);
  if ('$undefined' in o) return undefined;
  return Object.fromEntries(Object.entries(o).map(([k, v]) => [k, decode(v)]));
}
const expectedDiagnostics: Record<string, readonly string[]> = {
  'navigation-executors': [
    'executor-failed:goto: the destination page is not in this document',
    "executor-inert:named: unknown named action 'Bogus'",
  ],
  'a-goto-without-a-destination': [
    "executor-failed:goto: Cannot read properties of undefined (reading 'page')",
    "executor-failed:goto: Cannot read properties of undefined (reading 'kind')",
  ],
  'a-thumbnail-lens-stays-out': [
    'no-executor:named: no executor registered',
    'no-executor:goto: no executor registered',
  ],
};
function diagnosticsOf(value: unknown): string[] {
  if (Array.isArray(value)) return value.flatMap(diagnosticsOf);
  if (!value || typeof value !== 'object') return [];
  const object = value as Record<string, unknown>;
  return Object.entries(object).flatMap(([key, value]) =>
    key === 'diagnostics' && Array.isArray(value)
      ? value.map(
          (diagnostic: { code: string; message: string }) =>
            `${diagnostic.code}:${diagnostic.message}`,
        )
      : diagnosticsOf(value),
  );
}

const runs = new Map<string, TraceEntry[]>();
const values = new Map<string, unknown>();
test('all owned cases have unique names and exactly their current shipping outcome', async () => {
  let total = 0,
    functions = 0,
    scenarios = 0;
  for (const suite of suites) {
    const names = new Set<string>();
    for (const c of suite.cases) {
      assert.ok(!names.has(c.name), `${suite.file}: duplicate ${c.name}`);
      names.add(c.name);
      total++;
      if (suite.kind === 'function') {
        functions++;
        const f = c as FunctionCase;
        try {
          const v = suite.functions[f.fn](...(decode(canonical(f.args)) as never[]));
          canonical(v, snapshotHooks);
          assert.ok(!singular.has(c.name), `${c.name}: expected singular refusal`);
          values.set(`${suite.file}:${c.name}`, v);
        } catch (e) {
          assert.ok(singular.has(c.name), `${suite.file}:${c.name}: ${String(e)}`);
          assert.match((e as Error).message, /non-invertible matrix/);
        }
      } else {
        scenarios++;
        const result = await runScenario(c as ScenarioCase);
        assert.deepEqual(result.unmatched, [], `${c.name}: unmatched`);
        const errors: unknown[] = [];
        const diagnostics: string[] = [];
        result.trace.forEach((t, i) => {
          for (const value of [
            t.outcome && 'threw' in t.outcome ? t.outcome.threw : undefined,
            ...t.settled.map((s) => s.rejected),
          ]) {
            if (value === undefined) continue;
            const e = (value as { $error: { name: string; code?: string; message: string } })
              .$error;
            assert.ok(
              !/is not a function|no ref \d/.test(e.message),
              `${c.name}: obsolete method/ref`,
            );
            errors.push([i, e.code ?? e.name, e.message]);
          }
          diagnostics.push(...diagnosticsOf(t.outcome), ...diagnosticsOf(t.settled));
          for (const callback of t.callbacks) {
            const first = callback.args[0];
            if (
              first &&
              typeof first === 'object' &&
              'code' in first &&
              'action' in first &&
              'source' in first
            )
              diagnostics.push(`callback:${String(first.code)}`);
          }
          for (const report of t.reports) {
            assert.equal(c.name, 'change-hooks');
            assert.deepEqual(report, { $error: { name: 'Error', message: 'listener' } });
          }
        });
        assert.deepEqual(
          errors,
          expectedRefusals[c.name] ?? [],
          `${suite.file}:${c.name}: refusals`,
        );
        assert.deepEqual(
          diagnostics,
          expectedDiagnostics[c.name] ?? [],
          `${suite.file}:${c.name}: diagnostics`,
        );
        runs.set(`${suite.file}:${c.name}`, result.trace);
      }
    }
  }
  assert.equal(total, 1287);
  assert.equal(scenarios, 171);
  console.log(JSON.stringify({ total, functions, scenarios, unexpected: 0, duplicates: 0 }));
});
const geometry: PageGeometrySnapshot = {
  runs: [
    {
      start: 0,
      rect: { x: 10, y: 20, width: 18, height: 10 },
      glyphs: [0, 1, 2].map((i) => ({ loose: { x: 10 + i * 6, y: 20, width: 6, height: 10 } })),
    },
  ],
};
const page = {
  index: 0,
  ref: { kind: 'objectNumber' as const, objectNumber: 1 },
  label: null,
  size: { width: 600, height: 800 },
  rotation: 0 as const,
  userUnit: 1,
  boxes: {
    media: { x: 0, y: 0, width: 600, height: 800 },
    crop: { x: 0, y: 0, width: 600, height: 800 },
    bleed: { x: 0, y: 0, width: 600, height: 800 },
    trim: { x: 0, y: 0, width: 600, height: 800 },
    art: { x: 0, y: 0, width: 600, height: 800 },
  },
  pdfCropBox: { left: 0, bottom: 0, right: 600, top: 800 },
  tabOrder: null,
};
test('immediate and deferred JSON layouts reconstruct real char, word, quad and segment reads', async () => {
  for (const deferred of [false, true]) {
    const fake = createFakeEngine(
      {
        documents: {
          a: {
            pages: [page],
            responses: [
              {
                path: 'page.text.layout',
                ...(deferred ? { defer: 'layout' } : { value: { $textLayout: geometry } }),
              },
            ],
          },
        },
      },
      (v) => v,
    );
    const doc = await fake.engine.open({ kind: 'bytes', id: 'a', bytes: new Uint8Array() });
    const pending = doc.page(page.ref).text.layout();
    if (deferred) fake.settle('layout', { value: { $textLayout: geometry } });
    const layout = await pending;
    assert.equal(layout.charAt({ x: 13, y: 25 }), 0);
    assert.deepEqual(layout.wordAt(1), { start: 0, count: 3 });
    assert.deepEqual(layout.segments({ start: 0, count: 3 })[0].rect, {
      x: 10,
      y: 20,
      width: 18,
      height: 10,
    });
    assert.deepEqual(layout.charQuad(2), {
      upperLeft: { x: 22, y: 20 },
      upperRight: { x: 28, y: 20 },
      lowerLeft: { x: 22, y: 30 },
      lowerRight: { x: 28, y: 30 },
    });
  }
});
test('selection, search, geometry and framed motion retain meaningful successful results', () => {
  const search = runs.get('plugins/search.json:streams-a-cursor-chain')!;
  const hits = (
    search[5].outcome as {
      returned: Array<{
        start: number;
        count: number;
        segments: Array<{ rect: { width: number } }>;
      }>;
    }
  ).returned;
  assert.equal(hits.length, 4);
  assert.equal(hits[0].start, 0);
  assert.equal(hits[0].count, 4);
  assert.equal(hits[0].segments[0].rect.width, 40);
  assert.deepEqual((search[11].outcome as { returned: unknown }).returned, {
    pagesSearched: 3,
    pageCount: 3,
  });
  const selection = runs.get('plugins/selection.json:click-then-drag')!;
  assert.ok(
    selection
      .flatMap((t) => t.stateChanges)
      .some((c) => {
        const n = c.next as { selection?: unknown; segments?: Record<string, unknown[]> };
        return n.selection && (n.segments?.['1']?.length ?? 0) > 0;
      }),
  );
  const layout = values.get('plugins/selection-geometry.json:buildPageTextLayout/mixed') as {
    segments: unknown[][];
    quads: unknown[];
  };
  assert.ok(layout.segments.some((s) => s.length > 0));
  assert.ok(layout.quads.some(Boolean));
  const motion = runs.get('plugins/stage-motion.json:smooth-navigation-tween')!;
  const at = (i: number) =>
    (motion[i].state as { plugins: Record<string, { camera: { y: number } }> }).plugins['stage::a']
      .camera.y;
  assert.ok(at(6) < at(8) && at(8) < at(10));
  assert.deepEqual(motion[4].outcome, { returned: true });
  assert.deepEqual(motion[10].outcome, { returned: false });
  const empty = runs.get('plugins/stage-capability.json:a-document-with-no-pages')!;
  assert.deepEqual(empty[5].outcome, { returned: [] });
});
test('instance clocks cancel pending animation frames on close and destroy', async () => {
  for (const teardown of ['close', 'destroy'] as const) {
    const result = await runScenario({
      name: `frame-${teardown}`,
      covers: 'Pending host frames belong to the instance.',
      clock: { frames: true },
      plugins: [{ factory: 'stage', config: { scrollBehavior: 'smooth', responsive: [] } }],
      engine: {
        documents: {
          a: {
            pages: [page, { ...page, index: 1, ref: { kind: 'objectNumber', objectNumber: 2 } }],
            responses: [{ path: 'render.getPolicy', value: null }],
          },
        },
      },
      steps: [
        { op: 'call', target: 'kernel', method: 'start' },
        {
          op: 'call',
          target: 'documents',
          method: 'open',
          args: [{ kind: 'bytes', id: 'a', bytes: { $bytes: '' } }],
        },
        {
          op: 'call',
          target: { token: 'stage', doc: 'a' },
          method: 'setViewportSize',
          args: [{ width: 400, height: 400 }],
        },
        { op: 'call', target: { token: 'stage', doc: 'a' }, method: 'goToPage', args: [1] },
        { op: 'advance', ms: 16 },
        teardown === 'close'
          ? { op: 'call', target: 'documents', method: 'close', args: ['a'] }
          : { op: 'call', target: 'kernel', method: 'destroy' },
        { op: 'advance', ms: 1000 },
      ],
    });
    assert.deepEqual(result.unmatched, []);
    assert.ok(result.trace[4].stateChanges.length > 0);
    assert.deepEqual(result.trace[6].stateChanges, []);
    assert.deepEqual(result.trace[6].reports, []);
  }
});

test('layout mapping preserves pre-settlement events, error/abort paths and the originating session', async () => {
  const event = {
    type: 'pages.reordered' as const,
    pages: [page.ref],
    origin: {
      kind: 'local' as const,
      sessionId: 'layout-session',
      sub: null,
      ts: 0,
      serverId: null,
    },
    layout: { pageCount: 1, pages: [page], namedPages: [] },
    meta: { affectedPages: [], cacheDelta: null, opId: 'layout-event', undoable: true },
  };
  for (const deferred of [false, true]) {
    const fake = createFakeEngine(
      {
        documents: {
          a: {
            pages: [page],
            responses: [
              {
                path: 'page.text.layout',
                events: [event],
                ...(deferred ? { defer: 'layout' } : { value: { $textLayout: geometry } }),
              },
            ],
          },
        },
      },
      (v) => v,
    );
    const doc = await fake.engine.open({ kind: 'bytes', id: 'a', bytes: new Uint8Array() });
    const order: string[] = [];
    doc.events.subscribe(() => order.push('event'));
    const pending = doc
      .page(page.ref)
      .text.layout()
      .then((layout) => {
        assert.equal(layout.charCount, 3);
        order.push('settled');
      });
    if (deferred) fake.settle('layout', { value: { $textLayout: geometry } });
    await pending;
    assert.deepEqual(order, ['event', 'settled']);
  }
  const fake = createFakeEngine(
    {
      documents: {
        a: {
          pages: [page],
          responses: [
            {
              path: 'page.text.layout',
              times: 1,
              error: { name: 'EngineError', code: 'Unknown', message: 'layout failed' },
              events: [event],
            },
            { path: 'page.text.layout', times: 1, value: { runs: [] }, events: [event] },
            { path: 'page.text.layout', defer: 'late', events: [event] },
          ],
        },
      },
    },
    (v) => v,
  );
  const old = await fake.engine.open({ kind: 'bytes', id: 'a', bytes: new Uint8Array() });
  let oldEvents = 0;
  old.events.subscribe(() => oldEvents++);
  await assert.rejects(Promise.resolve(old.page(page.ref).text.layout()), /layout failed/);
  await assert.rejects(Promise.resolve(old.page(page.ref).text.layout()), /needs a \$textLayout/);
  assert.equal(oldEvents, 0);
  const aborted = old.page(page.ref).text.layout();
  const rejected = assert.rejects(Promise.resolve(aborted));
  aborted.abort();
  await rejected;
  await old.close();
  const reopened = await fake.engine.open({ kind: 'bytes', id: 'a', bytes: new Uint8Array() });
  let newEvents = 0;
  reopened.events.subscribe(() => newEvents++);
  fake.settle('late', { value: { $textLayout: geometry } });
  assert.equal(oldEvents, 1);
  assert.equal(newEvents, 0);
});

const scenarioOf = (file: string, name: string): ScenarioCase => {
  const suite = suites.find((s) => s.file === file);
  assert.ok(suite?.kind === 'scenario');
  const scenario = suite.cases.find((c) => c.name === name);
  assert.ok(scenario);
  return scenario;
};
const stageState = (trace: TraceEntry[], at: number) =>
  (
    trace[at].state as {
      plugins: Record<
        string,
        { camera: { x: number; y: number; zoom: number }; cursor: number; motionCause: string }
      >;
    }
  ).plugins['stage::a'];

test('review: search permission twins exercise both typed snippets branches', async () => {
  const scenario = scenarioOf('plugins/search.json', 'can-search');
  const result = await runScenario(scenario);
  assert.deepEqual(
    result.trace.slice(5).map((t) => t.outcome),
    [true, true, false, true, true, true, false, false, false, true, true, true].map(
      (returned) => ({ returned }),
    ),
  );
});

test('review: action XYZ destinations retain the actual requested camera placement', async () => {
  const base = scenarioOf('plugins/stage-feeds.json', 'navigation-executors');
  const result = await runScenario({
    ...base,
    steps: [
      ...base.steps.slice(0, 4),
      {
        op: 'call',
        target: { token: 'stage', doc: 'a' },
        method: 'pageToViewport',
        args: [
          { kind: 'objectNumber', objectNumber: 3 },
          { x: 100, y: 100 },
        ],
      },
    ],
  });
  assert.deepEqual(result.unmatched, []);
  assert.deepEqual(stageState(result.trace, 3).camera, { x: 88, y: 1720, zoom: 2 });
  assert.deepEqual(result.trace[4].outcome, { returned: { x: 24, y: 24 } });
});

test('review: page lifecycle triggers navigate and produce no failure diagnostics', async () => {
  const scenario = scenarioOf('plugins/stage-feeds.json', 'page-state-feed');
  const result = await runScenario(scenario);
  assert.deepEqual(result.unmatched, []);
  assert.deepEqual(
    result.trace.flatMap((t) => t.callbacks.filter((c) => c.fn === 'diagnostic')),
    [],
  );
  assert.deepEqual(result.trace[6].outcome, { returned: 2 });
  assert.equal(stageState(result.trace, 6).cursor, 2);
  assert.equal(stageState(result.trace, 6).motionCause, 'programmatic');
});

test('review: navigation executors honor smooth and instant settings with real host frames', async () => {
  const smooth = scenarioOf('plugins/stage-feeds.json', 'executors-navigate-smoothly');
  const result = await runScenario(smooth);
  assert.deepEqual(
    [4, 7, 10].map((i) => result.trace[i].outcome),
    [{ returned: true }, { returned: true }, { returned: true }],
  );
  assert.equal(stageState(result.trace, 5).camera.zoom, 2);
  assert.equal(stageState(result.trace, 8).cursor, 3);
  assert.equal(stageState(result.trace, 11).cursor, 4);
  const probe = await runScenario({
    ...smooth,
    steps: [...smooth.steps.slice(0, 4), { op: 'advance', ms: 32 }, { op: 'advance', ms: 300 }],
  });
  assert.ok(
    stageState(probe.trace, 4).camera.zoom > 1 && stageState(probe.trace, 4).camera.zoom < 2,
  );
  assert.deepEqual(stageState(probe.trace, 5).camera, { x: 88, y: 1720, zoom: 2 });
  const instant = scenarioOf('plugins/stage-feeds.json', 'navigation-executors');
  const jump = await runScenario({
    ...instant,
    clock: { frames: true },
    steps: [
      ...instant.steps.slice(0, 4),
      { op: 'call', target: { token: 'stage', doc: 'a' }, method: 'isMoving' },
    ],
  });
  assert.deepEqual(jump.trace[4].outcome, { returned: false });
  assert.deepEqual(stageState(jump.trace, 3).camera, { x: 88, y: 1720, zoom: 2 });
});
