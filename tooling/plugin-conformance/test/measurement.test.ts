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
    negative('createMeasurement', 'the page scale is not known yet', 'not-ready'),
  ],
  'pages-inserted-and-removed': [negative('setScale', missing(2), 'not-found')],
  'page-removed-while-a-change-waits': [negative('setPreset', 'no such page', 'not-found')],
  'close-mid-flight': [negative('setScale', 'document instance a#1 was closed', 'instance-closed')],
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
  assert.equal(pureSuite.cases.length, 45);
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
  const empty = { pending: 0, calibration: null, reports: [], loadErrors: {}, localViewports: {} };
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
  assert.deepEqual(pure('page-scale-loading'), {
    measure: null,
    source: 'default',
    ready: false,
    persistent: true,
  });
  const e = { name: 'EngineError', code: 'RuntimeUnavailable', message: 'offline' };
  assert.deepEqual(pure('page-scale-failed-known'), {
    measure: FEET,
    source: 'owned',
    ready: true,
    persistent: true,
    error: e,
  });
  assert.deepEqual(pure('page-scale-failed-empty'), {
    measure: null,
    source: 'default',
    ready: false,
    persistent: false,
    error: e,
  });
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
test('mirrors retry failed reads preserve confirmed data through stale epochs and drop removed pages', async () => {
  const t = (await run('viewport-reads-fail-refresh-and-rehydrate')).trace;
  assert.deepEqual(value(t, 2), {
    measure: null,
    source: 'default',
    ready: false,
    persistent: true,
    error: { name: 'EngineError', code: 'Unknown', message: 'offline' },
  });
  assert.deepEqual(value(t, 5), {
    measure: METRES,
    source: 'owned',
    ready: true,
    persistent: true,
  });
  assert.deepEqual(value(t, 7), value(t, 5));
  assert.deepEqual(value(t, 10), { measure: FEET, source: 'owned', ready: true, persistent: true });
  const e = (await run('refresh-epochs')).trace;
  assert.deepEqual(value(e, 6), {
    measure: null,
    source: 'default',
    ready: false,
    persistent: true,
  });
  assert.deepEqual(value(e, 8), { measure: FEET, source: 'owned', ready: true, persistent: true });
  assert.deepEqual(value(e, 12), value(e, 8));
  assert.deepEqual(value(e, 14), {
    measure: FEET,
    source: 'owned',
    ready: true,
    persistent: true,
    error: { name: 'EngineError', code: 'Unknown', message: 'gone' },
  });
  const removed = (await run('pages-inserted-and-removed')).trace;
  assert.deepEqual(value(removed, 3), {
    measure: FEET,
    source: 'owned',
    ready: true,
    persistent: true,
  });
  assert.deepEqual(value(removed, 8), {
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
    assert.deepEqual(event.origin.tx, { id: call.args[1].opId, index: 0, count: 1 });
    assert.equal(event.meta.opId, call.args[1].opId);
    assert.deepEqual(event.meta.changed, [dto.ref]);
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
test('scale changes remeasure actual refs skip locked records and clear back to mirrored default', async () => {
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
  const writes = calls(t, 'apply');
  assert.equal(writes.length, 2);
  for (const [i, write] of writes.entries()) {
    const change = write.args[0] as { ops: unknown[] };
    assert.deepEqual(change.ops, [
      {
        type: 'annotations.update',
        ref: objRef(1),
        patch: { subtype: 'line', measure: i === 0 ? FEET : value<PageScale>(t, 15).measure },
      },
    ]);
  }
  assert.deepEqual(
    calls(t, 'page.measure.setScale').map((c) => c.args),
    [[FEET], [METRES], [null]],
  );
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
  const responses = scenario('set-scale-on-one-page').engine!.documents!.a!.responses!.filter(
    (r) => r.path === 'apply',
  );
  assert.equal(responses.length, 2);
  for (const [i, response] of responses.entries()) {
    assert.equal(response.events?.length, 1);
    const event = response.events![0]!;
    assert.ok(event.type === 'annotations.updated');
    assert.equal(event.appearance.changed, true);
    assert.deepEqual(event.meta.changed, [objRef(1)]);
    assert.equal(event.origin.tx?.id, (writes[i]!.args[1] as { opId: string }).opId);
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
  assert.deepEqual(
    settlement(t, 8),
    [1, 2].map((page) => ({
      page: pr(page),
      updated: [],
      skipped: [],
      failed: [],
      scaleError: {
        name: 'EngineError',
        code: 'InvalidArg',
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
  assert.equal(calls(t, 'apply').length, 2);
  assert.equal(calls(t, 'page.measure.setScale').length, 4);
});
test('preflight refusals recover to valid authored formats origin CYX and calibration ratio', async () => {
  const t = (await run('set-scale-preflight')).trace;
  assert.equal(calls(t, 'page.measure.setScale').length, 3);
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
  assert.equal(calls(t, 'page.measure.setScale').length, 9);
});
test('engine writes serialize per page recover after refusal and re-read confirmed viewports', async () => {
  const t = (await run('service-writes-and-foreign-scales')).trace;
  assert.equal(calls(t.slice(0, 8), 'page.measure.setScale').length, 1);
  assert.deepEqual(
    calls(t, 'page.measure.setScale').map((c) => c.args),
    [[FEET], [METRES], [METRES]],
  );
  assert.deepEqual(value(t, 9), {
    measure: METRES,
    source: 'owned',
    ready: true,
    persistent: true,
  });
  assert.deepEqual(value(t, 10), [
    { page: pr(1), scale: METRES, updated: [], skipped: [], failed: [] },
  ]);
  assert.equal(calls(t, 'page.measure.listViewports').length, 4);
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
  assert.equal(calls(listener, 'page.measure.setScale').length, 2);
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
  assert.equal(calls(deleted, 'page.measure.setScale').length, 1);
  assert.equal(calls(deleted, 'apply').length, 1);
  assert.equal(value(deleted, 6), false);
  const closed = (await run('close-mid-flight')).trace;
  assert.equal(value(closed, 5), true);
  assert.deepEqual(
    settlement(closed, 2),
    [1, 2].map((page) => ({
      page: pr(page),
      updated: [],
      skipped: [],
      failed: [],
      scaleError: {
        name: 'EngineError',
        code: 'Unknown',
        message: 'document instance a#1 was closed',
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
test('refused annotation creation rolls back and a loading scale recovers without creating early', async () => {
  const t = (await run('create-measurement')).trace;
  assert.equal(t.flatMap((t) => t.callbacks).filter((c) => c.fn === 'created').length, 3);
  const last = t.at(-1)!;
  assert.ok(last.outcome && 'returned' in last.outcome);
  assert.equal(last.outcome.returned, true);
  const getIndex = scenario('create-measurement').steps.findIndex(
    (s) => s.op === 'call' && s.method === 'get',
  );
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
      assert.deepEqual(event.origin.tx, { id: result.meta.opId, index: 0, count: 1 });
    }
  }
});

import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { gunzipSync } from 'node:zlib';
test('the complete original B3 archive and all 61 measurement mappings remain exact', async () => {
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
    additions: { name: string }[];
  };
  assert.deepEqual(
    [accounting.original, accounting.current, accounting.added, accounting.mappings.length],
    [61, 64, 3, 61],
  );
  const current = suites.flatMap((s) =>
    s.cases.map((c) => ({ file: s.file, name: c.name, input: canonical(c) })),
  );
  assert.equal(current.length, 64);
  assert.equal(new Set(current.map((c) => c.file + ':' + c.name)).size, 64);
  for (const row of accounting.mappings) {
    const before = originals.find((c) => c.file === row.before.file && c.name === row.before.name),
      after = current.find((c) => c.file === row.after.file && c.name === row.after.name);
    assert.ok(before && after);
    assert.equal(row.before.inputSha256, before.inputSha256);
    assert.equal(row.after.inputSha256, digest(JSON.stringify(after.input)));
    assert.ok(row.evidence.length && row.reason);
    assert.equal(row.task11InputDrift, false);
  }
  assert.deepEqual(
    accounting.additions.map((c) => c.name),
    ['page-scale-loading', 'page-scale-failed-known', 'page-scale-failed-empty'],
  );
  const baseline = JSON.parse(await readFile(new URL('summary.json', history), 'utf8')) as {
    archive: { name: string; sha256: string }[];
  };
  for (const source of baseline.archive) {
    assert.equal(digest(await readFile(new URL(source.name + '.ts.txt', history))), source.sha256);
    if (source.name !== 'measurement' && source.name !== 'redaction')
      assert.equal(
        digest(await readFile(new URL('../src/suites/' + source.name + '.ts', import.meta.url))),
        source.sha256,
        source.name + ' deferred bytes',
      );
  }
  const signature = JSON.parse(
    await readFile(new URL('c-signature-inputs.json', history), 'utf8'),
  ) as { sources: { path: string; sha256: string }[] };
  for (const source of signature.sources)
    assert.equal(
      digest(await readFile(new URL('../' + source.path, import.meta.url))),
      source.sha256,
    );
});
