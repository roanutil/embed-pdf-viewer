import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createCapabilityToken, definePlugin } from '@embedpdf/core';
import { registerPlugin, runScenario, type TraceEntry, type ScenarioCase } from '../src/scenario';
import {
  call,
  scenario,
  square,
  objRef,
  hydrate,
  write,
  created,
  materialize,
  confirmation,
  pr,
  doc,
  updated,
  deletedFact,
  LOCAL,
  PLUGINS,
  boot,
  open,
  refused,
} from '../src/suites/annotation-live-fixtures';
const Nested = createCapabilityToken<unknown>('annotation-test/nested');
registerPlugin('annotation-test/nested', {
  factory: (_, env) =>
    definePlugin({
      id: 'annotation-test/nested',
      token: Nested,
      create: () => {
        let sequence = 0;
        const api = {
          flat() {
            return this === api;
          },
          get noun() {
            env.log('getter', []);
            const n = ++sequence;
            return {
              n,
              read() {
                return this.n;
              },
              get deeper() {
                env.log('deeper', []);
                return {
                  value: 'receiver',
                  read() {
                    return this.value;
                  },
                };
              },
              get callable() {
                env.log('callable', []);
                return function (this: { n: number }) {
                  return this.n;
                };
              },
            };
          },
          get bad() {
            throw new Error('getter refused');
          },
          remover() {
            return function () {
              return 'removed';
            };
          },
        };
        return { api };
      },
    }),
  tokens: [Nested],
});
const nested = (method: string, args: unknown[] = []) => ({
  op: 'call' as const,
  target: { token: Nested.name },
  method,
  args,
});
const errors = (trace: TraceEntry[]) =>
  trace.flatMap((t, i) => [
    ...(t.outcome && 'threw' in t.outcome ? [{ i, error: t.outcome.threw }] : []),
    ...t.settled.filter((s) => s.rejected).map((s) => ({ i, error: s.rejected })),
  ]);
test('dotted paths read each getter once and preserve the final receiver, flat calls and refs', async () => {
  const r = await runScenario({
    name: 'nested-receivers',
    covers: 'Current method traversal',
    plugins: [{ factory: 'annotation-test/nested' }],
    snapshot: 'none',
    reason: 'Call receiver/getter contract',
    steps: [
      nested('flat'),
      nested('noun.read'),
      nested('noun.deeper.read'),
      nested('noun.callable'),
      nested('remover'),
      { op: 'call', target: { ref: 0 } },
      nested('missing'),
      nested('noun.missing'),
      nested('absent.read'),
      nested('bad.read'),
      { op: 'call', target: { ref: 0 }, method: 'read' },
      { op: 'call', target: { ref: 999 } },
    ],
  });
  assert.deepEqual(r.unmatched, []);
  assert.deepEqual(
    r.trace.slice(0, 3).map((t) => t.outcome),
    [{ returned: true }, { returned: 1 }, { returned: 'receiver' }],
  );
  assert.deepEqual(r.trace[1].callbacks, [{ fn: 'getter', args: [] }]);
  assert.deepEqual(r.trace[2].callbacks, [
    { fn: 'getter', args: [] },
    { fn: 'deeper', args: [] },
  ]);
  assert.deepEqual(r.trace[3].outcome, { returned: 3 });
  assert.deepEqual(r.trace[3].callbacks, [
    { fn: 'getter', args: [] },
    { fn: 'callable', args: [] },
  ]);
  assert.deepEqual(r.trace[5].outcome, { returned: 'removed' });
  assert.deepEqual(
    errors(r.trace).map((e) => (e.error as { $error: { message: string } }).$error.message),
    [
      '[scenario] "missing" is not a function',
      '[scenario] "noun.missing" is not a function',
      '[scenario] "absent.read" is not a function',
      'getter refused',
      '[scenario] a ref is a function; call it without a method',
      '[scenario] no ref 999',
    ],
  );
});
test('the runner invokes a real current annotation noun', async () => {
  const r = await runScenario(
    scenario(
      'real-noun',
      'Current shipping nested annotation selection',
      [call('selection.set', [[objRef(1)]]), call('selection.list')],
      [hydrate([square(1)])],
    ),
  );
  assert.deepEqual(errors(r.trace), []);
  assert.deepEqual(r.unmatched, []);
  assert.deepEqual(r.trace.at(-1)!.outcome, { returned: [square(1)] });
});
const descriptor = () => Object.getOwnPropertyDescriptor(globalThis.crypto, 'getRandomValues');
const writeIds = (trace: TraceEntry[]) =>
  trace
    .flatMap((t) => t.engine)
    .filter(
      (e): e is { path: string; args: [unknown, { opId: string }] } =>
        !!e && typeof e === 'object' && 'path' in e && e.path === 'apply',
    )
    .map((e) => e.args[1].opId);
test('live entropy resets per case, stays unique across documents and restores the exact crypto descriptor', async () => {
  const before = descriptor();
  const proto = Object.getPrototypeOf(globalThis.crypto);
  const draft = {
    subtype: 'square',
    box: { x: 1, y: 2, width: 3, height: 4 },
  } satisfies AnnotationDraft;
  const born = materialize(10001, draft);
  const s = scenario(
    'entropy',
    'Queue UUIDv7 host input',
    [
      call('create', [pr(1), draft]),
      call('update', [objRef(10001), { color: '#000000' }]),
      open('b'),
      call('create', [pr(1), draft], 'b'),
    ],
    [],
    {
      engine: {
        documents: {
          a: doc([
            hydrate(),
            write(1, [{ item: created(born) }]),
            write(2, [{ item: updated(annotationAfter(born, { color: '#000000' })) }]),
          ]),
          b: doc([hydrate(), write(3, [{ item: created(born) }])]),
        },
      },
    },
  );
  const a = await runScenario(s);
  assert.deepEqual(descriptor(), before);
  assert.equal(Object.getPrototypeOf(globalThis.crypto), proto);
  const b = await runScenario(s);
  assert.deepEqual(errors(a.trace), []);
  assert.deepEqual(a.unmatched, []);
  assert.deepEqual(a.trace, b.trace);
  assert.deepEqual(writeIds(a.trace), writeIds(b.trace));
  assert.equal(new Set(writeIds(a.trace)).size, 3);
  assert.deepEqual(descriptor(), before);
  const refusedRun = await runScenario(
    scenario(
      'entropy-refusal',
      'Refusal cleanup',
      [call('create', [pr(1), { subtype: 'square', box: { x: 1, y: 2, width: 3, height: 4 } }])],
      [hydrate(), refused()],
    ),
  );
  assert.equal(errors(refusedRun.trace).length, 1);
  assert.deepEqual(descriptor(), before);
  await assert.rejects(
    () =>
      runScenario({
        ...s,
        name: 'runner-error',
        steps: [...boot, { op: 'settle', label: 'no-pending' }],
      }),
    /nothing pending/,
  );
  assert.deepEqual(descriptor(), before);
});

import { suites as apiSuites } from '../src/suites/annotation-api';
import { suites as crudSuites } from '../src/suites/annotation-crud';
import { suites as hydrationSuites } from '../src/suites/annotation-hydration';
import {
  suites as reconciliationSuites,
  reconciliationMapping,
} from '../src/suites/annotation-reconciliation';
import { expectedRefusals } from './annotation-live-refusals';
import { AnnotationSchema } from '../../../packages/engine/core/src/annotation/kinds';
import {
  annotationOfDraft,
  type Annotation,
  type AnnotationDraft,
  type ChangeOp,
  type ChangeResult,
} from '@embedpdf/engine-core/runtime';
import { annotationAfter } from '../../../packages/core/annotation/src/record';
import type { AnnotationState } from '../../../packages/plugin/annotation/src/model';
const owned = [...apiSuites, ...crudSuites, ...hydrationSuites, ...reconciliationSuites];
const currentCases = owned.flatMap((s) => (s.kind === 'scenario' ? s.cases : []));
const runs = new Map<string, { input: ScenarioCase; trace: TraceEntry[] }>();
function messages(value: unknown): string[] {
  if (Array.isArray(value)) return value.flatMap(messages);
  if (!value || typeof value !== 'object') return [];
  const o = value as Record<string, unknown>;
  if ('$error' in o) return [(o.$error as { message: string }).message];
  if (typeof o.code === 'string' && typeof o.message === 'string') return [o.message];
  return Object.entries(o).flatMap(([k, v]) => [
    ...(k === 'failed' && Array.isArray(v) && v.length ? [`failed:${v.length}`] : []),
    ...(k === 'diagnostics' && Array.isArray(v) && v.length
      ? v.map((x) => `diagnostic:${JSON.stringify(x)}`)
      : messages(v)),
  ]);
}
function diagnostics(trace: TraceEntry[]): [number, string, string[]][] {
  return trace.flatMap((t, i) =>
    (['outcome', 'settled', 'reports', 'callbacks'] as const).flatMap((channel) => {
      const found = messages(t[channel]);
      for (const cb of channel === 'callbacks' ? t.callbacks : [])
        if (cb.fn.startsWith('console.') && messages(cb.args).length === 0)
          found.push(`${cb.fn}:${JSON.stringify(cb.args)}`);
      return found.length ? [[i, channel, found] as [number, string, string[]]] : [];
    }),
  );
}
function getRun(name: string) {
  const r = runs.get(name);
  assert.ok(r, name);
  return r;
}
function read<T = unknown>(name: string, method: string, which = -1): T {
  const r = getRun(name);
  const indexes = r.input.steps.flatMap((s, i) =>
    s.op === 'call' && s.method === method ? [i] : [],
  );
  const i = which < 0 ? indexes.at(which) : indexes[which];
  assert.notEqual(i, undefined, `${name}:${method}`);
  const outcome = r.trace[i!]!.outcome;
  assert.ok(outcome);
  if ('returned' in outcome) return outcome.returned as T;
  assert.ok('promise' in outcome, `${name}:${method} threw`);
  const settled = r.trace.flatMap((t) => t.settled).find((s) => s.promise === outcome.promise);
  assert.ok(settled && 'resolved' in settled, `${name}:${method} unresolved/rejected`);
  return settled.resolved as T;
}
function session(name: string, i = -1) {
  const state = getRun(name).trace.at(i)?.state as { plugins: Record<string, AnnotationState> };
  assert.ok(state?.plugins['annotation::a']);
  return state.plugins['annotation::a']!.session;
}
function records(name: string) {
  return read<Annotation[]>(name, 'list');
}
function stateAtMethod(name: string, method: string) {
  const r = getRun(name);
  const i = r.input.steps.findIndex((s) => s.op === 'call' && s.method === method);
  assert.ok(i >= 0);
  return session(name, i);
}
function approximate(actual: unknown, expected: unknown, path = ''): void {
  if (typeof actual === 'number' && typeof expected === 'number') {
    assert.ok(
      Math.abs(actual - expected) < 1e-8 * Math.max(1, Math.abs(expected)),
      `${path}: ${actual} != ${expected}`,
    );
    return;
  }
  if (Array.isArray(actual) && Array.isArray(expected)) {
    assert.equal(actual.length, expected.length, path);
    actual.forEach((v, i) => approximate(v, expected[i], `${path}[${i}]`));
    return;
  }
  if (actual && expected && typeof actual === 'object' && typeof expected === 'object') {
    const a = actual as Record<string, unknown>,
      e = expected as Record<string, unknown>;
    assert.deepEqual(Object.keys(a).sort(), Object.keys(e).sort(), path);
    for (const k of Object.keys(e)) approximate(a[k], e[k], `${path}.${k}`);
    return;
  }
  assert.deepEqual(actual, expected, path);
}
test('all82 current live scenarios execute with exact named refusals and no unexpected diagnostics', async () => {
  const unexpectedConsole: unknown[][] = [];
  const saved = console.error;
  console.error = (...args: unknown[]) => unexpectedConsole.push(args);
  try {
    const seen = new Set<string>();
    for (const c of currentCases) {
      assert.ok(!seen.has(c.name), c.name);
      seen.add(c.name);
      const r = await runScenario(c);
      assert.deepEqual(r.unmatched, [], c.name);
      assert.deepEqual(diagnostics(r.trace), expectedRefusals[c.name] ?? [], c.name);
      runs.set(c.name, { input: c, trace: r.trace });
    }
    assert.equal(seen.size, 82);
    assert.deepEqual(unexpectedConsole, []);
  } finally {
    console.error = saved;
  }
});
test('every successful apply has matching current ops/items/events/opId/birth refs and semantically correct read-backs', () => {
  let writes = 0,
    creates = 0,
    updates = 0;
  for (const [name, { input, trace }] of runs) {
    const responses = input.engine!.documents!.a!.responses!.filter((r) => r.path === 'apply');
    const calls = trace
      .flatMap((t) => t.engine)
      .filter(
        (e): e is { path: string; args: [{ ops: ChangeOp[] }, { opId: string }] } =>
          !!e && typeof e === 'object' && 'path' in e && e.path === 'apply',
      );
    assert.equal(calls.length, responses.length, `${name}: unused/missing apply response`);
    calls.forEach((call, i) => {
      writes++;
      const response = responses[i]!;
      if (!response.value) return;
      const result = response.value as ChangeResult;
      assert.equal(call.args[1].opId, result.meta.opId, name);
      assert.equal(result.items.length, call.args[0].ops.length, name);
      assert.equal(response.events!.length, result.items.length, name);
      call.args[0].ops.forEach((op, j) => {
        const item = result.items[j]!;
        assert.equal(item.type, op.type, name);
        const event = response.events![j]!;
        assert.ok('origin' in event);
        assert.deepEqual(
          event.origin.tx,
          { id: result.meta.opId, index: j, count: result.items.length },
          name,
        );
        assert.equal(event.origin.kind, 'local');
        assert.equal(typeof event.origin.ts, 'number');
        assert.equal(event.origin.serverId, null);
        if (!('annotation' in item)) return;
        assert.equal(
          AnnotationSchema.safeParse(item.annotation).success,
          true,
          `${name}: incomplete annotation`,
        );
        if (op.type === 'annotations.create') {
          creates++;
          assert.deepEqual(
            item.annotation.ref,
            { kind: 'objectNumber', page: op.page, objectNumber: op.objectNumber },
            name,
          );
          const projected = annotationOfDraft(op.data, { ref: item.annotation.ref });
          // The engine supplies file metadata and its appearance/NM; all other read fields follow the actual stated draft.
          const omit = (a: Annotation) =>
            Object.fromEntries(
              Object.entries(a).filter(
                ([k]) => !['file', 'nm', 'hasAppearance', 'appearanceState'].includes(k),
              ),
            );
          approximate(omit(item.annotation), omit(projected), `${name}: create`);
          if (op.data.subtype === 'stamp')
            assert.ok(op.resources?.appearance, `${name}: stamp has appearance bytes`);
          if (op.data.subtype === 'file-attachment')
            assert.ok(op.resources?.file, `${name}: attachment has file bytes`);
        }
        if (op.type === 'annotations.update') {
          updates++;
          assert.deepEqual(item.annotation.ref, op.ref, name);
          approximate(
            annotationAfter(item.annotation, op.patch),
            item.annotation,
            `${name}: patch already held`,
          );
        }
      });
    });
  }
  assert.ok(
    writes > 70 && creates > 35 && updates > 25,
    JSON.stringify({ writes, creates, updates }),
  );
});
test('public CRUD preserves successful kinds, flags, resources, geometry and whole-change rollback', () => {
  const made = records('create-from-page-space');
  assert.equal(made.length, 10);
  assert.deepEqual(
    made.slice(0, 9).map((a) => a.subtype),
    ['square', 'circle', 'line', 'polygon', 'polyline', 'ink', 'highlight', 'redact', 'free-text'],
  );
  assert.deepEqual(made.at(-1)!.ref, objRef(10012));
  assert.equal(made.at(-1)!.hidden, true);
  assert.equal(made.at(-1)!.locked, false);
  assert.equal(made.at(-1)!.print, false);
  assert.equal(read('create-without-authority', 'canCreate'), false);
  assert.equal(read('create-without-authority', 'draft.get'), null);
  const changed = records('update-in-page-space');
  assert.ok(!changed.some((a) => a.ref.kind === 'objectNumber' && a.ref.objectNumber === 3));
  assert.ok(changed.some((a) => a.ref.kind === 'objectNumber' && a.ref.objectNumber === 4));
  const sq = changed.find(
    (a) => a.subtype === 'square' && a.ref.kind === 'objectNumber' && a.ref.objectNumber === 1,
  )!;
  assert.equal(sq.contents, 'new');
  assert.equal(sq.hidden, true);
  assert.deepEqual(
    records('restyle-writes-and-rolls-back')
      .slice(0, 2)
      .map((a) => ('color' in a ? a.color : null)),
    ['#ef4444', '#ef4444'],
  );
  assert.deepEqual(
    records('flags-writes').map((a) => ({ locked: a.locked, print: a.print, hidden: a.hidden })),
    [
      { locked: false, print: false, hidden: false },
      { locked: false, print: false, hidden: false },
    ],
  );
  const drawn = records('drawn-shape-commits')[0]!;
  assert.ok('box' in drawn);
  assert.deepEqual(drawn.box, { x: 100, y: 100, width: 80, height: 60 });
  assert.deepEqual(read('attachments-stamps-and-downloads', 'downloadResource'), {
    $bytes: 'AAEC',
  });
  const placed = getRun('attachments-stamps-and-downloads')
    .trace.flatMap((t) => t.engine)
    .filter(
      (e): e is { args: [{ ops: ChangeOp[] }] } =>
        !!e && typeof e === 'object' && 'path' in e && e.path === 'apply',
    );
  assert.equal(placed.length, 3);
  assert.ok(placed[0]!.args[0].ops[0]!.type === 'annotations.create');
});
test('rich text, typing switches and link previews retain actual successful content', () => {
  const props = read<{ values: Record<string, unknown> }>(
    'rich-text-editing',
    'selection.getProperties',
  );
  assert.equal(props.values.italic, true);
  const editing = read<Annotation>('rich-text-editing', 'text.getEditing', 1);
  assert.equal(editing.contents, 'Typing');
  const richResult = read<Extract<Annotation, { subtype: 'free-text' }>>(
    'rich-text-editing',
    'get',
  );
  assert.equal(richResult.fontSize, 20);
  assert.equal(richResult.richText.paragraphs[0]!.runs[0]!.text, 'Typing');
  assert.equal(richResult.richText.paragraphs[0]!.runs[0]!.style!.italic, true);
  assert.equal(read('rich-text-editing', 'text.getEditing'), null);
  assert.equal(
    read<Annotation>('text-commits-flush-when-switching-annotation', 'get', 0).contents,
    'a2',
  );
  assert.equal(
    read<Annotation>('text-commits-flush-when-switching-annotation', 'get', 1).contents,
    'b1',
  );
  const nav = read<{ target: { uri: string } }[]>(
    'live-link-targets-and-finishing-a-draft',
    'listLinkItems',
  );
  assert.equal(nav[0]!.target.uri, 'https://new');
  const picked = getRun('awaited-writes-and-pending-text')
    .trace.flatMap((t) => t.callbacks)
    .filter((c) => c.fn === 'first' || c.fn === 'second');
  assert.deepEqual(
    picked.map((c) => c.fn),
    ['second'],
  );
  assert.equal(read('awaited-writes-and-pending-text', 'placeAt', 0), true);
  assert.equal(read('awaited-writes-and-pending-text', 'placeAt', 1), false);
  assert.equal(read('memoized-selectors-and-edges', 'isPending', 0), true);
  assert.equal(read('memoized-selectors-and-edges', 'isPending', 1), false);
  assert.equal(
    read<Extract<Annotation, { subtype: 'square' }>>('memoized-selectors-and-edges', 'get').color,
    '#000000',
  );
});
test('measurement drafts and recalibration assert successful metric labels and explicit refusal reports', () => {
  const distance = records('measurement-distance-drafts')[0]!;
  assert.equal(distance.contents, '2.00 m');
  assert.ok(distance.subtype === 'line');
  assert.equal(distance.leader!.length, 30);
  const perim = records('dimension-drafts-continue-without-viewports')[0]!;
  assert.ok(perim.subtype === 'polyline');
  assert.equal(perim.vertices.length, 3);
  assert.equal(perim.contents, '2.78 m');
  const captured = getRun('measurement-distance-drafts')
    .trace.flatMap((t) => t.callbacks)
    .filter((c) => c.fn === 'captured');
  assert.equal(captured.length, 1);
  assert.equal((captured[0]!.args[0] as { tool: string }).tool, 'calibrate');
  const report = read<{
    updated: unknown[];
    skipped: { reason: string }[];
    failed: { ref: unknown }[];
  }>('remeasure-page', 'remeasurePage', 0);
  assert.deepEqual(report.updated, [objRef(1)]);
  assert.deepEqual(
    report.skipped.map((x) => x.reason),
    ['locked', 'foreign-measure'],
  );
  assert.deepEqual(
    report.failed.map((x) => x.ref),
    [objRef(6)],
  );
  const denied = read<{ skipped: { reason: string }[] }>(
    'remeasure-without-authority-and-after-errors',
    'remeasurePage',
    0,
  );
  assert.deepEqual(
    denied.skipped.map((x) => x.reason),
    ['no-authority'],
  );
  assert.equal(read('calibration-needs-modify', 'draft.get', 0), null);
  assert.equal(session('calibration-needs-modify', 6).draft?.kind, 'create-rect');
});
test('current mirror replay, page reloads, permission gates and deliberate protocol negatives assert actual state', () => {
  const numbers = (name: string) =>
    records(name).map((a) =>
      a.ref.kind === 'objectNumber' ? a.ref.objectNumber : a.ref.baseIndex,
    );
  assert.deepEqual(numbers('the-hydration-window-replays-by-cursor'), [1, 2, 7]);
  assert.equal(records('the-hydration-window-replays-by-cursor')[1]!.contents, 'new');
  assert.deepEqual(numbers('a-local-engine-replays-everything'), [7, 8]);
  assert.deepEqual(numbers('mixed-type-cursors-compare-as-javascript'), [10]);
  assert.deepEqual(numbers('a-null-cursor-reads-as-zero'), [7, 8]);
  assert.deepEqual(numbers('string-cursors-compare-as-strings'), [8]);
  assert.equal(read('no-read-grant-means-forbidden', 'getStatus'), 'forbidden');
  assert.equal(read('hydrates-every-page-at-open', 'getStatus'), 'ready');
  assert.equal(read('records-without-a-ref', 'getStatus'), 'error');
  assert.deepEqual(numbers('records-without-a-ref'), [1]);
  assert.equal(read<Annotation>('stable-base-index-records', 'get').contents, 'base reloaded');
  assert.deepEqual(session('stable-base-index-records').selected, ['base:1:0']);
  assert.equal(read('restored-page-reloads', 'getStatus'), 'error');
  assert.deepEqual(numbers('restored-page-reloads'), [2, 3]);
  assert.deepEqual(numbers('cross-plane-events'), [1, 3]);
  assert.deepEqual(
    read<Annotation[]>('page-membership-events', 'list').map((a) => a.ref),
    [objRef(4, 2)],
  );
});

import type { RenderItem } from '../../../packages/core/annotation/src/types';
const ids = (name: string) =>
  records(name).map((a) =>
    a.ref.kind === 'objectNumber'
      ? `obj:${a.ref.objectNumber}`
      : `base:${a.ref.page.objectNumber}:${a.ref.baseIndex}`,
  );
function pendingState(name: string, kind: string) {
  const r = getRun(name);
  const ack = r.input.steps.findIndex((s) => s.op === 'settle');
  assert.ok(ack > 0);
  assert.equal(
    session(name, ack - 1).draft?.kind,
    kind,
    `${name}: actual gesture before confirmation`,
  );
  assert.equal(session(name, ack).draft?.kind, kind, `${name}: actual gesture after confirmation`);
  assert.deepEqual(session(name).selected, ['obj:10001']);
  assert.equal(session(name).draft, null);
  assert.deepEqual(records(name)[0]!.ref, objRef(10001));
}
function reconciliationAssertion(name: string): void {
  const short = name.slice('reconcile/'.length);
  switch (short) {
    case 'loaded':
      assert.deepEqual(ids(name), ['obj:1', 'obj:2', 'obj:3']);
      assert.ok(records(name)[0]!.subtype === 'square');
      assert.deepEqual(records(name)[0]!.rect, { x: 0, y: 0, width: 10, height: 10 });
      break;
    case 'hydrated-reaps':
      assert.deepEqual(ids(name), ['obj:1']);
      assert.deepEqual(session(name).selected, []);
      assert.equal(session(name).editing, null);
      assert.equal(session(name).hovered, null);
      break;
    case 'hydrated-bump':
      assert.equal(read(name, 'getAppearanceEpoch', 0), 'obj:1@0|obj:2@0');
      assert.equal(read(name, 'getAppearanceEpoch', 1), 'obj:1@1|obj:3@0');
      break;
    case 'hydrated-keeps-dragged':
      assert.equal(session(name, 4).draft?.kind, 'move');
      assert.equal(session(name, 5).draft, null);
      assert.deepEqual(ids(name), []);
      assert.deepEqual(session(name).selected, []);
      break;
    case 'hydrated-nothing-reaped':
      assert.deepEqual(ids(name), ['obj:1']);
      assert.deepEqual(session(name).selected, ['obj:1']);
      break;
    case 'hydrated-empty-hover':
      assert.deepEqual(ids(name), []);
      assert.equal(session(name).hovered, null);
      assert.equal(session(name).editing, null);
      assert.equal(session(name).draft, null);
      break;
    case 'hydrated-keeps-surviving-hover':
      assert.deepEqual(ids(name), ['obj:1', 'obj:2']);
      assert.equal(session(name).hovered, 'obj:1');
      assert.equal(session(name).editing, 'obj:2');
      break;
    case 'created':
      assert.deepEqual(read<Annotation>(name, 'text.getEditing', 0).ref, objRef(10001));
      assert.deepEqual(read<Annotation>(name, 'text.getEditing', 1).ref, objRef(10001));
      assert.equal(session(name).editing, 'obj:10001');
      break;
    case 'created-index-ref':
      assert.deepEqual(ids(name), ['base:1:0', 'obj:10001']);
      assert.deepEqual(session(name).selected, ['base:1:0']);
      assert.deepEqual(read<Annotation>(name, 'get').ref, {
        kind: 'baseIndex',
        page: pr(1),
        baseIndex: 0,
      });
      break;
    case 'created-mid-move':
    case 'created-then-echo-mid-move':
      pendingState(name, 'move');
      assert.deepEqual(records(name)[0]!.rect, { x: 120, y: 120, width: 100, height: 100 });
      if (short === 'created-then-echo-mid-move')
        assert.equal(records(name)[0]!.contents, 'remote echo');
      break;
    case 'created-mid-handle':
      pendingState(name, 'handle');
      assert.deepEqual(records(name)[0]!.rect, { x: 100, y: 100, width: 120, height: 120 });
      break;
    case 'created-mid-rotate':
      pendingState(name, 'rotate');
      assert.ok('rotation' in records(name)[0]!);
      assert.equal((records(name)[0] as Extract<Annotation, { subtype: 'square' }>).rotation, 90);
      break;
    case 'created-mid-poly-draft':
      assert.deepEqual(read(name, 'draft.get', 0), read(name, 'draft.get', 1));
      assert.ok(read(name, 'draft.get', 0));
      assert.deepEqual(ids(name), ['obj:10001', 'obj:10002']);
      assert.equal(session(name).draft, null);
      assert.deepEqual(session(name).selected, ['obj:10002']);
      break;
    case 'created-relationships': {
      const rows = records(name);
      assert.deepEqual(ids(name), ['obj:10001', 'obj:10002', 'obj:10003', 'obj:10004']);
      for (const a of rows.slice(1)) assert.deepEqual(a.reply!.to, objRef(10001));
      assert.equal(rows[1]!.reply!.type, 'reply');
      assert.equal(rows[2]!.reply!.type, 'group');
      assert.ok(rows[3]!.subtype === 'link');
      assert.deepEqual(rows[3]!.target, { kind: 'uri', uri: 'https://example.com' });
      assert.deepEqual(session(name).selected, ['obj:10001', 'obj:10003']);
      break;
    }
    case 'created-unknown':
      assert.deepEqual(ids(name), ['obj:1', 'obj:99']);
      assert.deepEqual(session(name).selected, ['obj:1']);
      assert.equal(
        read<RenderItem[]>(name, 'listPageItems').find((x) => x.id === 'obj:99')!.source,
        'baked',
      );
      break;
    case 'create-failed':
      assert.deepEqual(ids(name), ['obj:1']);
      assert.deepEqual(session(name).selected, ['obj:1']);
      break;
    case 'create-failed-mid-move':
    case 'create-failed-mid-handle': {
      const ack = getRun(name).input.steps.findIndex((s) => s.op === 'settle');
      assert.equal(session(name, ack - 1).draft?.kind, short.endsWith('move') ? 'move' : 'handle');
      assert.equal(session(name, ack).draft, null);
      assert.deepEqual(session(name, ack).selected, []);
      assert.deepEqual(ids(name), []);
      break;
    }
    case 'upsert':
      assert.deepEqual(ids(name), ['obj:1', 'obj:2', 'obj:3']);
      assert.equal(records(name)[0]!.contents, 'second');
      assert.equal(read(name, 'getAppearanceEpoch', 0), read(name, 'getAppearanceEpoch', 1));
      break;
    case 'upsert-while-dragging':
    case 'upsert-while-rotating':
    case 'upsert-while-group': {
      const kind = short.endsWith('dragging')
        ? 'move'
        : short.endsWith('rotating')
          ? 'rotate'
          : 'group';
      assert.equal(session(name, 3).draft?.kind, kind);
      assert.equal(session(name, 4).draft?.kind, kind);
      assert.equal(session(name).draft, null);
      assert.equal(records(name)[0]!.contents, 'remote');
      assert.ok(session(name).selected.includes('obj:1'));
      break;
    }
    case 'upsert-while-marquee':
      assert.equal(session(name, 4).draft?.kind, 'marquee');
      assert.deepEqual(session(name).selected, ['obj:1']);
      assert.equal(session(name).draft, null);
      assert.equal(
        (records(name)[0] as Extract<Annotation, { subtype: 'square' }>).color,
        '#000000',
      );
      break;
    case 'bump-ap':
      assert.equal(read(name, 'getAppearanceEpoch', 0), 'obj:1@0');
      assert.equal(read(name, 'getAppearanceEpoch', 1), 'obj:1@0');
      assert.equal(read(name, 'getAppearanceEpoch', 2), 'obj:1@1');
      break;
    case 'measurement-create-annot': {
      const [line, link, polygon] = records(name);
      assert.ok(line?.subtype === 'line');
      assert.equal(line.color, '#0000ff');
      assert.equal(line.lineEndings!.end, 'closed-arrow');
      assert.equal(line.print, true);
      assert.ok(link?.subtype === 'link');
      assert.deepEqual(link.target, { kind: 'uri', uri: 'https://e.x' });
      assert.equal(polygon!.hidden, true);
      assert.deepEqual(polygon!.page, pr(2));
      break;
    }
    case 'measurement-hydrated-under-caption-drag':
      assert.equal(session(name, 4).draft?.kind, 'caption');
      assert.equal(session(name, 5).draft?.kind, 'caption');
      assert.equal(session(name, 7).draft, null);
      assert.deepEqual(session(name).selected, []);
      assert.deepEqual(ids(name), []);
      break;
    default:
      assert.fail(`unmapped reconciliation assertion: ${short}`);
  }
}
test('all25 update +2 measurement retirements have unique current scenarios and explicit behavioral assertions', () => {
  assert.equal(reconciliationMapping.length, 27);
  assert.equal(new Set(reconciliationMapping.map((x) => `${x.family}/${x.historical}`)).size, 27);
  assert.equal(reconciliationMapping.filter((x) => x.family === 'annotation-update').length, 25);
  assert.equal(
    reconciliationMapping.filter((x) => x.family === 'annotation-measurement-model').length,
    2,
  );
});
for (const row of reconciliationMapping)
  test(`${row.family}/${row.historical} → ${row.scenario}`, () =>
    reconciliationAssertion(row.scenario));
test('H3 extended stamp/measurement scenarios preserve preview lifetime, geometry and real draft kinds', () => {
  for (const [name, kind] of [
    ['reconcile/h3-confirmed-deletion-mid-move', 'move'],
    ['reconcile/h3-mirror-disappearance-mid-handle', 'handle'],
  ] as const) {
    const r = getRun(name);
    const ack = r.input.steps.findIndex((s) => s.op === 'settle');
    assert.equal(session(name, ack - 1).draft?.kind, kind);
    assert.equal(session(name, ack).draft?.kind, kind);
    assert.deepEqual(ids(name), []);
    assert.deepEqual(session(name).selected, []);
    assert.equal(session(name).draft, null);
  }
  for (const refused of [false, true]) {
    const name = `reconcile/h3-pending-stamp-${refused ? 'refuse' : 'confirm'}`;
    const preview = read<{ ref: unknown; image: unknown }[]>(name, 'renderAppearances', 0);
    assert.equal(preview.length, 1);
    assert.deepEqual(preview[0]!.ref, objRef(10001));
    assert.ok(preview[0]!.image);
    assert.deepEqual(read(name, 'renderAppearances', 1), []);
    assert.equal(session(name, 7).draft?.kind, 'move');
    assert.equal(session(name).draft, null);
    if (refused) {
      assert.deepEqual(ids(name), []);
      assert.deepEqual(session(name).selected, []);
    } else {
      const a = records(name)[0]!;
      assert.ok(a.subtype === 'stamp');
      assert.deepEqual(a.box, { x: 130, y: 130, width: 100, height: 50 });
      assert.equal(a.name, 'Approved');
    }
  }
  const caption = 'reconcile/h3-pending-caption-confirm';
  assert.equal(session(caption, 5).draft?.kind, 'caption');
  assert.equal(session(caption, 6).draft?.kind, 'caption');
  assert.equal(session(caption).draft, null);
  const a = records(caption)[0]!;
  assert.ok(a.subtype === 'line');
  assert.deepEqual(a.captionOffset, { along: 20, perpendicular: -20 });
  assert.equal(a.contents, '2.00 m');
  assert.deepEqual(a.ref, objRef(10001));
});

import { createKernel } from '@embedpdf/core';
import { createFakeEngine } from '../src/fake-engine';
import { annotationPlugin } from '../../../packages/plugin/annotation/src/annotation.plugin';
import { AnnotationToken } from '../../../packages/plugin/annotation/src/host-contract';
import { interactionPlugin } from '../../../packages/plugin/interaction/src';
const turn = () => new Promise<void>((resolve) => setImmediate(resolve));
test('H3 directly proves synchronous forget during actual store/mirror deletion and refusal', async () => {
  for (const mode of ['confirm', 'delete', 'refuse'] as const) {
    const reports: unknown[] = [];
    const fake = createFakeEngine(
      {
        documents: {
          a: doc([
            hydrate(),
            { path: 'apply', times: 1, defer: 'born' },
            { path: 'apply', defer: 'moved' },
          ]),
        },
      },
      (v) => v,
      (e) => reports.push(e),
    );
    const kernel = createKernel({
      engine: fake.engine,
      plugins: [interactionPlugin(), annotationPlugin()],
      report: (e) => reports.push(e),
    });
    try {
      await kernel.start();
      await kernel.documents.open({ kind: 'bytes', id: 'a', bytes: new Uint8Array() });
      const a = kernel.capability(AnnotationToken, 'a');
      await a.whenSynced();
      const state = () => (kernel.getState().plugins['annotation::a'] as AnnotationState).session;
      let refusalObserved = false;
      a.onWriteFailed(() => {
        refusalObserved = true;
      });
      const initialList = a.list();
      assert.equal(a.list(), initialList);
      const initialItems = a.listPageItems(pr(1));
      assert.equal(a.listPageItems(pr(1)), initialItems);
      const draft = {
        subtype: 'square',
        box: { x: 100, y: 100, width: 100, height: 100 },
        color: '#ef4444',
        strokeWidth: 2,
      } satisfies AnnotationDraft;
      const created = a.create(pr(1), draft);
      void created.catch(() => {});
      await turn();
      assert.notEqual(a.list(), initialList);
      assert.deepEqual(a.list()[0]!.ref, objRef(10001));
      assert.equal(a.isPending(objRef(10001)), true);
      a.selection.set([objRef(10001)]);
      a.editPointer('down', pr(1), { x: 100, y: 125 }, false);
      a.editPointer('move', pr(1), { x: 110, y: 135 }, false);
      assert.equal(state().draft?.kind, 'move');
      if (mode === 'refuse') {
        assert.equal(
          fake.settle('born', {
            error: { name: 'EngineError', code: 'Refused', message: 'intended direct refusal' },
          }),
          true,
        );
        await assert.rejects(Promise.resolve(created), /intended direct refusal/);
        assert.equal(refusalObserved, true);
        assert.equal(state().draft, null);
        assert.deepEqual(state().selected, []);
      } else {
        const call = fake.log.find(
          (x): x is { doc: string; path: string; args: [{ ops: ChangeOp[] }, { opId: string }] } =>
            'path' in x && x.path === 'apply',
        )!;
        const opId = call.args[1].opId;
        const born = materialize(10001, draft);
        const meta = {
          affectedPages: [pr(1)],
          changed: [objRef(10001)],
          cacheDelta: null,
          opId,
          undoable: true,
        };
        fake.emit('a', {
          type: 'annotations.created',
          page: pr(1),
          annotation: born,
          meta,
          origin: { ...LOCAL, tx: { id: opId, index: 0, count: 1 } },
        });
        assert.equal(
          state().draft?.kind,
          'move',
          'confirmation folds without ending the active move',
        );
        assert.equal(
          a.isPending(objRef(10001)),
          false,
          'the actual matching tx stops replay before the apply promise answers',
        );
        fake.settle('born', {
          value: {
            items: [{ type: 'annotations.create', page: pr(1), annotation: born, meta }],
            meta,
          },
        });
        await created;
        if (mode === 'delete') {
          fake.emit('a', deletedFact([objRef(10001)]));
          // No await, drain, timer or queued check between the fact and these reads.
          assert.equal(state().draft, null, 'store watch commits forget synchronously');
          assert.deepEqual(state().selected, []);
          assert.deepEqual(a.list(), []);
        }
      }
      a.editPointer('move', pr(1), { x: 120, y: 145 }, false);
      a.editPointer('up', pr(1), { x: 120, y: 145 }, false);
      await turn();
      assert.equal(state().draft, null);
      if (mode === 'confirm') {
        const writes = fake.log.filter(
          (x): x is { doc: string; path: string; args: [{ ops: ChangeOp[] }, { opId: string }] } =>
            'path' in x && x.path === 'apply',
        );
        assert.equal(writes.length, 2);
        const update = writes[1]!.args[0].ops[0]!;
        assert.ok(update.type === 'annotations.update');
        assert.deepEqual(update.ref, objRef(10001));
        assert.deepEqual(update.patch, {
          box: { x: 120, y: 120, width: 100, height: 100 },
          subtype: 'square',
        });
        const annotation = annotationAfter(materialize(10001, draft), update.patch);
        const opId = writes[1]!.args[1].opId;
        const meta = {
          affectedPages: [pr(1)],
          changed: [objRef(10001)],
          cacheDelta: null,
          opId,
          undoable: true,
        };
        const appearance = { changed: false, action: 'preserved' as const };
        fake.emit('a', {
          type: 'annotations.updated',
          page: pr(1),
          annotation,
          appearance,
          meta,
          origin: { ...LOCAL, tx: { id: opId, index: 0, count: 1 } },
        });
        fake.settle('moved', {
          value: {
            items: [{ type: 'annotations.update', page: pr(1), annotation, appearance, meta }],
            meta,
          },
        });
        await turn();
        assert.deepEqual(a.get(objRef(10001))!.rect, { x: 120, y: 120, width: 100, height: 100 });
      } else if (mode === 'delete')
        assert.equal(
          fake.log.filter((x) => 'path' in x && x.path === 'apply').length,
          1,
          'a synchronously forgotten deleted target never writes a move',
        );
      assert.deepEqual(fake.unmatched, []);
      assert.deepEqual(reports, []);
    } finally {
      await kernel.destroy();
    }
  }
});
const EntropyPort = createCapabilityToken<unknown>('annotation-test/entropy-port');
registerPlugin('annotation-test/entropy-port', {
  factory: () =>
    definePlugin({
      id: 'annotation-test/entropy-port',
      token: EntropyPort,
      create: () => {
        globalThis.crypto.getRandomValues(new Uint8Array(15));
        globalThis.crypto.getRandomValues(new Uint16Array(8));
        return { api: {} };
      },
    }),
  tokens: [EntropyPort],
});
test('live entropy delegates other crypto requests and restores prior accessor descriptors after construction failures', async () => {
  const before = descriptor(),
    proto = Object.getPrototypeOf(globalThis.crypto),
    original = globalThis.crypto.getRandomValues;
  const delegated: string[] = [];
  const forward = <T extends ArrayBufferView>(v: T): T => {
    delegated.push(`${v.constructor.name}:${v.byteLength}`);
    return Reflect.apply(original, globalThis.crypto, [v]) as T;
  };
  const previous = { configurable: true, enumerable: false, get: () => forward, set: undefined };
  Object.defineProperty(globalThis.crypto, 'getRandomValues', previous);
  try {
    const r = await runScenario(
      scenario(
        'entropy-delegation',
        'Only UUID-sized Uint8Array uses deterministic entropy',
        [],
        [hydrate()],
        { plugins: [...PLUGINS, { factory: 'annotation-test/entropy-port' }] },
      ),
    );
    assert.deepEqual(r.unmatched, []);
    assert.deepEqual(delegated, ['Uint8Array:15', 'Uint16Array:16']);
    assert.deepEqual(descriptor(), previous);
    assert.equal(Object.getPrototypeOf(globalThis.crypto), proto);
    const bad = await runScenario(
      scenario(
        'bad-construction',
        'A tool construction failure still restores host crypto',
        [],
        [hydrate()],
        {
          plugins: [
            { factory: 'interaction' },
            {
              factory: 'annotation-live',
              config: { tools: [{ id: 'invalid', extends: 'no-such-tool' }] },
            },
          ],
        },
      ),
    );
    assert.ok(bad.trace.flatMap((t) => t.settled).some((x) => x.rejected));
    assert.deepEqual(descriptor(), previous);
    await assert.rejects(
      () =>
        runScenario(
          scenario(
            'factory-failure',
            'Runner cleanup applies when a later factory throws',
            [],
            [hydrate()],
            { plugins: [...PLUGINS, { factory: 'missing-fixture-factory' }] },
          ),
        ),
      /no plugin factory/,
    );
    assert.deepEqual(descriptor(), previous);
  } finally {
    if (before) Object.defineProperty(globalThis.crypto, 'getRandomValues', before);
    else delete (globalThis.crypto as unknown as Record<string, unknown>).getRandomValues;
  }
});

test('the explicit internal failure-callback reentry edge stays publicly empty and aborts its dependent update', async () => {
  const reports: unknown[] = [];
  const fake = createFakeEngine(
    {
      documents: {
        a: doc([
          hydrate(),
          { path: 'apply', times: 1, defer: 'born' },
          { path: 'apply', defer: 'moved' },
        ]),
      },
    },
    (v) => v,
    (e) => reports.push(e),
  );
  const kernel = createKernel({
    engine: fake.engine,
    plugins: [interactionPlugin(), annotationPlugin()],
    report: (e) => reports.push(e),
  });
  try {
    await kernel.start();
    await kernel.documents.open({ kind: 'bytes', id: 'a', bytes: new Uint8Array() });
    const a = kernel.capability(AnnotationToken, 'a');
    await a.whenSynced();
    const state = () => (kernel.getState().plugins['annotation::a'] as AnnotationState).session;
    let reentered = false,
      callbackError: unknown;
    a.onWriteFailed(() => {
      if (reentered) return;
      reentered = true;
      assert.equal(state().draft?.kind, 'move');
      try {
        a.editPointer('move', pr(1), { x: 120, y: 145 }, false);
        a.editPointer('up', pr(1), { x: 120, y: 145 }, false);
      } catch (error) {
        callbackError = error;
      }
    });
    const create = a.create(pr(1), {
      subtype: 'square',
      box: { x: 100, y: 100, width: 100, height: 100 },
      color: '#ef4444',
      strokeWidth: 2,
    });
    void create.catch(() => {});
    await turn();
    a.selection.set([objRef(10001)]);
    a.editPointer('down', pr(1), { x: 100, y: 125 }, false);
    a.editPointer('move', pr(1), { x: 110, y: 135 }, false);
    fake.settle('born', {
      error: { name: 'EngineError', code: 'Refused', message: 'intended reentry refusal' },
    });
    await assert.rejects(Promise.resolve(create), /intended reentry refusal/);
    await turn();
    assert.equal(reentered, true);
    assert.equal(callbackError, undefined);
    assert.equal(state().draft, null);
    // A separate current implementation edge; ordinary refusal tests above still require selected:[].
    assert.deepEqual(state().selected, ['obj:10001']);
    assert.deepEqual(a.selection.list(), []);
    assert.equal(a.selection.getAnchor(), null);
    assert.equal(a.get(objRef(10001)), null);
    assert.equal(a.canUpdate(objRef(10001)), false);
    assert.equal(a.canDelete(objRef(10001)), false);
    assert.deepEqual(a.list(), []);
    assert.deepEqual(a.listPageItems(pr(1)), []);
    assert.deepEqual(a.listChromeNodes(pr(1)), []);
    assert.equal(a.draft.get(), null);
    const writes = fake.log.flatMap((entry, index) =>
      'path' in entry && entry.path === 'apply' ? [{ entry, index }] : [],
    );
    assert.equal(writes.length, 2);
    assert.ok('path' in writes[1]!.entry);
    assert.ok(
      fake.log.some((entry) => 'abort' in entry && entry.abort === writes[1]!.index),
      'the dependent move is aborted',
    );
    assert.deepEqual(reports, []);
    assert.deepEqual(fake.unmatched, []);
  } finally {
    await kernel.destroy();
  }
});

import { readFile } from 'node:fs/promises';
test('the original49 live definitions, all3 renames,33 additions and27 historical mappings account exactly', async () => {
  const root = new URL('../historical/task-12b2b1b-i/', import.meta.url);
  const history = JSON.parse(await readFile(new URL('accounting.json', root), 'utf8')) as {
    base: string;
    before: number;
    after: number;
    mapped: number;
    retired: number;
    renamed: number;
    added: number;
    suites: {
      file: string;
      before: number;
      after: number;
      renamed: number;
      added: string[];
      cases: {
        before: string;
        after: string;
        status: string;
        oldCanonicalInputSha256: string;
        task11InputDrift: boolean;
        migrationNotes: string;
      }[];
    }[];
    newReconciliationSuite: { file: string; added: string[] };
  };
  assert.equal(history.base, '4a48f7acf');
  assert.deepEqual(
    [
      history.before,
      history.after,
      history.mapped,
      history.retired,
      history.renamed,
      history.added,
    ],
    [49, 82, 49, 0, 3, 33],
  );
  let mapped = 0,
    added = 0,
    renamed = 0,
    drifts = 0;
  for (const row of history.suites) {
    const suite = owned.find((s) => s.file === row.file);
    assert.ok(suite && suite.kind === 'scenario');
    assert.equal(row.before, row.cases.length);
    assert.equal(new Set(row.cases.map((c) => c.before)).size, row.before);
    assert.equal(row.after, suite.cases.length);
    const original = await readFile(
      new URL(`${row.file.slice('plugins/'.length, -'.json'.length)}.ts.txt`, root),
      'utf8',
    );
    for (const c of row.cases) {
      assert.ok(original.includes(`name: '${c.before}'`), c.before);
      assert.ok(
        suite.cases.some((now) => now.name === c.after),
        c.after,
      );
      assert.match(c.oldCanonicalInputSha256, /^[a-f0-9]{64}$/);
      assert.ok(c.migrationNotes.length > 30);
      mapped++;
      if (c.status === 'renamed') renamed++;
      if (c.task11InputDrift) drifts++;
    }
    assert.deepEqual(
      suite.cases
        .map((c) => c.name)
        .filter((n) => !row.cases.some((c) => c.after === n))
        .sort(),
      row.added,
    );
    added += row.added.length;
  }
  assert.equal(mapped, 49);
  assert.equal(renamed, 3);
  assert.equal(drifts, 2);
  assert.deepEqual(history.newReconciliationSuite.added, [
    ...reconciliationSuites.flatMap((s) => s.cases.map((c) => c.name)),
  ]);
  assert.equal(added + history.newReconciliationSuite.added.length, 33);
  const historical = JSON.parse(await readFile(new URL('reconciliation.json', root), 'utf8')) as {
    family: string;
    historical: string;
    scenario: string;
    assertion: string;
    definition: string;
  }[];
  assert.equal(historical.length, 27);
  assert.deepEqual(
    historical
      .map(({ family, historical, scenario }) => ({ family, historical, scenario }))
      .sort((a, b) => a.scenario.localeCompare(b.scenario)),
    [...reconciliationMapping].sort((a, b) => a.scenario.localeCompare(b.scenario)),
  );
  for (const row of historical) {
    assert.equal(row.assertion, `${row.family}/${row.historical} → ${row.scenario}`);
    assert.ok(row.definition.includes(`['${row.historical}'`));
    reconciliationAssertion(row.scenario);
  }
});
