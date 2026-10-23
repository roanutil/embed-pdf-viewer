import assert from 'node:assert/strict';
import { readdir } from 'node:fs/promises';
import { test } from 'node:test';
import { runScenario, type ScenarioCase } from '../src/scenario';
import type { FunctionSuite, Suite } from '../src/vectors';

const loaded: Suite[] = [];
for (const name of (await readdir(new URL('../src/suites/', import.meta.url)))
  .filter((name) => name.endsWith('.ts'))
  .sort()) {
  const module = await import(new URL('../src/suites/' + name, import.meta.url).href);
  assert.ok(Array.isArray(module.suites), name + ' exports discoverable suites');
  loaded.push(...module.suites);
}
const scenario = (file: string, name: string): ScenarioCase => {
  const suite = loaded.find((suite) => suite.file === file);
  assert.ok(suite && suite.kind === 'scenario');
  const input = suite.cases.find((entry) => entry.name === name);
  assert.ok(input, name);
  return input;
};
const functions = (file: string): FunctionSuite['functions'] => {
  const suite = loaded.find((suite) => suite.file === file);
  assert.ok(suite && suite.kind === 'function');
  return suite.functions;
};
const call = (file: string, name: string, ...args: unknown[]): unknown =>
  (functions(file)[name] as (...args: unknown[]) => unknown)(...args);

test('render keeps different page rasters distinct and cancels tile work on close', async () => {
  const batch = await runScenario(scenario('plugins/render-capability.json', 'render-pages-batch'));
  assert.deepEqual(batch.unmatched, []);
  const images = batch.trace
    .flatMap((entry) => entry.engine ?? [])
    .filter(
      (entry) =>
        entry !== null &&
        typeof entry === 'object' &&
        'path' in entry &&
        entry.path === 'page.render.image',
    );
  assert.equal(images.length, 3);
  const closed = await runScenario(
    scenario('plugins/render-tile-plane.json', 'close-with-tiles-in-flight'),
  );
  assert.deepEqual(closed.unmatched, []);
  assert.equal(
    closed.trace
      .flatMap((entry) => entry.engine ?? [])
      .filter(
        (entry) =>
          entry !== null &&
          typeof entry === 'object' &&
          'path' in entry &&
          entry.path === 'page.render.image',
      ).length,
    4,
  );
});

test('signature preparation and cancellation publish mirror facts before the next read', async () => {
  const input = scenario('plugins/signature.json', 'abort-the-parked-signing');
  const pending = {
    op: 'call',
    target: { token: 'signature', doc: 'a' },
    method: 'getPending',
  } as const;
  const steps = input.steps.flatMap((step) =>
    step.op === 'call' && (step.method === 'prepareSignature' || step.method === 'cancelPending')
      ? [step, pending]
      : [step],
  );
  const result = await runScenario({ ...input, steps });
  assert.deepEqual(result.unmatched, []);
  const reads = steps.flatMap((step, index) =>
    step === pending ? [result.trace[index].outcome] : [],
  );
  assert.deepEqual(reads[0], {
    returned: { signingId: 'signing-1', field: { kind: 'objectNumber', objectNumber: 2 } },
  });
  assert.deepEqual(reads.slice(1), [{ returned: null }, { returned: null }]);
  assert.doesNotMatch(
    JSON.stringify(result),
    /is not a function|Cannot read properties|removed upstream/,
  );
});

test('current layout edits preserve their input and scope removals to one bar', () => {
  const before = {
    bars: { main: { place: 'top', start: ['a', 'b'] }, other: { place: 'bottom', start: ['a'] } },
  };
  const frozen = JSON.stringify(before);
  const result = call('core/ui-transforms.json', 'remove', before, ['a'], {
    from: 'main',
  }) as typeof before;
  assert.equal(JSON.stringify(before), frozen);
  assert.deepEqual(result.bars.main.start, ['b']);
  assert.deepEqual(result.bars.other.start, ['a']);
  assert.deepEqual(call('core/ui-schema.json', 'validateLayout', before, ['a', 'b']), []);
  assert.deepEqual(call('core/ui-helpers.json', 'isLayout', before), { raw: false, created: true });
});

test('current shallow state comparison and signature target transitions preserve identity', () => {
  assert.equal(
    call('kernel/utils.json', 'shallowEqual', { status: 'ready' }, { status: 'ready' }),
    true,
  );
  assert.equal(
    call('kernel/utils.json', 'shallowEqual', { status: 'ready' }, { status: 'loading' }),
    false,
  );
  assert.equal(
    call('kernel/utils.json', 'shallowEqual', { error: { code: 'io' } }, { error: { code: 'io' } }),
    false,
  );
  const ref = { kind: 'objectNumber', objectNumber: 2 };
  const target = call('plugins/signature-functions.json', 'setTarget', ref, { ...ref }) as {
    changed: boolean;
  };
  assert.equal(target.changed, false);
});
