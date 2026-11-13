import assert from 'node:assert/strict';
import { test } from 'node:test';
import { suites } from '../src/suites/measurement';
import { runScenario } from '../src/scenario';

test('measurement reads a loaded scale and a successful dimension without stale engine calls', async () => {
  const suite = suites.find((s) => s.kind === 'scenario')!;
  assert.equal(suite.kind, 'scenario');
  const result = await runScenario(
    suite.cases.find((c) => c.name === 'reads-without-a-measure-service')!,
  );
  assert.deepEqual(result.unmatched, []);
  assert.deepEqual(
    result.trace.flatMap((t) => (t.outcome && 'threw' in t.outcome ? [t.outcome] : [])),
    [],
  );
  assert.ok(
    result.trace.some(
      (t) =>
        t.outcome &&
        'returned' in t.outcome &&
        JSON.stringify(t.outcome.returned).includes('2.00 m'),
    ),
  );
});

test('created measurement responses contain the actual drawn geometry and captions', async () => {
  const suite = suites.find((s) => s.kind === 'scenario')!;
  assert.equal(suite.kind, 'scenario');
  const result = await runScenario(suite.cases.find((c) => c.name === 'create-measurement')!);
  const values = result.trace
    .flatMap((t) => t.settled)
    .flatMap((s) =>
      'resolved' in s && s.resolved && typeof s.resolved === 'object' && 'annotation' in s.resolved
        ? [s.resolved.annotation]
        : [],
    );
  assert.equal(values.length, 3);
  for (const value of values) {
    assert.ok(value && typeof value === 'object' && 'rect' in value);
    const rect = value.rect as { width: number; height: number };
    assert.ok(
      rect.width > 40 && rect.height > 0,
      'confirmed drawing bounds must include line/shape/caption',
    );
  }
});

import { canonical, canonicalError } from '../src/canonical';
import type { FunctionSuite } from '../src/vectors';
import type { TraceEntry, ScenarioCase } from '../src/scenario';
import { AnnotationSchema } from '../../../packages/engine/core/src/annotation/kinds';
import { annotationOfNew } from '../../../packages/core/annotation/src/record';
import type { AnnotationDraft } from '@embedpdf/engine-core/runtime';
import { METRES, FEET, FEET_INCHES, pr, objRef } from '../src/suites/measurement-fixtures';
const scenarioSuite = suites.find((s) => s.kind === 'scenario')!;
assert.equal(scenarioSuite.kind, 'scenario');
const pureSuite = suites.find((s) => s.kind === 'function') as FunctionSuite;
const runCache = new Map<string, ReturnType<typeof runScenario>>();
function scenario(name: string): ScenarioCase {
  const input = scenarioSuite.cases.find((c) => c.name === name);
  assert.ok(input, name);
  return input;
}
function run(name: string) {
  let result = runCache.get(name);
  if (!result) {
    result = runScenario(scenario(name));
    runCache.set(name, result);
  }
  return result;
}
function value<T = unknown>(trace: TraceEntry[], i: number): T {
  const o = trace[i]?.outcome;
  assert.ok(o && 'returned' in o, `step ${i} must return`);
  return o.returned as T;
}
const error = (
  message: string,
  code: string,
  cause?: unknown,
  permission: string | null = null,
) => ({
  $error: {
    name: 'PluginError',
    message,
    code,
    capability: 'measurement',
    permission,
    ...(cause ? { cause } : {}),
  },
});
const engine = (message: string, code = 'InvalidArg') => ({
  $error: { name: 'EngineError', message, code },
});
const range = (message: string) => ({ $error: { name: 'RangeError', message } });
const negative = (
  method: string,
  message: string,
  code: string,
  cause?: unknown,
  permission: string | null = null,
) => ({ method, error: error(message, code, cause, permission) });
const missing = (page = 9) => `page ${page} is not in this document`;
const expectedRejections: Record<string, unknown[]> = {
  'reads-without-a-measure-service': [negative('ensureLoaded', 'no such page', 'not-found')],
  'set-scale-on-one-page': [
    negative(
      'setScale',
      'Only rectilinear measures can be authored',
      'invalid-input',
      engine('Only rectilinear measures can be authored'),
    ),
    negative('setScale', 'Missing x formats', 'invalid-input', engine('Missing x formats')),
    negative('setScale', missing(), 'not-found'),
  ],
  'set-scale-on-several-pages': [
    negative('setScale', missing(), 'not-found'),
    negative('setScale', missing(), 'not-found'),
  ],
  'set-scale-preflight': [
    'Invalid number format',
    'Invalid number format',
    'Invalid number format',
    'Invalid number format',
    'Invalid measurement text',
    'Missing distance formats',
    'Value is outside PDF float range',
  ].map((message) => negative('setScale', message, 'invalid-input', engine(message))),
  'units-precision-and-presets': [
    negative(
      'setPrecision',
      'Invalid decimal precision',
      'operation-failed',
      range('Invalid decimal precision'),
    ),
    negative(
      'setUnit',
      'This scale has an unknown base unit; calibrate it first',
      'operation-failed',
      range('This scale has an unknown base unit; calibrate it first'),
    ),
    negative(
      'setAreaUnit',
      'This scale has an unknown base unit; calibrate it first',
      'operation-failed',
      range('This scale has an unknown base unit; calibrate it first'),
    ),
    negative('setPreset', "unknown scale preset 'nope'", 'not-found'),
  ],
  'service-writes-and-foreign-scales': [
    negative('setUnit', 'calibrate this page before changing its units or precision', 'not-ready'),
    negative(
      'setPrecision',
      'calibrate this page before changing its units or precision',
      'not-ready',
    ),
    negative('setScale', 'read-only', 'operation-failed', engine('read-only', 'Refused')),
  ],
  'calibration-flow': [
    negative(
      'calibrate',
      'Calibration requires finite positive lengths and scales',
      'invalid-input',
    ),
    negative(
      'calibrate',
      'Calibration requires finite positive lengths and scales',
      'invalid-input',
    ),
    negative('calibrate', missing(), 'not-found'),
  ],
  'without-authority': [
    ...['setScale', 'clearScale', 'calibrate'].map((method) =>
      negative(
        method,
        'changing a scale requires doc.annotate.modify',
        'permission-denied',
        undefined,
        'doc.annotate.modify',
      ),
    ),
    negative(
      'createMeasurement',
      'measurement.createMeasurement requires annotations:create',
      'permission-denied',
      undefined,
      'annotations:create',
    ),
  ],
  'create-measurement': [
    ...Array.from({ length: 3 }, () =>
      negative('createMeasurement', 'a distance needs exactly two points', 'invalid-input'),
    ),
    negative('createMeasurement', missing(), 'not-found'),
    {
      method: 'createMeasurement',
      error: {
        $error: {
          name: 'PluginError',
          message: 'measurement create refused',
          code: 'operation-failed',
          capability: 'annotation',
          permission: null,
          cause: engine('measurement create refused', 'Refused'),
        },
      },
    },
  ],
  'pages-inserted-and-removed': [negative('setScale', missing(2), 'not-found')],
  // Upstream 8bc6546e5: the change is sent at once, so the engine refuses the page it removed.
  'page-removed-while-a-change-waits': [
    negative('setScale', 'no page 1', 'not-found', engine('no page 1', 'NotFound')),
  ],
  // Upstream 8bc6546e5: the change queue refuses what the close cut short.
  'close-mid-flight': [negative('setScale', 'the document closed', 'instance-closed')],
};
const expectedReports: Record<string, unknown[]> = {
  'listener-throws': [{ $error: { name: 'Error', message: 'listener broke' } }],
  'close-mid-flight': [
    error('a state update after the instance closed (a#1); dropped', 'instance-closed'),
  ],
};
for (const input of scenarioSuite.cases)
  test(`${input.name}: exact diagnostic channels and complete execution`, async () => {
    const previous = {
      error: console.error,
      warn: console.warn,
      crypto: Object.getOwnPropertyDescriptor(crypto, 'getRandomValues'),
    };
    const before = canonical(input);
    const result = await run(input.name);
    assert.deepEqual(canonical(input), before);
    assert.deepEqual(result.unmatched, []);
    assert.equal(result.trace.length, input.steps.length);
    const thrown = result.trace.flatMap((t, i) =>
      t.outcome && 'threw' in t.outcome
        ? [
            {
              method: input.steps[i]?.op === 'call' ? input.steps[i].method : null,
              error: t.outcome.threw,
            },
          ]
        : [],
    );
    assert.deepEqual(
      thrown,
      input.name === 'without-authority'
        ? [
            negative(
              'startCalibration',
              'measurement.startCalibration requires doc.annotate.modify',
              'permission-denied',
              undefined,
              'doc.annotate.modify',
            ),
          ]
        : [],
    );
    const rejected = result.trace
      .flatMap((t) => t.settled)
      .filter((s) => 'rejected' in s)
      .map((s) => {
        const i = result.trace.findIndex(
          (t) => t.outcome && 'promise' in t.outcome && t.outcome.promise === s.promise,
        );
        const step = input.steps[i];
        assert.ok(step?.op === 'call');
        return { method: step.method, error: s.rejected };
      });
    assert.deepEqual(rejected, expectedRejections[input.name] ?? []);
    assert.deepEqual(
      result.trace.flatMap((t) => t.reports),
      expectedReports[input.name] ?? [],
    );
    assert.deepEqual(
      result.trace.flatMap((t) => t.callbacks).filter((c) => c.fn.startsWith('console.')),
      [],
    );
    const promised = result.trace.flatMap((t) =>
      t.outcome && 'promise' in t.outcome ? [t.outcome.promise] : [],
    );
    const settled = new Set(result.trace.flatMap((t) => t.settled).map((s) => s.promise));
    assert.deepEqual(
      promised.filter((p) => !settled.has(p)),
      [],
    );
    assert.equal(console.error, previous.error);
    assert.equal(console.warn, previous.warn);
    assert.deepEqual(Object.getOwnPropertyDescriptor(crypto, 'getRandomValues'), previous.crypto);
  });

function pure(name: string): unknown {
  const c = pureSuite.cases.find((c) => c.name === name);
  assert.ok(c, name);
  const args = structuredClone(c.args),
    before = canonical(args);
  try {
    return pureSuite.functions[c.fn]!(...(args as never[]));
  } finally {
    assert.deepEqual(canonical(args), before, `${name}: invoked args mutated`);
  }
}
const pureNegatives: Record<string, unknown> = {
  ...Object.fromEntries(
    [
      'with-unit-unknown-base',
      'with-unit-no-x',
      'with-area-unit-unknown-base',
      'with-area-unit-no-x',
    ].map((name) => [name, range('This scale has an unknown base unit; calibrate it first')]),
  ),
  ...Object.fromEntries(
    ['with-precision-too-big', 'with-precision-not-power', 'with-precision-fraction'].map(
      (name) => [name, range('Invalid decimal precision')],
    ),
  ),
  'default-measure-bad-user-unit': engine(
    'Calibration requires finite positive lengths and scales',
  ),
};
test('all current pure inputs invoke shipping functions with exactly eight named refusals and recovery', () => {
  // 41: upstream 8bc6546e5 removed clearLoadError and pageScaleOf (four cases).
  assert.equal(pureSuite.cases.length, 41);
  let refused = 0;
  for (const c of pureSuite.cases) {
    let outcome: unknown;
    try {
      outcome = { returned: pure(c.name) };
    } catch (e) {
      outcome = { threw: canonicalError(e) };
    }
    if (c.name in pureNegatives) {
      assert.deepEqual(outcome, { threw: pureNegatives[c.name] }, c.name);
      refused++;
    } else {
      assert.ok(outcome && typeof outcome === 'object' && 'returned' in outcome, c.name);
      assert.ok(!JSON.stringify(outcome).includes('removed upstream'));
    }
  }
  assert.equal(refused, 8);
  assert.deepEqual(pure('with-unit'), {
    ...METRES,
    distance: [{ unit: 'ft', conversion: 3.280839895013123, precision: 100, fraction: 'decimal' }],
    area: [{ unit: 'ft²', conversion: 10.763910416709722, precision: 100, fraction: 'decimal' }],
  });
  assert.deepEqual(pure('with-precision-one'), {
    ...METRES,
    distance: [{ unit: 'm', conversion: 1, precision: 1, fraction: 'decimal' }],
    area: [{ unit: 'm²', conversion: 1, precision: 1, fraction: 'decimal' }],
  });
});
test('current state transitions and full scale projection fields replace cached PAGE_SCALE', () => {
  // Upstream 8bc6546e5 removed loadErrors with the viewport mirror.
  const empty = { pending: 0, calibration: null, reports: [], localViewports: {} };
  assert.deepEqual(pure('initial-state'), empty);
  assert.deepEqual(pure('reduce-pending-up'), { ...empty, pending: 2 });
  assert.deepEqual(pure('reduce-pending-floors-at-zero'), empty);
  const c = pureSuite.cases.find((c) => c.name === 'reduce-page-scale')!;
  assert.equal(c.fn, 'setLocalViewports');
  assert.deepEqual(pure(c.name), {
    ...empty,
    pending: 1,
    localViewports: {
      2: [
        { bbox: { x: 0, y: 0, width: 400, height: 500 }, measure: METRES, owned: true, name: null },
      ],
    },
  });
  assert.deepEqual(pure('select-owned'), {
    measure: FEET,
    source: 'owned',
    ready: true,
    persistent: true,
  });
  assert.deepEqual(pure('select-last-containing'), {
    measure: FEET,
    source: 'foreign',
    ready: true,
    persistent: false,
  });
  assert.deepEqual(pure('select-foreign-without-measure'), {
    measure: null,
    source: 'foreign',
    ready: true,
    persistent: false,
  });
  assert.deepEqual(pure('select-default'), {
    measure: METRES,
    source: 'default',
    ready: true,
    persistent: false,
  });
  assert.deepEqual(pure('select-none'), {
    measure: FEET,
    source: 'default',
    ready: true,
    persistent: true,
  });
  assert.deepEqual(pure('select-beside-the-centre'), {
    measure: METRES,
    source: 'default',
    ready: true,
    persistent: false,
  });
  assert.deepEqual(pure('select-on-a-raised-crop'), {
    measure: FEET,
    source: 'foreign',
    ready: true,
    persistent: false,
  });
  for (const entry of pureSuite.cases.filter((c) => c.fn === 'selectPageScale'))
    assert.deepEqual(Object.keys(entry.args[1] as object), ['width', 'height']);
  // pageScaleOf and its three read states went with the mirror (upstream 8bc6546e5).
  for (const name of ['page-scale-loading', 'page-scale-failed-known', 'page-scale-failed-empty'])
    assert.equal(
      pureSuite.cases.some((c) => c.name === name),
      false,
    );
  assert.equal('pageScaleOf' in pureSuite.functions, false);
  assert.equal('clearLoadError' in pureSuite.functions, false);
});
test('pure scale units precision fractions presets and explicit user units remain meaningful', () => {
  const imperial = pure('default-measure-imperial-user-unit') as typeof FEET;
  assert.equal(imperial.x[0]?.unit, 'ft');
  assert.ok(Math.abs(imperial.x[0]!.conversion! - 1 / 432) < 1e-9);
  assert.deepEqual(pure('default-measure-object'), FEET);
  assert.equal((pure('default-presets') as unknown[]).length, 8);
  const fraction = pure('with-area-unit-from-fraction') as typeof FEET;
  assert.deepEqual(fraction.distance, FEET_INCHES.distance);
  assert.equal(fraction.area[0]?.unit, 'yd²');
  assert.equal(fraction.area[0]?.precision, 100);
  const kept = pure('with-unit-keeps-precision') as typeof FEET;
  assert.equal(kept.distance[0]?.precision, 1000);
  const precision = pure('with-precision') as typeof FEET;
  assert.deepEqual(precision.distance[0], FEET_INCHES.distance[0]);
  assert.deepEqual(precision.distance[1], {
    unit: 'in',
    conversion: 12,
    precision: 1000,
    fraction: 'decimal',
  });
  assert.deepEqual((pure('with-precision-empty-lists') as typeof FEET).distance, []);
});

test('loaded scales read real labels geometry foreign and unavailable results', async () => {
  const t = (await run('reads-without-a-measure-service')).trace;
  assert.deepEqual(value(t, 2), {
    measure: {
      subtype: 'rectilinear',
      ratio: '1:1',
      x: [{ unit: 'm', conversion: 0.00035277777351439, precision: 100 }],
      distance: [{ unit: 'm', conversion: 1, precision: 100 }],
      area: [{ unit: 'm²', conversion: 1, precision: 100 }],
    },
    source: 'default',
    ready: true,
    persistent: true,
  });
  assert.deepEqual(value(t, 20), { kind: 'distance', value: 2.000000014901161, label: '2.00 m' });
  assert.deepEqual(value(t, 17), { unavailable: 'invalid-geometry' });
  assert.deepEqual(value(t, 18), { unavailable: 'no-measure' });
  assert.deepEqual(value(t, 21), { unavailable: 'not-dimension' });
  const foreign = (await run('measure-service-viewports')).trace;
  assert.deepEqual(value(foreign, 2), {
    measure: FEET,
    source: 'owned',
    ready: true,
    persistent: true,
  });
  assert.deepEqual(value(foreign, 3), {
    measure: { subtype: 'geospatial' },
    source: 'foreign',
    ready: true,
    persistent: true,
  });
  assert.deepEqual(value(foreign, 4), {
    measure: null,
    source: 'foreign',
    ready: true,
    persistent: true,
  });
  assert.deepEqual(value(foreign, 8), { unavailable: 'foreign-measure' });
});
// Upstream 8bc6546e5 removed the viewports mirror, its read errors and its epochs: a page's
// scale rides on its layout. These pin what replaced them.
test('layouts carry scales: events move them, a desync re-reads them, removed pages drop', async () => {
  const t = (await run('scale-events-move-the-layout')).trace;
  // Known at open, with no read of the viewports.
  assert.deepEqual(value(t, 2), { measure: METRES, source: 'owned', ready: true, persistent: true });
  assert.equal(value(t, 3), true);
  assert.equal(calls(t, 'page.measure.listViewports').length, 0);
  // Another session's pages.scaleSet moves page 1.
  assert.deepEqual(value(t, 6), { measure: FEET, source: 'owned', ready: true, persistent: true });
  assert.equal(value(t, 7), false);
  // stream.desynced reads the page list again (a4805a492) and takes page 2's scale from it.
  assert.deepEqual(
    t[8]!.engine.map((e) => (e as { path: string }).path),
    ['pages.list', 'annotations.list'],
  );
  assert.deepEqual(value(t, 9), { measure: METRES, source: 'owned', ready: true, persistent: true });
  // document.versioned and a layout with the same scales change neither page's.
  assert.deepEqual(value(t, 12), value(t, 6));
  assert.deepEqual(value(t, 13), value(t, 9));
  const removed = (await run('pages-inserted-and-removed')).trace;
  assert.deepEqual(value(removed, 3), { measure: FEET, source: 'owned', ready: true, persistent: true });
  assert.deepEqual(value(removed, 5), value(removed, 3));
  assert.deepEqual(value(removed, 7), {
    measure: null,
    source: 'default',
    ready: false,
    persistent: false,
  });
});
test('calibration captures authored endpoints preserves other-page requests and dismisses once', async () => {
  const t = (await run('calibration-flow')).trace;
  const callbacks = t.flatMap((t) => t.callbacks);
  assert.deepEqual(
    callbacks.filter((c) => c.fn === 'calibrationRequested').map((c) => c.args),
    [
      [
        {
          request: {
            page: pr(1),
            from: { x: 100, y: 100 },
            to: { x: 244, y: 100 },
            userSpaceLength: 144,
          },
        },
      ],
      [
        {
          request: {
            page: pr(2),
            from: { x: 10, y: 10 },
            to: { x: 10, y: 110 },
            userSpaceLength: 100,
          },
        },
      ],
      [
        {
          request: {
            page: pr(2),
            from: { x: 10, y: 10 },
            to: { x: 10, y: 110 },
            userSpaceLength: 100,
          },
        },
      ],
    ],
  );
  assert.equal(callbacks.filter((c) => c.fn === 'calibrationCompleted').length, 2);
  assert.equal(callbacks.filter((c) => c.fn === 'calibrationDismissed').length, 1);
  assert.deepEqual(value(t, 13), {
    page: pr(1),
    from: { x: 100, y: 100 },
    to: { x: 244, y: 100 },
    userSpaceLength: 144,
  });
  assert.equal(value(t, 15), null);
  assert.deepEqual(value(t, 22), {
    page: pr(2),
    from: { x: 10, y: 10 },
    to: { x: 10, y: 110 },
    userSpaceLength: 100,
  });
  assert.equal(value(t, 24), null);
  assert.equal(value(t, 36), null);
  const zero = (await run('zero-length-capture')).trace;
  assert.deepEqual(
    zero.flatMap((t) => t.callbacks),
    [
      {
        fn: 'captured',
        args: [{ tool: 'calibrate', page: pr(1), from: { x: 10, y: 10 }, to: { x: 10, y: 10 } }],
      },
    ],
  );
  assert.equal(value(zero, 6), null);
});

test('real annotation creates carry complete geometry styles labels refs resources and confirmed tx', async () => {
  const t = (await run('create-measurement')).trace;
  const calls = t
    .flatMap((t) => t.engine)
    .filter(
      (
        c,
      ): c is {
        path: string;
        args: [
          { ops: { type: string; objectNumber: number; data: AnnotationDraft; page: unknown }[] },
          { opId: string },
        ];
      } => !!c && typeof c === 'object' && 'path' in c && c.path === 'apply',
    );
  assert.equal(calls.length, 4);
  for (const [i, call] of calls.slice(0, 3).entries()) {
    const ops = call.args[0].ops;
    assert.equal(ops.length, 1);
    const op = ops[0]!;
    assert.equal(op.type, 'annotations.create');
    assert.equal(op.objectNumber, 10001 + i);
    assert.deepEqual(op.page, pr(1));
    assert.ok(
      op.data.subtype === 'line' || op.data.subtype === 'polyline' || op.data.subtype === 'polygon',
    );
    assert.equal(op.data.strokeWidth, 1);
    assert.equal(op.data.color, '#e5484d');
    assert.equal(op.data.opacity, 1);
    assert.ok('measure' in op.data);
    assert.equal(op.data.measure?.subtype, 'rectilinear');
    assert.ok('captionEnabled' in op.data && op.data.captionEnabled === true);
    const settled = t
      .flatMap((t) => t.settled)
      .find(
        (s) =>
          'resolved' in s &&
          s.resolved &&
          typeof s.resolved === 'object' &&
          'annotation' in s.resolved &&
          JSON.stringify(s.resolved.annotation).includes(`"objectNumber":${10001 + i}`),
      );
    assert.ok(
      settled &&
        'resolved' in settled &&
        settled.resolved &&
        typeof settled.resolved === 'object' &&
        'annotation' in settled.resolved,
    );
    const dto = AnnotationSchema.parse(settled.resolved.annotation);
    assert.deepEqual(dto, annotationOfNew(op.data, { ref: objRef(10001 + i) }));
    assert.equal(dto.contents, ['0.05 m', '0.03 m', '0.00 m²'][i]);
    assert.deepEqual(dto.ref, objRef(10001 + i));
    const events = scenario('create-measurement')
      .engine!.documents!.a!.responses!.flatMap((r) => r.events ?? [])
      .filter((e) => e.type === 'annotations.created');
    const event = events[i]!;
    assert.ok(event.type === 'annotations.created');
    assert.deepEqual(event.origin.transaction, { id: call.args[1].opId, index: 0, count: 1 });
    assert.equal(event.meta.opId, call.args[1].opId);
    assert.deepEqual(event.meta.changedRefs, [dto.ref]);
    assert.deepEqual(event.annotation, canonical(dto));
  }
  const distance = calls[0]!.args[0].ops[0]!.data;
  assert.ok(distance.subtype === 'line');
  assert.deepEqual(distance.linePoints, { start: { x: 100, y: 100 }, end: { x: 244, y: 100 } });
  assert.deepEqual(distance.lineEndings, { start: 'closed-arrow', end: 'closed-arrow' });
  assert.deepEqual(distance.leader, { length: 12, extension: 5, offset: 0 });
  assert.equal(distance.captionPosition, 'inline');
});

import type { PageScale } from '../../../packages/plugin/measurement/src/contract';
function calls(trace: TraceEntry[], path: string) {
  return trace
    .flatMap((t) => t.engine)
    .filter(
      (e): e is { path: string; args: unknown[]; pon?: number } =>
        !!e && typeof e === 'object' && 'path' in e && e.path === path,
    );
}
function settlement(trace: TraceEntry[], sourceStep: number) {
  const o = trace[sourceStep]?.outcome;
  assert.ok(o && 'promise' in o);
  const found = trace.flatMap((t) => t.settled).find((s) => s.promise === o.promise);
  assert.ok(found && 'resolved' in found);
  return found.resolved;
}
test('registered imperial and custom defaults preserve explicit presets and userUnit', async () => {
  const t = (await run('imperial-default-and-custom-presets')).trace;
  assert.deepEqual(value(t, 4), { kind: 'distance', value: 0.08333333395421505, label: '0.08 ft' });
  assert.deepEqual(value(t, 3), [
    { id: 'site', label: '1:500', paper: 1, real: 500, unit: 'm' },
    { id: 'plan', label: '2 in = 300 ft', paper: 2, real: 300, unit: 'ft' },
  ]);
  const owned = value<PageScale>(t, 6);
  assert.equal(owned.measure?.subtype, 'rectilinear');
  assert.ok(owned.measure && 'ratio' in owned.measure);
  assert.equal(owned.measure.ratio, '2:300');
  assert.equal(owned.source, 'owned');
  assert.equal(owned.persistent, true);
  const c = (await run('custom-default-measure')).trace;
  assert.deepEqual(value(c, 2), {
    measure: FEET_INCHES,
    source: 'default',
    ready: true,
    persistent: true,
  });
  assert.deepEqual(value(c, 3), value(c, 2));
  assert.deepEqual(value(c, 4), {
    kind: 'distance',
    value: 1.2500000093132257,
    label: '1 ft 3 in',
  });
  const preset = value<PageScale>(c, 6);
  assert.ok(preset.measure?.subtype === 'rectilinear');
  assert.equal(preset.measure.x[0]?.conversion, 0.1111111119389534);
  assert.deepEqual(value(c, 8), value(c, 2));
});
test('scale changes remeasure actual refs skip locked records and clear back to the default', async () => {
  const t = (await run('set-scale-on-one-page')).trace;
  assert.equal(value(t, 7), true);
  assert.equal(value(t, 9), false);
  assert.deepEqual(value(t, 10), { measure: FEET, source: 'owned', ready: true, persistent: true });
  assert.deepEqual(settlement(t, 6), [
    {
      page: pr(1),
      scale: FEET,
      updated: [objRef(1)],
      skipped: [{ ref: objRef(2), reason: 'locked' }],
      failed: [],
    },
  ]);
  assert.deepEqual(value(t, 11), settlement(t, 6));
  // Upstream 8bc6546e5: each scale change is one change: its pages.setScale op, then the
  // re-measure's updates; recalculate false writes the scale alone.
  const writes = calls(t, 'apply');
  assert.equal(writes.length, 3);
  const opsOf = (i: number) => (writes[i]!.args[0] as { ops: unknown[] }).ops;
  assert.deepEqual(opsOf(0), [
    { type: 'pages.setScale', pages: [pr(1)], measure: FEET },
    { type: 'annotations.update', ref: objRef(1), patch: { subtype: 'line', measure: FEET } },
  ]);
  assert.deepEqual(opsOf(1), [{ type: 'pages.setScale', pages: [pr(1)], measure: METRES }]);
  assert.deepEqual(opsOf(2), [
    { type: 'pages.setScale', pages: [pr(1)], measure: null },
    {
      type: 'annotations.update',
      ref: objRef(1),
      patch: { subtype: 'line', measure: value<PageScale>(t, 15).measure },
    },
  ]);
  assert.equal(calls(t, 'page.measure.setScale').length, 0);
  assert.deepEqual(value(t, 13), {
    measure: METRES,
    source: 'owned',
    ready: true,
    persistent: true,
  });
  const reset = value<PageScale>(t, 15);
  assert.equal(reset.source, 'default');
  assert.equal(reset.ready, true);
  assert.equal(reset.persistent, true);
  assert.deepEqual(value(t, 19), settlement(t, 14));
  // Each answer's events: pages.scaleSet, then annotations.updated for the re-measured line
  // (appearance regenerated, changedRefs [objRef(1)]), all under the write's opId.
  const responses = scenario('set-scale-on-one-page').engine!.documents!.a!.responses!.filter(
    (r) => r.path === 'apply',
  );
  assert.equal(responses.length, 3);
  for (const [i, response] of responses.entries()) {
    const opId = (writes[i]!.args[1] as { opId: string }).opId;
    const events = response.events ?? [];
    assert.equal(events.length, opsOf(i).length);
    events.forEach((event, j) => {
      assert.deepEqual(event.origin.transaction, { id: opId, index: j, count: events.length });
    });
    assert.equal(events[0]!.type, 'pages.scaleSet');
    const updated = events[1];
    if (i === 1) {
      assert.equal(updated, undefined, 'recalculate false re-measures nothing');
      continue;
    }
    assert.ok(updated && updated.type === 'annotations.updated');
    assert.equal(updated.appearance.changed, true);
    assert.deepEqual(updated.meta.changedRefs, [objRef(1)]);
  }
});
test('multi-page strict preflight and tolerant scale errors remain separate channels', async () => {
  const t = (await run('set-scale-on-several-pages')).trace;
  assert.deepEqual(
    settlement(t, 6),
    [1, 2].map((page) => ({
      page: pr(page),
      scale: FEET,
      updated: [objRef(page === 1 ? 1 : 20, page)],
      skipped: [],
      failed: [],
    })),
  );
  // Upstream 8bc6546e5: a page's report serializes the plugin error its refusal became, which
  // keeps no engine code.
  assert.deepEqual(
    settlement(t, 8),
    [1, 2].map((page) => ({
      page: pr(page),
      updated: [],
      skipped: [],
      failed: [],
      scaleError: {
        code: 'Unknown',
        message: 'Only rectilinear measures can be authored',
      },
    })),
  );
  const reports = value<
    { page: unknown; scale: typeof FEET; updated: unknown[]; failed: unknown[] }[]
  >(t, 11);
  assert.equal(reports.length, 2);
  for (const r of reports) {
    assert.deepEqual(r.updated, []);
    assert.deepEqual(r.failed, []);
    assert.deepEqual(r.scale.distance, [
      { unit: 'mm', conversion: 304.8, precision: 100, fraction: 'decimal' },
    ]);
  }
  // One change per call, however many pages it covers.
  const writes = calls(t, 'apply');
  assert.equal(writes.length, 2);
  const first = (writes[0]!.args[0] as { ops: { type: string; pages?: unknown }[] }).ops[0]!;
  assert.deepEqual([first.type, first.pages], ['pages.setScale', [pr(1), pr(2)]]);
  assert.equal(calls(t, 'page.measure.setScale').length, 0);
});
test('preflight refusals recover to valid authored formats origin CYX and calibration ratio', async () => {
  const t = (await run('set-scale-preflight')).trace;
  // Three changes reach the engine, each one apply (upstream 8bc6546e5).
  assert.equal(calls(t, 'apply').length, 3);
  const first = value<PageScale>(t, 10);
  assert.ok(first.measure?.subtype === 'rectilinear');
  assert.deepEqual(first.measure.distance, [{ unit: 'm', conversion: 1 }]);
  const second = value<PageScale>(t, 12);
  assert.ok(second.measure?.subtype === 'rectilinear');
  assert.deepEqual(second.measure.origin, { x: 10, y: 20 });
  assert.equal(second.measure.cyx, 1);
  assert.deepEqual(second.measure.distance, [
    { unit: 'm', conversion: 1, precision: 100, labelPosition: 'suffix', fixed: true },
  ]);
  const calibrated = value<PageScale>(t, 14);
  assert.ok(calibrated.measure?.subtype === 'rectilinear');
  assert.equal(calibrated.measure.ratio, '1 in = 10.285714 m');
  assert.ok(Math.abs(calibrated.measure.x[0]!.conversion! - 1 / 7) < 1e-8);
  assert.equal(calibrated.source, 'owned');
});
test('unit changes preserve base conversions imported fractions and valid recovery after bad units', async () => {
  const t = (await run('units-precision-and-presets')).trace;
  const s = (i: number) => {
    const scale = value<PageScale>(t, i);
    assert.ok(scale.measure?.subtype === 'rectilinear');
    return scale.measure;
  };
  assert.deepEqual(s(3).distance, [
    { unit: 'ft', conversion: 3.280839895013123, precision: 100, fraction: 'decimal' },
  ]);
  assert.equal(s(5).area[0]?.unit, 'acre');
  assert.equal(s(7).distance[0]?.precision, 1000);
  assert.equal(s(7).area[0]?.precision, 1000);
  assert.deepEqual(s(11).distance, [
    { unit: 'in', conversion: 12.000000000000002, precision: 100, fraction: 'decimal' },
  ]);
  assert.deepEqual(s(14).distance, FEET_INCHES.distance);
  assert.equal(s(14).area[0]?.unit, 'ha');
  assert.equal(s(19).ratio, '1:50');
  assert.equal(calls(t, 'apply').length, 9);
  assert.equal(calls(t, 'page.measure.setScale').length, 0);
});
test('engine writes land in order, recover after a refusal and take the layout the engine answers', async () => {
  const t = (await run('service-writes-and-foreign-scales')).trace;
  // A foreign scale can't take a unit or precision change: nothing reaches the engine for them.
  assert.equal(calls(t.slice(0, 5), 'apply').length, 0);
  // Upstream 8bc6546e5: three changes, each one apply, in order; the refused one changes nothing.
  assert.deepEqual(
    calls(t, 'apply').map((c) => (c.args[0] as { ops: unknown[] }).ops),
    [FEET, METRES, METRES].map((measure) => [
      { type: 'pages.setScale', pages: [pr(1)], measure },
    ]),
  );
  assert.deepEqual(value(t, 8), {
    measure: METRES,
    source: 'owned',
    ready: true,
    persistent: true,
  });
  assert.deepEqual(value(t, 9), [
    { page: pr(1), scale: METRES, updated: [], skipped: [], failed: [] },
  ]);
  assert.equal(calls(t, 'page.measure.listViewports').length, 0);
});
test('authority refuses before writes and throwing listeners retain successful scale changes', async () => {
  const denied = (await run('without-authority')).trace;
  assert.equal(value(denied, 2), false);
  assert.equal(value(denied, 3), false);
  assert.equal(calls(denied, 'page.measure.setScale').length, 0);
  assert.equal(calls(denied, 'apply').length, 0);
  assert.equal(value(denied, 8), 'pointer');
  assert.equal(value(denied, 10), false);
  const listener = (await run('listener-throws')).trace;
  assert.equal(calls(listener, 'apply').length, 2);
  assert.equal(calls(listener, 'page.measure.setScale').length, 0);
  assert.deepEqual(
    listener.flatMap((t) => t.callbacks).map((c) => c.fn),
    ['loud'],
  );
  assert.deepEqual(settlement(listener, 6), [
    { page: pr(1), scale: METRES, updated: [], skipped: [], failed: [] },
  ]);
});
test('closing and page deletion settle exact channels then a fresh document recovers', async () => {
  const deleted = (await run('page-removed-while-a-change-waits')).trace;
  // Upstream 8bc6546e5: the change waits on the annotations' load, then is one apply, which the
  // engine refuses for the page it removed; no page write of its own.
  assert.equal(calls(deleted, 'page.measure.setScale').length, 0);
  assert.equal(calls(deleted, 'apply').length, 1);
  assert.equal(value(deleted, 6), false);
  assert.deepEqual(value(deleted, 7), []);
  const closed = (await run('close-mid-flight')).trace;
  // Both changes were sent at once; the close refuses them on the queue.
  assert.equal(calls(closed, 'apply').length, 2);
  assert.equal(value(closed, 4), true);
  assert.deepEqual(
    settlement(closed, 2),
    [1, 2].map((page) => ({
      page: pr(page),
      updated: [],
      skipped: [],
      failed: [],
      scaleError: {
        code: 'Unknown',
        message: 'the document closed',
      },
    })),
  );
  assert.deepEqual(
    closed.flatMap((t) => t.reports),
    expectedReports['close-mid-flight'],
  );
  const fresh = await runScenario(scenario('reads-without-a-measure-service'));
  assert.deepEqual(fresh.unmatched, []);
  assert.equal(value<PageScale>(fresh.trace, 2).ready, true);
  assert.deepEqual(
    fresh.trace.flatMap((t) => t.reports),
    [],
  );
});
// The deferred scale read this test also covered went with upstream 8bc6546e5: a page's scale is
// known when the document opens, so a create never waits for it.
test('refused annotation creation rolls back after three confirmed creates', async () => {
  const t = (await run('create-measurement')).trace;
  assert.equal(t.flatMap((t) => t.callbacks).filter((c) => c.fn === 'created').length, 3);
  const getIndex = scenario('create-measurement').steps.findIndex(
    (s) => s.op === 'call' && s.method === 'get',
  );
  assert.equal(getIndex, t.length - 1);
  assert.equal(value(t, getIndex), null);
  assert.equal(calls(t, 'apply').length, 4);
});

test('all authored persistent scale results carry complete matching mutation provenance', () => {
  for (const input of scenarioSuite.cases) {
    const responses = Object.values(input.engine?.documents ?? {})
      .flatMap((doc) => doc.responses ?? [])
      .filter((r) => r.path === 'page.measure.setScale' && !r.error && !r.defer);
    const ids = new Set<string>();
    for (const response of responses) {
      assert.ok(response.value && typeof response.value === 'object' && 'meta' in response.value);
      const result = response.value as {
        page: unknown;
        meta: { opId: string; affectedPages: unknown[]; cacheDelta: null; undoable: boolean };
      };
      assert.deepEqual(result.page, pr(response.pon!));
      assert.deepEqual(result.meta.affectedPages, [result.page]);
      assert.equal(result.meta.cacheDelta, null);
      assert.equal(result.meta.undoable, true);
      assert.ok(result.meta.opId);
      assert.equal(ids.has(result.meta.opId), false);
      ids.add(result.meta.opId);
      assert.equal(response.events?.length, 1);
      const event = response.events![0]!;
      assert.ok(event.type === 'pages.scaleSet');
      assert.deepEqual(event.meta, result.meta);
      assert.deepEqual(event.origin.transaction, { id: result.meta.opId, index: 0, count: 1 });
    }
  }
});

import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { gunzipSync } from 'node:zlib';
test('the complete original B3 archive, its 61 measurement mappings and the 2026-11-12 re-accounting remain exact', async () => {
  const history = new URL('../historical/task-12b2b3/', import.meta.url);
  const digest = (data: string | Uint8Array) => createHash('sha256').update(data).digest('hex');
  const hashes = JSON.parse(
    await readFile(new URL('archive-sha256.json', history), 'utf8'),
  ) as Record<string, string>;
  for (const [file, hash] of Object.entries(hashes)) {
    const bytes = file.endsWith('.uncompressed')
      ? gunzipSync(await readFile(new URL('inputs-runtime.json.gz', history)))
      : await readFile(new URL(file, history));
    assert.equal(digest(bytes), hash, file);
  }
  const originals = JSON.parse(
    await readFile(new URL('original-inputs.json', history), 'utf8'),
  ) as { source: string; file: string; name: string; input: unknown; inputSha256: string }[];
  assert.equal(originals.length, 211);
  assert.equal(new Set(originals.map((r) => r.file + ':' + r.name)).size, 211);
  for (const row of originals)
    assert.equal(digest(JSON.stringify(row.input)), row.inputSha256, row.name);
  const accounting = JSON.parse(
    await readFile(new URL('../historical/task-12b2b3-i/accounting.json', import.meta.url), 'utf8'),
  ) as {
    original: number;
    current: number;
    added: number;
    mappings: {
      before: { file: string; name: string; inputSha256: string };
      after: { file: string; name: string; inputSha256: string };
      task11InputDrift: boolean;
      evidence: string[];
      reason: string;
    }[];
    additions: { name: string; file: string; inputSha256: string }[];
  };
  assert.deepEqual(
    [accounting.original, accounting.current, accounting.added, accounting.mappings.length],
    [61, 64, 3, 61],
  );
  // The 2026-11-12 catch-up (upstream c7f8d75ff, 8bc6546e5) rewrote the suite. task-12b2b3-i's
  // accounting still pins the 64 cases it left current; the catch-up's accounting maps each of
  // those to the case that carries it now, or retires it with its reason.
  const catchUp = JSON.parse(
    await readFile(
      new URL('../historical/catchup-2026-11-12-measurement/accounting.json', import.meta.url),
      'utf8',
    ),
  ) as {
    previousAccounting: { path: string; sha256: string };
    previous: number;
    current: number;
    unchanged: number;
    rewritten: number;
    retired: number;
    added: number;
    mappings: {
      before: { file: string; name: string; inputSha256: string };
      after: { file: string; name: string; inputSha256: string } | null;
      disposition: 'unchanged' | 'rewritten' | 'retired';
      reason: string;
      upstream: string[];
    }[];
    additions: { file: string; name: string; inputSha256: string; reason: string; upstream: string[] }[];
  };
  assert.equal(
    digest(
      await readFile(new URL('../historical/task-12b2b3-i/accounting.json', import.meta.url)),
    ),
    catchUp.previousAccounting.sha256,
  );
  const previous = [
    ...accounting.mappings.map((m) => m.after),
    ...accounting.additions,
  ].map(({ file, name, inputSha256 }) => ({ file, name, inputSha256 }));
  assert.deepEqual(
    catchUp.mappings.map((m) => m.before),
    previous,
  );
  for (const row of accounting.mappings) {
    const before = originals.find((c) => c.file === row.before.file && c.name === row.before.name);
    assert.ok(before);
    assert.equal(row.before.inputSha256, before.inputSha256);
    assert.ok(row.evidence.length && row.reason);
    assert.equal(row.task11InputDrift, false);
  }
  const current = suites.flatMap((s) =>
    s.cases.map((c) => ({ file: s.file, name: c.name, input: canonical(c) })),
  );
  assert.equal(current.length, 60);
  assert.equal(new Set(current.map((c) => c.file + ':' + c.name)).size, 60);
  const mapped = new Set<string>();
  for (const row of catchUp.mappings) {
    assert.ok(row.reason);
    if (!row.after) {
      assert.equal(row.disposition, 'retired');
      assert.ok(row.upstream.length);
      continue;
    }
    const after = current.find((c) => c.file === row.after!.file && c.name === row.after!.name);
    assert.ok(after, row.after.name);
    assert.equal(row.after.inputSha256, digest(JSON.stringify(after.input)));
    assert.equal(
      row.disposition,
      row.after.inputSha256 === row.before.inputSha256 ? 'unchanged' : 'rewritten',
    );
    assert.equal(mapped.has(row.after.file + ':' + row.after.name), false);
    mapped.add(row.after.file + ':' + row.after.name);
  }
  for (const row of catchUp.additions) {
    const after = current.find((c) => c.file === row.file && c.name === row.name);
    assert.ok(after && row.reason && row.upstream.length, row.name);
    assert.equal(row.inputSha256, digest(JSON.stringify(after.input)));
    assert.equal(mapped.has(row.file + ':' + row.name), false);
    mapped.add(row.file + ':' + row.name);
  }
  assert.equal(mapped.size, 60);
  assert.deepEqual(
    [
      catchUp.previous,
      catchUp.current,
      catchUp.unchanged,
      catchUp.rewritten,
      catchUp.retired,
      catchUp.added,
    ],
    [64, 60, 36, 22, 6, 2],
  );
  assert.deepEqual(
    accounting.additions.map((c) => c.name),
    ['page-scale-loading', 'page-scale-failed-known', 'page-scale-failed-empty'],
  );
  const baseline = JSON.parse(await readFile(new URL('summary.json', history), 'utf8')) as {
    archive: { name: string; sha256: string }[];
  };
  for (const source of baseline.archive) {
    assert.equal(digest(await readFile(new URL(source.name + '.ts.txt', history))), source.sha256);
  }
  // Signature and stamp now use current fixtures; their former active-source freeze is retired.
});
