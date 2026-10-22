import assert from 'node:assert/strict';
import { test } from 'node:test';
import '../src/suites/interaction';
import { suites as fill } from '../src/suites/form-fill';
import { suites as design } from '../src/suites/form-design';
import { runScenario } from '../src/scenario';

test('all 60 fill/design scenarios use the current form boundary without throws or unmatched calls', async () => {
  const cases = [...fill, ...design].flatMap((s) => s.cases);
  assert.equal(cases.length, 60);
  const broken: unknown[] = [];
  for (const input of cases) {
    assert.ok('steps' in input);
    try {
      const result = await runScenario(input);
      const throws = result.trace.flatMap((t, i) =>
        t.outcome && 'threw' in t.outcome ? [{ step: i, error: t.outcome.threw }] : [],
      );
      if (result.unmatched.length || throws.length)
        broken.push({ name: input.name, unmatched: result.unmatched, throws });
    } catch (error) {
      broken.push({ name: input.name, error: String(error) });
    }
  }
  assert.deepEqual(broken, []);
});

import { FormSnapshotSchema } from '../../../packages/engine/core/src/forms/schema';
import { WidgetDTOSchema } from '../../../packages/engine/core/src/annotation/kinds/widget';
import type { FormSnapshot, WidgetAnnotation } from '@embedpdf/engine-core/runtime';
import { ref, objRef, pr, operationAt, placed } from '../src/suites/form-live-fixtures';
import { canonical } from '../src/canonical';
function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
function object(value: unknown): Record<string, unknown> {
  assert.ok(isObject(value));
  return value;
}
function array(value: unknown): unknown[] {
  assert.ok(Array.isArray(value));
  return value;
}
const all = [...fill, ...design].flatMap((s) => s.cases);
async function run(name: string) {
  const input = all.find((c) => c.name === name);
  assert.ok(input && 'steps' in input);
  return runScenario(input);
}
function returned(trace: Awaited<ReturnType<typeof runScenario>>['trace'], i: number): unknown {
  const outcome = trace[i]?.outcome;
  assert.ok(outcome && 'returned' in outcome, 'returned outcome at ' + i);
  return outcome.returned;
}
const engines = (trace: Awaited<ReturnType<typeof runScenario>>['trace'], path: string) =>
  trace
    .flatMap((t) => t.engine)
    .map(object)
    .filter((e) => e.path === path);
function checkRows(form: FormSnapshot) {
  for (const w of form.widgets) {
    WidgetDTOSchema.parse(w);
    assert.deepEqual(w.box, w.rect, 'unrotated row geometry');
    if (w.field) {
      const f = form.fields.find((f) => JSON.stringify(f.ref) === JSON.stringify(w.field));
      assert.ok(f, 'row field exists');
      assert.equal(w.fieldFamily, f.family, 'row family matches its field');
    }
  }
}
test('complete current snapshots have schema-valid rows whose page geometry and family agree with the field', () => {
  for (const input of all) {
    assert.ok('steps' in input);
    for (const d of Object.values(input.engine?.documents ?? {}))
      for (const response of d.responses ?? [])
        if (response.path === 'forms.list' && response.value) {
          checkRows(FormSnapshotSchema.parse(response.value));
        }
  }
});
test('the actual pointer create confirms the same geometry and refs that were sent, and selects its field', async () => {
  const { trace } = await run('a-click-places-the-tool-default');
  const calls = engines(trace, 'apply');
  assert.equal(calls.length, 1);
  const args = array(calls[0]!.args),
    op = object(array(object(args[0]).ops)[0]);
  assert.equal(op.type, 'forms.create');
  assert.equal(op.objectNumber, 10001);
  assert.deepEqual(op.widgetObjectNumbers, [10002]);
  assert.deepEqual(object(array(object(op.draft).widgets)[0]).rect, {
    x: 20,
    y: 88,
    width: 160,
    height: 24,
  });
  const selected = array(returned(trace, 5));
  assert.equal(selected.length, 1);
  const widget = WidgetDTOSchema.parse(selected[0]);
  assert.deepEqual(widget.ref, objRef(10002));
  assert.deepEqual(widget.rect, { x: 20, y: 88, width: 160, height: 24 });
  assert.deepEqual(widget.box, widget.rect);
  assert.deepEqual(object(returned(trace, 6)).ref, ref(10001));
  assert.equal(returned(trace, 7), 'form-text');
});

const pluginRefusal = (message: string, engineCode: string, code: string) => ({
  $error: {
    name: 'PluginError',
    message,
    code,
    capability: 'form',
    permission: null,
    cause: { $error: { name: 'EngineError', message, code: engineCode } },
  },
});
const denied = (verb: string) => ({
  $error: {
    name: 'PluginError',
    message: `${verb} requires doc.forms.fill`,
    code: 'permission-denied',
    capability: 'form',
    permission: 'doc.forms.fill',
  },
});
const expectedRejections: Record<string, unknown[]> = {
  'a-failed-write-reverts': [pluginRefusal('invalid value', 'InvalidArg', 'invalid-input')],
  'the-write-gate': [
    denied('form.setValue'),
    denied('form.setValue'),
    denied('form.setValue'),
    denied('form.reset'),
  ],
  'commit-value-without-scripting': [pluginRefusal('nope', 'InvalidArg', 'invalid-input')],
  'a-failed-reset-batch-rejects': [
    pluginRefusal('disk full', 'Io', 'operation-failed'),
    pluginRefusal('reset refused', 'Refused', 'operation-failed'),
    pluginRefusal('batch refused', 'InvalidArg', 'invalid-input'),
  ],
  'reset-without-the-batch': [pluginRefusal('no default', 'InvalidArg', 'invalid-input')],
  'set-value-passthrough': [pluginRefusal('nope', 'InvalidArg', 'invalid-input')],
  interchange: [
    pluginRefusal('export refused', 'NotImplemented', 'unsupported'),
    pluginRefusal('garbled bundle', 'InvalidArg', 'invalid-input'),
    pluginRefusal('repair refused', 'Refused', 'operation-failed'),
  ],
  'reset-all-failures': [
    denied('form.reset'),
    pluginRefusal('reset atomic refusal', 'Refused', 'operation-failed'),
  ],
  'placement-refused-by-the-engine': [
    {
      $error: {
        name: 'PluginError',
        message: 'permission denied: doc.forms.modify',
        code: 'permission-denied',
        capability: 'form',
        permission: 'doc.forms.modify',
        details: { required: 'doc.forms.modify' },
        cause: {
          $error: {
            name: 'PermissionDenied',
            message: 'permission denied: doc.forms.modify',
            code: 'Forbidden',
            details: { required: 'doc.forms.modify' },
          },
        },
      },
    },
  ],
  'update-field': [pluginRefusal('family mismatch', 'InvalidArg', 'invalid-input')],
  'delete-field': [pluginRefusal('unknown field', 'NotFound', 'not-found')],
  'delete-field-without-annotation': [pluginRefusal('gone', 'NotFound', 'not-found')],
  'detach-widget': [
    pluginRefusal('not attached', 'InvalidArg', 'invalid-input'),
    pluginRefusal('unknown field', 'NotFound', 'not-found'),
  ],
  'attach-widget': [pluginRefusal('cannot add widget', 'InvalidArg', 'invalid-input')],
};
test('all named refusals and diagnostics use exact rejection, report and callback channels; no tracked promise is left pending', async () => {
  const previousError = console.error,
    previousWarn = console.warn,
    entropy = Object.getOwnPropertyDescriptor(crypto, 'getRandomValues');
  for (const input of all) {
    assert.ok('steps' in input);
    const { trace } = await runScenario(input);
    assert.deepEqual(
      trace.flatMap((t) => t.settled.filter((s) => 'rejected' in s).map((s) => s.rejected)),
      expectedRejections[input.name] ?? [],
      input.name + ' exact rejections',
    );
    assert.deepEqual(
      trace.flatMap((t) => t.reports),
      input.name === 'confirmed-change-events'
        ? [
            { $error: { name: 'Error', message: 'listener broke' } },
            { $error: { name: 'Error', message: 'listener broke' } },
          ]
        : [],
      input.name + ' report channel',
    );
    assert.deepEqual(
      trace.flatMap((t) => t.callbacks.filter((c) => c.fn.startsWith('console.'))),
      input.name === 'a-failed-placement-is-logged'
        ? [
            {
              fn: 'console.error',
              args: [
                '[form] placing the field failed:',
                pluginRefusal('intended placement refusal', 'Refused', 'operation-failed'),
              ],
            },
          ]
        : [],
      input.name + ' console channel',
    );
    const settled = new Set(trace.flatMap((t) => t.settled.map((s) => s.promise)));
    for (const t of trace)
      if (t.outcome && 'promise' in t.outcome)
        assert.ok(settled.has(t.outcome.promise), input.name + ' unresolved promise');
    assert.equal(console.error, previousError, input.name + ' console.error restoration');
    assert.equal(console.warn, previousWarn);
    assert.deepEqual(Object.getOwnPropertyDescriptor(crypto, 'getRandomValues'), entropy);
  }
});
test('hydration, forbidden/error recovery and shared widget loading expose the actual mirror lifecycle', async () => {
  const hydration = await run('hydrates-on-open');
  assert.equal(returned(hydration.trace, 2), null);
  assert.deepEqual(returned(hydration.trace, 3), []);
  const form = FormSnapshotSchema.parse(returned(hydration.trace, 9));
  assert.equal(form.fields.length, 4);
  assert.equal(form.widgets.length, 4);
  assert.deepEqual(object(returned(hydration.trace, 12)).ref, ref(2));
  assert.equal(returned(hydration.trace, 13), null);
  const forbidden = await run('no-read-grant-never-lists');
  assert.deepEqual(engines(forbidden.trace, 'forms.list'), []);
  assert.equal(returned(forbidden.trace, 3), null);
  assert.deepEqual(
    [4, 5, 6].map((i) => returned(forbidden.trace, i)),
    [false, false, true],
  );
  const failed = await run('a-failed-load-warns');
  assert.equal(returned(failed.trace, 2), 'error');
  assert.equal(returned(failed.trace, 3), null);
  assert.equal(returned(failed.trace, 5), 'ready');
  assert.equal(engines(failed.trace, 'forms.list').length, 2);
  const coalesced = await run('refresh-coalesces-and-forces');
  assert.equal(engines(coalesced.trace, 'forms.list').length, 3);
  assert.equal(FormSnapshotSchema.parse(returned(coalesced.trace, 7)).fields[0]?.name, 'name');
  assert.equal(
    object(FormSnapshotSchema.parse(returned(coalesced.trace, 7)).fields[0]).value,
    'Grace',
  );
  const geometry = await run('widget-geometry-loads-once-per-page');
  assert.equal(engines(geometry.trace, 'forms.list').length, 1);
  assert.deepEqual(
    geometry.trace
      .flatMap((t) => t.engine)
      .map(object)
      .filter((e) => String(e.path).startsWith('annotations.')),
    [],
  );
  const far = object(array(returned(geometry.trace, 11))[0]);
  assert.deepEqual(far.box, { x: 30, y: 100, width: 100, height: 20 });
  assert.ok(hydration.trace.every((t) => t.stateChanges.length === 0));
});
test('authority fuses with readOnly while per-field twins and document design grants remain independent', async () => {
  const { trace } = await run('fill-authority-fuses-into-disabled');
  assert.deepEqual(
    array(returned(trace, 3)).map((x) => object(x).disabled),
    [true, true],
  );
  assert.deepEqual(
    array(returned(trace, 6)).map((x) => object(x).disabled),
    [false, true],
  );
  assert.equal(returned(trace, 5), false);
  assert.equal(returned(trace, 7), true);
  const twins = await run('the-twins');
  assert.deepEqual(
    [3, 4, 5, 6, 7, 8].map((i) => returned(twins.trace, i)),
    [true, true, false, false, false, true],
  );
  const deniedTrace = await run('the-write-gate');
  assert.deepEqual(engines(deniedTrace.trace, 'apply'), []);
  assert.deepEqual(returned(deniedTrace.trace, 6), { value: 'Ada' });
});
test('family writes send exact current ops and opIds, confirm public values, and overlap pending answers in order', async () => {
  const { trace } = await run('typed-writes');
  const calls = engines(trace, 'apply');
  assert.deepEqual(
    calls.map((e) => e.args),
    [
      [
        { ops: [{ type: 'forms.setValue', field: ref(1), value: { value: 'Grace' } }] },
        { opId: operationAt(1) },
      ],
      [
        { ops: [{ type: 'forms.setValue', field: ref(2), value: { checked: true } }] },
        { opId: operationAt(2) },
      ],
      [
        { ops: [{ type: 'forms.setValue', field: ref(2), value: { value: null } }] },
        { opId: operationAt(3) },
      ],
      [
        { ops: [{ type: 'forms.setValue', field: ref(3), value: { value: 'g' } }] },
        { opId: operationAt(4) },
      ],
    ],
  );
  assert.deepEqual(
    [6, 7, 8].map((i) => returned(trace, i)),
    [{ value: 'Grace' }, { checked: false }, { value: 'g' }],
  );
  assert.deepEqual(
    trace
      .slice(2, 6)
      .flatMap((t) => t.settled)
      .map((s) => object(s.resolved).status),
    ['applied', 'applied', 'applied', 'applied'],
  );
  assert.equal(engines(trace, 'forms.list').length, 1);
  const overlap = await run('writes-serialize');
  assert.equal(engines(overlap.trace.slice(0, 4), 'apply').length, 2);
  assert.deepEqual(returned(overlap.trace, 4), { value: 'b' });
  assert.deepEqual(returned(overlap.trace, 7), { value: 'b' });
  assert.equal(overlap.trace[5]?.settled[0]?.promise, 2);
  assert.equal(overlap.trace[6]?.settled[0]?.promise, 3);
  const refusal = await run('a-failed-write-reverts');
  assert.deepEqual(returned(refusal.trace, 3), { value: 'Ada' });
  assert.deepEqual(returned(refusal.trace, 5), { value: 'good' });
  const statuses = await run('commit-value-without-scripting');
  assert.deepEqual(
    statuses.trace
      .slice(2, 4)
      .flatMap((t) => t.settled)
      .map((s) => object(s.resolved).status),
    ['applied', 'unchanged'],
  );
});
test('reset sends one atomic current op, restores defaults and selects refs with ISO exclusion semantics', async () => {
  const { trace } = await run('reset-rides-the-batch');
  assert.deepEqual(
    engines(trace, 'apply').map((e) => e.args),
    [[{ ops: [{ type: 'forms.reset', fields: [ref(1)] }] }, { opId: operationAt(1) }]],
  );
  assert.equal(returned(trace, 3), null);
  assert.equal(object(array(object(trace[2]?.settled[0]?.resolved).fields)[0]).value, '');
  assert.deepEqual(trace[4]?.settled[0]?.resolved, { fields: [] });
  assert.equal(engines(trace, 'forms.list').length, 1);
  const allReset = await run('reset-all');
  const ops = engines(allReset.trace, 'apply').map((e) =>
    object(array(object(array(e.args)[0]).ops)[0]),
  );
  assert.deepEqual(
    ops.map((o) => o.fields),
    [
      [ref(1), ref(2), ref(3), ref(4)],
      [ref(1), ref(2)],
      [ref(2), ref(3), ref(4)],
    ],
  );
  assert.equal(returned(allReset.trace, 6), null);
  assert.deepEqual(allReset.trace[5]?.settled[0]?.resolved, { fields: [] });
  const failed = await run('reset-all-failures');
  assert.deepEqual(returned(failed.trace, 6), { value: 'Ada' });
  assert.equal(returned(failed.trace, 8), null);
  assert.equal(engines(failed.trace, 'apply').length, 2);
});
test('batches distinguish accepted, failed and skipped entries and preserve integer-name operation order', async () => {
  const { trace } = await run('batch-writes');
  assert.deepEqual(trace[3]?.settled[0]?.resolved, {
    applied: [ref(1)],
    skipped: [],
    failed: [
      {
        ref: ref(404),
        error: {
          code: 'not-found',
          message: 'no form field has this ref',
          capability: 'form',
          permission: null,
        },
      },
    ],
  });
  assert.deepEqual(trace[4]?.settled[0]?.resolved, {
    applied: [ref(1), ref(3)],
    skipped: [
      { ref: 'absent', reason: 'the form has no field with this name' },
      { ref: 'agree', reason: "a checkbox field can't take this value" },
    ],
    failed: [],
  });
  assert.deepEqual(trace[6]?.settled[0]?.resolved, {
    applied: [],
    skipped: [],
    failed: [
      {
        ref: ref(1),
        error: {
          code: 'permission-denied',
          message: 'form.setValues requires doc.forms.fill',
          capability: 'form',
          permission: 'doc.forms.fill',
        },
      },
    ],
  });
  assert.deepEqual(trace[7]?.settled[0]?.resolved, {
    applied: [],
    skipped: [{ ref: 'name', reason: 'you may not fill in this field' }],
    failed: [],
  });
  assert.deepEqual(returned(trace, 8), { name: 'Ada', agree: false, colour: 'g', colours: ['g'] });
  const integer = await run('values-keyed-by-integer-like-names');
  assert.deepEqual(Object.keys(object(returned(integer.trace, 2))), ['2', '10', 'b', 'a', '02']);
  const ops = array(object(array(engines(integer.trace, 'apply')[0]!.args)[0]).ops);
  assert.deepEqual(
    ops.map((op) => object(op).field),
    [ref(3), ref(2), ref(1), ref(4), ref(5)],
  );
  assert.deepEqual(returned(integer.trace, 4), {
    2: 'Two',
    10: 'Ten',
    b: 'B',
    a: 'A',
    '02': 'Zero-two',
  });
});
test('confirmed facts retain exact origins, continue after listener failures, and resync only on loads', async () => {
  const { trace } = await run('confirmed-change-events');
  const callbacks = trace.flatMap((t) => t.callbacks);
  assert.deepEqual(
    callbacks.map((c) => c.fn),
    [
      'valueChanged.bad',
      'valueChanged',
      'valueChanged.bad',
      'valueChanged',
      'fieldCreated',
      'fieldUpdated',
      'fieldUpdated',
      'fieldUpdated',
      'fieldDeleted',
      'resynced',
    ],
  );
  assert.deepEqual(object(callbacks[1]!.args[0]).origin, {
    kind: 'remote',
    sessionId: 's2',
    sub: 'u-9',
    ts: 0,
    serverId: null,
  });
  assert.deepEqual(object(callbacks[3]!.args[0]).origin, {
    kind: 'local',
    sessionId: 's1',
    sub: null,
    ts: 0,
    serverId: null,
  });
  assert.deepEqual(callbacks[8]!.args, [
    {
      ref: ref(3),
      origin: {
        kind: 'local',
        sessionId: 's1',
        sub: null,
        ts: 0,
        serverId: null,
        tx: { id: operationAt(1), index: 0, count: 1 },
      },
    },
  ]);
  const unsub = await run('an-unsubscribed-listener-hears-nothing');
  assert.deepEqual(
    unsub.trace.flatMap((t) => t.callbacks),
    [],
  );
  const foreign = await run('the-effect-folds-foreign-events');
  assert.deepEqual(
    [4, 6].map((i) => returned(foreign.trace, i)),
    [{ value: 'Grace' }, { value: 'Local' }],
  );
  assert.equal(returned(foreign.trace, 8), null);
  assert.equal(returned(foreign.trace, 9), null);
  assert.equal(engines(foreign.trace, 'forms.list').length, 2);
  const inflight = await run('the-effect-refresh-is-latched');
  assert.deepEqual(
    [4, 6, 8, 10].map((i) => returned(inflight.trace, i)),
    [{ value: 'Ada' }, { value: 'Ada' }, { value: 'Second' }, { value: 'After' }],
  );
});
test('design create/update/delete use final refs, fold state and lend widgets without reloading annotation pages', async () => {
  const created = await run('place-a-text-field');
  const creates = engines(created.trace, 'apply');
  assert.deepEqual(
    creates.map((e) => object(array(object(array(e.args)[0]).ops)[0]).objectNumber),
    [10001, 10003],
  );
  assert.deepEqual(object(returned(created.trace, 3)).ref, ref(10001));
  assert.deepEqual(object(array(returned(created.trace, 5))[0]).page, pr(2));
  const update = await run('update-field');
  assert.equal(object(returned(update.trace, 3)).name, 'renamed');
  assert.deepEqual(array(object(array(engines(update.trace, 'apply')[0]!.args)[0]).ops), [
    { type: 'forms.update', field: ref(1), patch: { name: 'renamed' } },
  ]);
  const lending = await run('placement-reloads-the-annotation-page');
  assert.deepEqual(object(array(returned(lending.trace, 6))[0]).ref, objRef(10002));
  assert.deepEqual(object(returned(lending.trace, 7)).ref, ref(10001));
  assert.deepEqual(
    lending.trace
      .flatMap((t) => t.engine)
      .map(object)
      .filter((e) => String(e.path).startsWith('annotations.'))
      .map((e) => e.path),
    ['annotations.list'],
  );
  const deleted = await run('delete-field');
  assert.equal(returned(deleted.trace, 5), null);
  assert.deepEqual(returned(deleted.trace, 6), []);
  assert.deepEqual(returned(deleted.trace, 7), []);
  assert.deepEqual(returned(deleted.trace, 8), []);
  const detached = await run('detach-widget');
  assert.equal(returned(detached.trace, 5), null);
  assert.equal(object(returned(detached.trace, 6)).field, null);
  assert.deepEqual(array(object(array(engines(detached.trace, 'apply')[0]!.args)[0]).ops), [
    { type: 'forms.removeWidget', field: ref(7), widget: objRef(107) },
  ]);
  const added = await run('attach-widget');
  assert.deepEqual(
    array(returned(added.trace, 4)).map((w) => object(w).ref),
    [objRef(107), objRef(10001)],
  );
  const addOp = object(array(object(array(engines(added.trace, 'apply')[0]!.args)[0]).ops)[0]);
  assert.equal(addOp.type, 'forms.addWidget');
  assert.equal(addOp.objectNumber, 10001);
  assert.deepEqual(addOp.field, ref(7));
});
test('model hit tests choose minimum area and stable ties; actual placement previews clear and promote to lent widgets', async () => {
  const hits = await run('widget-hit-test-live-boxes-compare-areas');
  assert.deepEqual(
    [2, 3, 4].map((i) => object(returned(hits.trace, i)).annotObjectNumber),
    [201, 203, 205],
  );
  const absent = await run('widget-hit-test-without-annotation');
  assert.equal(returned(absent.trace, 2), null);
  assert.equal(object(returned(absent.trace, 4)).annotObjectNumber, 102);
  const preview = await run('the-live-preview');
  assert.deepEqual(returned(preview.trace, 5), []);
  const draft = object(array(returned(preview.trace, 7))[0]);
  assert.equal(draft.source, 'draft');
  assert.deepEqual(draft.box, { x: 10, y: 10, width: 50, height: 30 });
  assert.deepEqual(returned(preview.trace, 9), []);
  assert.ok(array(returned(preview.trace, 12)).every((item) => object(item).source !== 'draft'));
});
test('real placement clamps page edges, retains thin drags and projection, declines ineligible tools, and logs exact refused rollback', async () => {
  const rects = async (name: string) =>
    engines((await run(name)).trace, 'apply').map((e) => {
      const request = object(array(e.args)[0]);
      const op = object(array(request.ops)[0]);
      const draft = object(op.draft);
      return object(array(draft.widgets)[0]).rect;
    });
  assert.deepEqual(await rects('placement-is-page-bound'), [
    { x: 0, y: 790, width: 600, height: 10 },
    { x: 599, y: 799, width: 1, height: 1 },
  ]);
  assert.deepEqual(await rects('a-thin-drag-is-a-drag'), [{ x: 50, y: 50, width: 100, height: 2 }]);
  assert.deepEqual(await rects('the-home-page-tracks-through-the-projection'), [
    { x: 10, y: 10, width: 100, height: 50 },
    { x: 10, y: 10, width: 50, height: 30 },
  ]);
  assert.deepEqual(await rects('a-click-near-the-edge-is-clamped'), [
    { x: 460, y: 0, width: 140, height: 72 },
  ]);
  for (const name of ['the-click-threshold-is-strict', 'the-handler-declines'])
    assert.deepEqual(engines((await run(name)).trace, 'apply'), []);
  const priorities = await run('the-handler-outranks-lower-priorities');
  assert.ok(priorities.trace.flatMap((t) => t.callbacks).some((c) => c.fn === 'gestureStarted'));
  assert.ok(
    priorities.trace
      .flatMap((t) => t.callbacks)
      .every((c) => c.fn !== 'low.onDown' && c.fn !== 'low.onUp'),
  );
  const refusal = await run('a-failed-placement-is-logged');
  assert.equal(engines(refusal.trace, 'apply').length, 1);
  assert.deepEqual(returned(refusal.trace, 9), []);
  assert.equal(
    refusal.trace.flatMap((t) => t.callbacks).filter((c) => c.fn === 'console.error').length,
    1,
  );
  const stable = await run('auto-select-waits-for-a-stable-world');
  assert.equal(engines(stable.trace, 'apply').length, 3);
  assert.deepEqual(
    [7, 11, 14].map((i) => returned(stable.trace, i)),
    [[], [], []],
  );
});
test('close releases old event listeners and reopening restores the original subscriber count; traces repeat with exact host restoration', async () => {
  const input = all.find((c) => c.name === 'close-unsubscribes');
  assert.ok(input && 'steps' in input);
  const first = await runScenario(input),
    second = await runScenario(input);
  assert.deepEqual(second, first);
  assert.equal(
    object(first.trace[3]?.engineState).a &&
      object(object(first.trace[3]?.engineState).a).subscribers,
    0,
  );
  assert.equal(
    object(object(first.trace[4]?.engineState).a).subscribers,
    object(object(first.trace[1]?.engineState).a).subscribers,
  );
  assert.deepEqual(
    first.trace.flatMap((t) => t.callbacks),
    [],
  );
  assert.deepEqual(returned(first.trace, 6), { value: 'Grace' });
});

import { registerPlugin } from '../src/scenario';
import {
  scenario,
  text,
  checkbox,
  signature,
  snapshot,
  row,
  call,
} from '../src/suites/form-live-fixtures';
registerPlugin('form-live-intentional-factory-error', {
  factory: () => {
    throw new Error('intended live construction failure');
  },
});
test('the fixed diagnostics-before-annotation factory order restores console and shared entropy after a construction failure', async () => {
  const input = all.find((c) => c.name === 'a-failed-placement-is-logged');
  assert.ok(input && 'steps' in input);
  const previousError = console.error,
    previousWarn = console.warn,
    entropy = Object.getOwnPropertyDescriptor(crypto, 'getRandomValues');
  await assert.rejects(
    runScenario({
      ...input,
      name: 'form-live-construction-error',
      plugins: [...(input.plugins ?? []), { factory: 'form-live-intentional-factory-error' }],
    }),
    { message: 'intended live construction failure' },
  );
  assert.equal(console.error, previousError);
  assert.equal(console.warn, previousWarn);
  assert.deepEqual(Object.getOwnPropertyDescriptor(crypto, 'getRandomValues'), entropy);
});
test('current required validation orders missing fields by page and position, and ignores fields denied to the session', async () => {
  const fields = [
    text(1, 'later', '', { required: true }, [placed(101, 2)]),
    text(2, 'first', '', { required: true }),
    checkbox(3, 'agree', false, { required: true }),
    signature(4, 'sign', { required: true, widgets: [placed(104, 2)] }),
  ];
  const widgets = [
    row(101, ref(1), 2, { x: 20, y: 20, width: 100, height: 24 }),
    row(102, ref(2), 1, { x: 20, y: 20, width: 100, height: 24 }),
    row(103, ref(3), 1, { x: 5, y: 10, width: 18, height: 18 }, { fieldFamily: 'checkbox' }),
    row(104, ref(4), 2, { x: 20, y: 50, width: 100, height: 24 }, { fieldFamily: 'signature' }),
  ];
  const input = scenario(
    'required-validation-host-check',
    'Current required fields, independently ordered and gated.',
    [call('validate')],
    snapshot(fields, widgets),
  );
  const result = await runScenario(input);
  const validation = object(returned(result.trace, 2));
  assert.equal(validation.valid, false);
  assert.deepEqual(
    array(validation.missing).map((f) => object(f).ref),
    [ref(3), ref(2), ref(1), ref(4)],
  );
  const denied = scenario(
    'required-validation-denied-host-check',
    'A session with no field fill authority has no missing required fields.',
    [call('validate')],
    snapshot(fields, widgets),
    [],
    { security: { allows: ['doc.forms.read'], allowsField: { fill: false } } },
  );
  const deniedResult = await runScenario(denied);
  assert.deepEqual(returned(deniedResult.trace, 2), { valid: true, missing: [] });
  assert.deepEqual(engines(deniedResult.trace, 'apply'), []);
});

import {
  assertFormBundle,
  assertFormBundleManifest,
} from '../../../packages/engine/core/src/transfer/FormBundle';
import { DEFAULT_BUNDLE_LIMITS } from '../../../packages/engine/core/src/transfer/bundleLimits';
test('bundle interchange carries complete current fields and widget rows, maps imported refs, and folds confirmed creation', async () => {
  const input = all.find((c) => c.name === 'interchange');
  assert.ok(input && 'steps' in input);
  for (const document of Object.values(input.engine?.documents ?? {}))
    for (const response of document.responses ?? [])
      if (response.path === 'forms.export' && response.value) {
        assertFormBundleManifest(response.value, new Map(), DEFAULT_BUNDLE_LIMITS);
        assert.deepEqual(object(response.value).resources, {});
        await assertFormBundle({ ...response.value, resources: {} });
      }
  const { trace } = await runScenario(input);
  const exported = object(trace[2]?.settled[0]?.resolved);
  assert.equal(exported.format, 'embedpdf/form');
  assert.equal(array(exported.widgets).length, 4);
  assert.deepEqual(trace[4]?.settled[0]?.resolved, {
    fields: [object(returned(trace, 5))],
    refMap: [{ from: ref(40), to: ref(50) }],
    dropped: [],
  });
  assert.equal(object(returned(trace, 5)).name, 'imported');
  assert.equal(object(returned(trace, 5)).value, 'Imported');
  const engineImports = engines(trace, 'forms.import');
  assert.equal(engineImports.length, 2);
  const imported = object(object(array(object(array(engineImports[0]!.args)[0]).fields)[0]).data);
  assert.deepEqual(imported.ref, ref(40));
  assert.equal(imported.name, 'imported');
  assert.equal(imported.value, 'Imported');
  assert.deepEqual(imported.widgets, []);
  assert.deepEqual(array(engineImports[0]!.args)[1], { attribution: 'stamp' });
});

import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
test('all original 60 inputs and exact source bytes remain archived and every active replacement digest is accounted for', async () => {
  const root = new URL('../', import.meta.url),
    history = new URL('historical/task-12b2b2-ii/', root),
    oldHistory = new URL('historical/task-12b2b2-i/', root);
  const before = array(
    JSON.parse(await readFile(new URL('original-inputs.json', history), 'utf8')),
  ).map(object);
  const original220 = array(
    JSON.parse(await readFile(new URL('original-inputs.json', oldHistory), 'utf8')),
  ).map(object);
  const baseline = object(JSON.parse(await readFile(new URL('baseline.json', history), 'utf8')));
  const accounting = object(
    JSON.parse(await readFile(new URL('accounting.json', history), 'utf8')),
  );
  const hash = (value: string | Uint8Array) => createHash('sha256').update(value).digest('hex');
  assert.equal(before.length, 60);
  assert.equal(new Set(before.map((r) => r.file + ':' + r.name)).size, 60);
  assert.deepEqual(
    [accounting.originalOwned, accounting.currentOwned, accounting.mapped, accounting.renamed],
    [60, 60, 60, 0],
  );
  assert.deepEqual(accounting.retired, []);
  assert.deepEqual(accounting.added, []);
  const mappings = array(accounting.mappings).map(object);
  assert.equal(mappings.length, 60);
  assert.equal(
    new Set(mappings.map((m) => object(m.before).file + ':' + object(m.before).name)).size,
    60,
  );
  for (const mapping of mappings) {
    const from = object(mapping.before),
      to = object(mapping.after);
    const old = before.find((r) => r.file === from.file && r.name === from.name);
    assert.ok(old);
    assert.deepEqual(
      old,
      original220.find((r) => r.file === from.file && r.name === from.name),
    );
    const current = [...fill, ...design]
      .flatMap((s) => s.cases.map((input) => ({ file: s.file, name: input.name, input })))
      .find((r) => r.file === to.file && r.name === to.name);
    assert.ok(current);
    assert.equal(hash(JSON.stringify(old.input)), from.canonicalInputSha256);
    assert.equal(hash(JSON.stringify(canonical(current.input))), to.canonicalInputSha256);
    assert.equal(mapping.task11InputDrift, false);
    assert.ok(array(mapping.sourceEvidence).length > 0);
  }
  for (const item of array(baseline.sources).map(object)) {
    const name = String(item.path).split('/').at(-1)!;
    assert.equal(hash(await readFile(new URL(name + '.txt', history))), item.sha256);
    assert.deepEqual(
      await readFile(new URL(name + '.txt', history)),
      await readFile(new URL(name + '.txt', oldHistory)),
    );
  }
  const oldBaseline = object(
    JSON.parse(await readFile(new URL('baseline.json', oldHistory), 'utf8')),
  );
  for (const item of array(oldBaseline.sources).map(object)) {
    const name = String(item.path).split('/').at(-1)!;
    if (name === 'form-fixtures.ts')
      assert.equal(
        hash(await readFile(new URL('src/suites/' + name, root))),
        item.sha256,
        name + ' deferred bytes',
      );
  }
});

test('snapshot reads retain raw scalar/array distinctions, page filtering, unplaced widget refs and crop-relative fractional geometry', async () => {
  const input = all.find((c) => c.name === 'reads-over-the-snapshot');
  assert.ok(input && 'steps' in input);
  const { trace } = await runScenario(input);
  const values: unknown[] = [];
  for (const [i, step] of input.steps.entries())
    if (step.op === 'call' && step.method === 'getValue') values.push(returned(trace, i));
  assert.deepEqual(values, [
    { value: 'Ada' },
    { checked: false },
    { value: 'r' },
    { selectedValues: ['g'] },
    { value: 'l1\nl2' },
    { checked: false },
    { checked: false },
    { value: 'r' },
    { selectedValues: ['r'] },
    null,
    null,
    { checked: true },
    null,
    { value: 'l' },
  ]);
  const pageFilter = input.steps.findIndex(
    (step) =>
      step.op === 'call' &&
      step.method === 'list' &&
      isObject(step.args?.[0]) &&
      JSON.stringify(step.args[0]) === JSON.stringify({ page: pr(2) }),
  );
  assert.ok(pageFilter >= 0);
  assert.deepEqual(
    array(returned(trace, pageFilter)).map((f) => object(f).ref),
    [ref(8)],
  );
  const crop = await run('geometry-on-an-offset-crop');
  assert.deepEqual(object(array(returned(crop.trace, 3))[0]).box, {
    x: 30,
    y: 99.5,
    width: 100,
    height: 20.5,
  });
});

test('whole-form reset refusals preserve all selected values and success confirms every changed field, row and fact', async () => {
  const input = all.find((c) => c.name === 'reset-all-failures');
  assert.ok(input && 'steps' in input);
  const probe = {
    ...input,
    steps: input.steps.flatMap((step, index) =>
      index === 4
        ? [call('onResynced', [{ $fn: 'initialResetSnapshot' }]), step]
        : step.op === 'call' && step.method === 'getValue'
          ? [step, ...[2, 3, 4].map((n) => call('getValue', [ref(n)])), call('getSnapshot')]
          : [step],
    ),
  };
  const { trace } = await runScenario(probe);
  const reads: unknown[] = [];
  const snapshots: FormSnapshot[] = [];
  for (const [i, step] of probe.steps.entries())
    if (step.op === 'call') {
      if (step.method === 'getValue') reads.push(returned(trace, i));
      if (step.method === 'getSnapshot')
        snapshots.push(FormSnapshotSchema.parse(returned(trace, i)));
    }
  assert.deepEqual(reads, [
    { value: 'Ada' },
    { checked: false },
    { value: 'r' },
    { selectedValues: ['g'] },
    null,
    { checked: false },
    null,
    null,
  ]);
  const initial = trace.flatMap((t) => t.callbacks).find((c) => c.fn === 'initialResetSnapshot');
  assert.ok(initial);
  const initialForm = FormSnapshotSchema.parse(object(initial.args[0]).snapshot);
  assert.deepEqual(
    initialForm.fields.map((f) => f.valueEntry),
    [
      { kind: 'scalar', value: 'Ada' },
      { kind: 'scalar', value: 'Off' },
      { kind: 'scalar', value: 'r' },
      { kind: 'array', values: ['g'] },
    ],
  );
  assert.deepEqual(
    snapshots.map((s) => s.fields.map((f) => f.valueEntry)),
    [
      [
        { kind: 'scalar', value: 'Ada' },
        { kind: 'scalar', value: 'Off' },
        { kind: 'scalar', value: 'r' },
        { kind: 'array', values: ['g'] },
      ],
      [{ kind: 'none' }, { kind: 'scalar', value: 'Off' }, { kind: 'none' }, { kind: 'none' }],
    ],
  );
  const changed = trace
    .flatMap((t) => t.settled)
    .filter((s) => s.resolved && isObject(s.resolved) && 'fields' in s.resolved)
    .map((s) => object(s.resolved));
  assert.equal(changed.length, 1);
  assert.deepEqual(
    array(changed[0]!.fields).map((f) => object(f).ref),
    [ref(1), ref(3), ref(4)],
  );
  const colour = snapshots[1]!.fields.find(
    (field) => field.ref.kind === 'objectNumber' && field.ref.objectNumber === 3,
  );
  assert.ok(colour?.family === 'combobox');
  assert.equal(colour.value, '');
  assert.ok(colour.options.every((option) => !option.selected));
  const colours = snapshots[1]!.fields.find(
    (field) => field.ref.kind === 'objectNumber' && field.ref.objectNumber === 4,
  );
  assert.ok(colours?.family === 'listbox');
  assert.deepEqual(colours.selectedValues, []);
  assert.ok(colours.options.every((option) => !option.selected));
  assert.deepEqual(
    changed[0]!.fields,
    snapshots[1]!.fields.filter(
      (field) => field.ref.kind === 'objectNumber' && [1, 3, 4].includes(field.ref.objectNumber),
    ),
  );
  assert.deepEqual(
    engines(trace, 'apply').map((e) => e.args),
    [1, 2].map((n) => [
      { ops: [{ type: 'forms.reset', fields: [ref(1), ref(2), ref(3), ref(4)] }] },
      { opId: operationAt(n) },
    ]),
  );
  for (const name of ['reset-all-failures', 'reset-all', 'a-reset-action-without-a-batch-door']) {
    const target = all.find((c) => c.name === name);
    assert.ok(target && 'steps' in target);
    const answers = Object.values(target.engine?.documents ?? {})
      .flatMap((d) => d.responses ?? [])
      .filter((r) => r.path === 'apply' && r.value);
    const answer = object(answers[0]!.value),
      item = object(array(answer.items)[0]);
    assert.deepEqual(
      array(item.fields).map((f) => object(f).ref),
      [ref(1), ref(3), ref(4)],
      name + ' changed fields',
    );
    assert.deepEqual(
      array(item.widgets).map((w) => object(w).ref),
      [objRef(101), objRef(103), objRef(104)],
      name + ' changed rows',
    );
    const n = name === 'reset-all-failures' ? 2 : 1;
    const meta = {
      affectedPages: [pr(1)],
      cacheDelta: null,
      opId: operationAt(n),
      undoable: true,
      changedFields: [ref(1), ref(3), ref(4)],
      changedWidgets: [placed(101), placed(103), placed(104)],
      redrawnWidgets: [placed(101), placed(103), placed(104)],
    };
    assert.deepEqual(item.meta, meta, name + ' reset item meta');
    assert.deepEqual(answer.meta, meta, name + ' reset answer meta');
    const facts = answers[0]!.events ?? [];
    assert.equal(facts.length, 3, name + ' changed facts');
    for (const [index, event] of facts.entries()) {
      const fact = object(event),
        number = [1, 3, 4][index]!,
        widgetNumber = number + 100;
      assert.equal(fact.type, 'forms.valueSet');
      assert.deepEqual(object(fact.field).ref, ref(number));
      assert.deepEqual(
        array(fact.widgets).map((w) => object(w).ref),
        [objRef(widgetNumber)],
      );
      assert.deepEqual(fact.meta, {
        ...meta,
        changedFields: [ref(number)],
        changedWidgets: [placed(widgetNumber)],
        redrawnWidgets: [placed(widgetNumber)],
      });
      assert.deepEqual(object(fact.origin).tx, { id: operationAt(n), index, count: 3 });
    }
  }
});
test('ordinary create answers preserve every explicitly supplied placement style, including palette defaults', async () => {
  let checked = 0;
  for (const input of design.flatMap((s) => s.cases)) {
    assert.ok('steps' in input);
    const responses = Object.values(input.engine?.documents ?? {})
      .flatMap((d) => d.responses ?? [])
      .filter((r) => r.path === 'apply');
    const reads = responses.flatMap((response) =>
      response.value
        ? array(object(response.value).items)
            .filter((item) => object(item).type === 'forms.create')
            .flatMap((item) => array(object(item).widgets).map((row) => WidgetDTOSchema.parse(row)))
        : [],
    );
    const probe = {
      ...input,
      steps: [...input.steps, ...reads.map((row) => call('getWidget', [row.ref]))],
    };
    const { trace } = await runScenario(probe);
    for (const [index, entry] of engines(trace, 'apply').entries()) {
      const response = responses[index];
      if (!response?.value) continue;
      const op = object(array(object(array(entry.args)[0]).ops)[0]);
      if (op.type !== 'forms.create') continue;
      const widgets = array(object(op.draft).widgets),
        rows = array(object(array(object(response.value).items)[0]).widgets);
      for (const [at, placementValue] of widgets.entries()) {
        const placement = object(placementValue),
          widgetNumber = array(op.widgetObjectNumbers)[at],
          matched = rows.find((value) => object(object(value).ref).objectNumber === widgetNumber);
        if (input.name === 'auto-select-waits-for-a-stable-world' && index === 2 && !matched)
          continue;
        assert.ok(matched, input.name + ' created widget row');
        const result = WidgetDTOSchema.parse(matched);
        const readIndex = reads.findIndex(
          (row) => row.ref.kind === 'objectNumber' && row.ref.objectNumber === widgetNumber,
        );
        assert.ok(readIndex >= 0);
        const confirmed = WidgetDTOSchema.parse(returned(trace, input.steps.length + readIndex));
        for (const key of [
          'interiorColor',
          'color',
          'strokeWidth',
          'borderStyle',
          'fontSize',
          'textAlign',
          'caption',
        ])
          if (key in placement) {
            assert.deepEqual(object(result)[key], placement[key], input.name + ' supplied ' + key);
            assert.deepEqual(
              object(confirmed)[key],
              placement[key],
              input.name + ' confirmed ' + key,
            );
            checked++;
          }
      }
    }
  }
  assert.ok(checked >= 35, 'ordinary create style assertions exercised');
  const { trace } = await run('place-a-text-field');
  const op = object(array(object(array(engines(trace, 'apply')[1]!.args)[0]).ops)[0]);
  assert.equal(object(array(object(op.draft).widgets)[0]).interiorColor, '#ffffff');
  assert.equal(WidgetDTOSchema.parse(array(returned(trace, 5))[0]).interiorColor, '#ffffff');
});
