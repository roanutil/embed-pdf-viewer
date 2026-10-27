import assert from 'node:assert/strict';
import { test } from 'node:test';
import { suites } from '../src/suites/redaction';
import { runScenario } from '../src/scenario';
test('pending projection returns three real marks through the current annotation plane', async () => {
  const suite = suites.find((s) => s.kind === 'scenario')!;
  assert.equal(suite.kind, 'scenario');
  const result = await runScenario(suite.cases.find((c) => c.name === 'pending-projection')!);
  assert.deepEqual(result.unmatched, []);
  const values = result.trace.flatMap((t) =>
    t.outcome && 'returned' in t.outcome ? [t.outcome.returned] : [],
  );
  assert.ok(
    values.some(
      (v) =>
        Array.isArray(v) &&
        v.length === 3 &&
        v.every((m) => m.kind === 'area' || m.kind === 'text'),
    ),
  );
  assert.ok(values.includes(3));
  assert.deepEqual(
    result.trace.flatMap((t) => t.reports),
    [],
  );
});

import { canonical } from '../src/canonical';
import type { ScenarioCase, TraceEntry } from '../src/scenario';
import type { FunctionSuite } from '../src/vectors';
import type {
  Annotation,
  AnnotationDraft,
  AnnotationPatch,
  DocumentEvent,
  ChangeResult,
  Change,
} from '@embedpdf/engine-core/runtime';
import {
  objRef,
  baseRef,
  pr,
  quad,
  layout,
  redact,
  square,
  result as answer,
} from '../src/suites/redaction-fixtures';
type ChangeSet = Extract<Change, { ops: unknown }>;
import { annotationReadPort, pending } from '../src/suites/redaction-read-inputs';
const scenarios = suites.find((s) => s.kind === 'scenario')!;
assert.equal(scenarios.kind, 'scenario');
const pure = suites.find((s) => s.kind === 'function') as FunctionSuite;
function redactDraft(data: AnnotationDraft) {
  assert.ok(data.subtype === 'redact');
  return data;
}
function redactPatch(data: AnnotationPatch) {
  assert.ok(data.subtype === 'redact');
  return data;
}
const cache = new Map<string, ReturnType<typeof runScenario>>();
function input(name: string): ScenarioCase {
  const c = scenarios.cases.find((c) => c.name === name);
  assert.ok(c, name);
  return c;
}
function run(name: string) {
  let p = cache.get(name);
  if (!p) {
    p = runScenario(input(name));
    cache.set(name, p);
  }
  return p;
}
function value<T = unknown>(trace: TraceEntry[], i: number): T {
  const o = trace[i]?.outcome;
  assert.ok(o && 'returned' in o, `returned step ${i}`);
  return o.returned as T;
}
function resolved<T = unknown>(trace: TraceEntry[], i: number): T {
  const result = trace[i]!.settled.find((s) => 'resolved' in s);
  assert.ok(result, `resolved step ${i}`);
  return result.resolved as T;
}
const err = (
  message: string,
  code: string,
  capability = 'redaction',
  permission: string | null = null,
  cause?: unknown,
) => ({
  $error: {
    name: 'PluginError',
    message,
    code,
    capability,
    permission,
    ...(cause ? { cause } : {}),
  },
});
const engine = (message: string, code = 'Refused') => ({
  $error: { name: 'EngineError', message, code },
});
const negative = (method: string, error: unknown) => ({ method, error });
const denied = (method: string, permission = 'annotations:create') =>
  negative(
    method,
    err(`redaction.${method} requires ${permission}`, 'permission-denied', 'redaction', permission),
  );
const unsupported = (method: string, sibling: string) =>
  negative(method, err(`no ${sibling} plugin`, 'unsupported'));
const rejections: Record<string, unknown[]> = {
  'marking-needs-create-authority': [denied('markSelection')],
  'without-a-selection-plugin': [unsupported('markSelection', 'selection')],
  'apply-refused': [
    negative(
      'apply',
      err('permission denied', 'operation-failed', 'redaction', null, engine('permission denied')),
    ),
  ],
  'apply-refusals-before-the-engine': [
    negative('apply', err('no matching pending marks', 'not-found')),
    negative('apply', err('no matching pending marks', 'not-found')),
    negative('applyAll', err('the document has no pages', 'not-ready')),
    negative(
      'apply',
      err(
        'redaction unsupported',
        'operation-failed',
        'redaction',
        null,
        engine('redaction unsupported', 'Unsupported'),
      ),
    ),
  ],
  'set-label': [
    negative('updateLabel', err('the target is not a redaction mark', 'not-found')),
    negative('updateLabel', err('the target is not a redaction mark', 'not-found')),
  ],
  'mark-area-and-page': [
    negative('markPage', err('page 9 is not in this document', 'not-found')),
    denied('markArea'),
    denied('markPage'),
  ],
  'marking-refusals': [
    unsupported('markMatches', 'search'),
    denied('markMatches'),
    unsupported('markSelection', 'selection'),
    denied('markSelection'),
  ],
  'apply-pages': [negative('applyPages', err('no pages given', 'invalid-input'))],
  'update-label-refused': [
    negative(
      'updateLabel',
      err('locked', 'operation-failed', 'annotation', null, engine('locked')),
    ),
  ],
  'mark-ownership-and-flags': [denied('updateLabel', 'annotations:update')],
  'close-during-apply': [
    negative('applyAll', err('document instance a#1 was closed', 'instance-closed')),
  ],
};
const expectedThrows: Record<string, unknown[]> = {
  'without-a-selection-plugin': [
    negative('activateTool', err("unknown tool 'redact'", 'not-found', 'interaction')),
  ],
};
const expectedReports: Record<string, unknown[]> = {
  'a-throwing-listener-is-isolated': [{ $error: { name: 'Error', message: 'listener broke' } }],
  'close-during-apply': [
    err('a state update after the instance closed (a#1); dropped', 'instance-closed'),
  ],
};
for (const c of scenarios.cases)
  test(`${c.name}: exact negative channels and no hidden stale calls`, async () => {
    const r = await run(c.name);
    assert.deepEqual(r.unmatched, []);
    const promiseMethods = new Map<number, string>();
    const thrown: unknown[] = [],
      rejected: unknown[] = [];
    for (const [i, t] of r.trace.entries()) {
      const method =
        c.steps[i]?.op === 'call'
          ? ((c.steps[i] as Extract<ScenarioCase['steps'][number], { op: 'call' }>).method ??
            '<ref>')
          : '<noncall>';
      if (t.outcome && 'promise' in t.outcome) promiseMethods.set(t.outcome.promise, method);
      if (t.outcome && 'threw' in t.outcome) thrown.push(negative(method, t.outcome.threw));
      for (const s of t.settled)
        if ('rejected' in s) rejected.push(negative(promiseMethods.get(s.promise)!, s.rejected));
    }
    assert.deepEqual(thrown, expectedThrows[c.name] ?? []);
    assert.deepEqual(rejected, rejections[c.name] ?? []);
    assert.deepEqual(
      r.trace.flatMap((t) => t.reports),
      expectedReports[c.name] ?? [],
    );
    assert.equal(
      r.trace.flatMap((t) => t.callbacks).filter((c) => c.fn.startsWith('console.')).length,
      0,
    );
    const issued = r.trace.flatMap((t) =>
      t.outcome && 'promise' in t.outcome ? [t.outcome.promise] : [],
    );
    const settled = r.trace.flatMap((t) => t.settled.map((s) => s.promise));
    assert.deepEqual(issued.sort(), settled.sort());
  });

test('pending projection keeps exact pages, refs, overlay labels and geometry', async () => {
  const t = (await run('pending-projection')).trace;
  assert.equal(value(t, 2), 'ready');
  assert.deepEqual(value(t, 3), [
    {
      ref: objRef(10),
      page: pr(1),
      pageIndex: 0,
      kind: 'area',
      bounds: { x: 90, y: 90, width: 120, height: 120 },
      overlayText: 'CONFIDENTIAL',
      repeat: false,
    },
    {
      ref: objRef(11),
      page: pr(1),
      pageIndex: 0,
      kind: 'text',
      bounds: { x: 50, y: 55, width: 150, height: 35 },
      overlayText: null,
      repeat: false,
    },
    {
      ref: baseRef(4, 2),
      page: pr(2),
      pageIndex: 1,
      kind: 'area',
      bounds: { x: 10, y: 460, width: 40, height: 40 },
      overlayText: null,
      repeat: false,
    },
  ]);
  assert.equal(value(t, 4), 3);
  const reads = (await run('pending-reads')).trace;
  assert.deepEqual(value(reads, 2), value<unknown[]>(t, 3)[0]);
  assert.deepEqual(value(reads, 3), value<unknown[]>(t, 3)[2]);
  assert.equal(value(reads, 4), null);
  assert.deepEqual(value(reads, 5), value(reads, 6));
  assert.deepEqual(value(reads, 7), []);
  assert.deepEqual(value(reads, 8), value(t, 3));
  assert.equal(value(reads, 10), 2);
  assert.equal(value(reads, 11), 0);
  assert.equal(value(reads, 12), 3);
});

test('pure read host fulfills current list queries stably and rejects every undeclared dependency', () => {
  const records = [redact(1), square(2), redact(3, { page: pr(2), ref: objRef(3, 2) })],
    pages = [layout(0, 1), layout(1, 2)];
  const port = annotationReadPort(records, pages);
  const query = { pages: [pr(2)] };
  assert.deepEqual(port.list(query), [records[2]]);
  assert.equal(port.list(query), port.list(query));
  assert.equal(port.list(), records);
  assert.throws(() => port.get(objRef(1)), { message: 'undeclared pure annotation port get' });
  const reads = pending(pages, records);
  assert.equal(reads.api.listPending(), reads.api.listPending());
  assert.equal(reads.api.listPending({ page: pr(1) }), reads.api.listPending({ page: pr(1) }));
  assert.equal(reads.api.getPendingCount(1), 1);
  assert.deepEqual(reads.api.listPending({ page: pr(99) }), []);
  assert.equal(reads.api.getPending(objRef(2)), null);
  assert.deepEqual(reads.api.estimateCollateral(), { count: 1, refs: [objRef(2)] });
  assert.deepEqual(reads.api.estimateCollateral([]), { count: 0, refs: [] });
});

test('selection marking preserves the exact selected cells on both pages and clears once', async () => {
  const t = (await run('mark-the-text-selection')).trace;
  const marks = resolved<{ marks: { ref: unknown; kind: string; bounds: unknown }[] }>(t, 5).marks;
  assert.deepEqual(
    marks.map((m) => m.ref),
    [objRef(10001), objRef(10002, 2)],
  );
  assert.deepEqual(
    marks.map((m) => m.kind),
    ['text', 'text'],
  );
  assert.equal(value(t, 6), false);
  assert.equal(value(t, 8), 2);
  assert.deepEqual(resolved(t, 9), { marks: [] });
  const calls = t.flatMap((t) => t.engine) as {
    path: string;
    args: [ChangeSet, { opId: string }];
  }[];
  const creates = calls
    .filter((c) => c.path === 'apply')
    .flatMap((c) => c.args[0].ops)
    .filter((o) => o.type === 'annotations.create');
  assert.deepEqual(
    creates.map((o) => redactDraft(o.data).quadPoints),
    [[quad(50, 50, 66, 12), quad(50, 70, 66, 12)], [quad(30, 20, 24, 12)]],
  );
  const denied = (await run('marking-needs-create-authority')).trace;
  assert.equal(value(denied, 5), false);
  assert.equal(value(denied, 7), true);
  assert.equal(
    denied.flatMap((t) => t.engine).filter((c) => (c as { path: string }).path === 'apply').length,
    0,
  );
});

test('search marking uses current ranges, skips empty geometry and preserves filtered hit order', async () => {
  const t = (await run('mark-search-matches')).trace;
  assert.deepEqual(
    resolved<{ marks: { ref: unknown }[] }>(t, 2).marks.map((m) => m.ref),
    [objRef(10001), objRef(10002, 2)],
  );
  assert.deepEqual(
    resolved<{ marks: { ref: unknown }[] }>(t, 3).marks.map((m) => m.ref),
    [objRef(10003, 2)],
  );
  const calls = t.flatMap((t) => t.engine) as { path: string; args: unknown[] }[];
  assert.deepEqual(
    calls.filter((c) => c.path === 'search.query').map((c) => c.args),
    [[{ text: 'the', snippets: true }], [{ text: 'the', snippets: true }]],
  );
  assert.equal(value<unknown[]>(t, 4).length, 3);
});

test('configured area and page creation use page space and overlay styling with exact pending events', async () => {
  const t = (await run('mark-area-and-page')).trace;
  assert.deepEqual(resolved<{ mark: { bounds: unknown } }>(t, 4).mark.bounds, {
    x: 10,
    y: 20,
    width: 100,
    height: 50,
  });
  assert.deepEqual(resolved<{ mark: { bounds: unknown } }>(t, 5).mark.bounds, {
    x: 0,
    y: 0,
    width: 400,
    height: 500,
  });
  assert.deepEqual(
    t
      .flatMap((t) => t.callbacks)
      .filter((c) => c.fn === 'pending')
      .map((c) => c.args),
    [[{ pages: [pr(1)] }], [{ pages: [pr(2)] }]],
  );
  const calls = t.flatMap((t) => t.engine) as { path: string; args: [ChangeSet] }[];
  const creates = calls
    .filter((c) => c.path === 'apply')
    .flatMap((c) => c.args[0].ops)
    .filter((o) => o.type === 'annotations.create');
  assert.deepEqual(
    creates.slice(0, 2).map((o) => ({
      fill: redactDraft(o.data).interiorColor,
      font: redactDraft(o.data).fontFamily,
      color: redactDraft(o.data).fontColor,
      size: redactDraft(o.data).fontSize,
    })),
    [
      { fill: '#ff0000', font: 'courier', color: '#00ff00', size: 0 },
      { fill: '#ff0000', font: 'courier', color: '#00ff00', size: 0 },
    ],
  );
  assert.equal(value(t, 9), false);
  assert.equal(creates.length, 3);
  const empty = (await run('an-overlay-without-values-adds-nothing')).trace;
  assert.equal(value(empty, 4), 1);
  const defaultCreate = (empty[3]!.engine as { path: string; args: [ChangeSet] }[]).find(
    (c) => c.path === 'apply',
  )!.args[0].ops[0]!;
  assert.equal(defaultCreate.type, 'annotations.create');
  assert.equal(redactDraft(defaultCreate.data).interiorColor, '#000000');
  assert.equal(redactDraft(defaultCreate.data).fontColor, '#ffffff');
});

test('labels carry current DA styling and null clears the label without losing repeat', async () => {
  const t = (await run('set-label')).trace;
  const calls = t.flatMap((t) => t.engine) as { path: string; args: [ChangeSet] }[];
  const updates = calls
    .filter((c) => c.path === 'apply')
    .flatMap((c) => c.args[0].ops)
    .filter((o) => o.type === 'annotations.update');
  assert.equal(updates.length, 4);
  for (const o of updates.slice(0, 3)) {
    assert.equal(redactPatch(o.patch).fontFamily, 'helvetica');
    assert.equal(redactPatch(o.patch).fontSize, 0);
    assert.equal(redactPatch(o.patch).fontColor, '#ffffff');
    assert.equal(redactPatch(o.patch).textAlign, 'center');
  }
  assert.equal(redactPatch(updates[1]!.patch).overlayText, null);
  assert.equal(redactPatch(updates[1]!.patch).repeat, true);
  assert.equal(redactPatch(updates[3]!.patch).fontFamily, 'courier');
  assert.equal(redactPatch(updates[3]!.patch).fontSize, 9);
  assert.deepEqual(resolved<{ mark: { overlayText: unknown; repeat: boolean } }>(t, 5).mark, {
    ref: objRef(10),
    page: pr(1),
    pageIndex: 0,
    kind: 'area',
    bounds: { x: 90, y: 90, width: 120, height: 120 },
    overlayText: null,
    repeat: true,
  });
});

test('refused label writes and deletes roll back without losing successful collateral removals', async () => {
  const t = (await run('update-label-refused')).trace;
  assert.equal(value<{ overlayText: string }>(t, 4).overlayText, 'CONFIDENTIAL');
  assert.equal(value<{ repeat: boolean }>(t, 6).repeat, true);
  assert.equal(t.flatMap((t) => t.callbacks).filter((c) => c.fn === 'pending').length, 1);
  const u = (await run('unmark-and-clear')).trace;
  assert.deepEqual(resolved(u, 3), {
    applied: [objRef(10)],
    skipped: [
      { ref: objRef(1), reason: 'not a pending redaction mark' },
      { ref: objRef(99), reason: 'not a pending redaction mark' },
    ],
    failed: [],
  });
  assert.deepEqual(resolved(u, 5), {
    applied: [baseRef(4, 2)],
    skipped: [],
    failed: [
      {
        ref: objRef(11),
        error: {
          code: 'operation-failed',
          message: 'locked',
          capability: 'annotation',
          permission: null,
        },
      },
    ],
  });
  assert.deepEqual(
    value<{ ref: unknown }[]>(u, 6).map((m) => m.ref),
    [objRef(11)],
  );
  assert.equal(u.flatMap((t) => t.callbacks).filter((c) => c.fn === 'pending').length, 2);
});

test('owner-scoped permissions and locked flags agree with actual writes and exact returned failures', async () => {
  const t = (await run('mark-ownership-and-flags')).trace;
  assert.deepEqual(
    [2, 3, 4, 5, 6, 7].map((i) => value(t, i)),
    [true, true, false, false, false, false],
  );
  const batch = resolved<{ applied: unknown[]; failed: unknown[]; skipped: unknown[] }>(t, 10);
  assert.deepEqual(batch, {
    applied: [objRef(60)],
    skipped: [],
    failed: [61, 62, 63].map((n) => ({
      ref: objRef(n),
      error: {
        code: 'permission-denied',
        message: 'redaction.unmark requires annotations:delete',
        capability: 'redaction',
        permission: 'annotations:delete',
      },
    })),
  });
  assert.deepEqual(
    value<{ ref: unknown }[]>(t, 11).map((m) => m.ref),
    [objRef(61), objRef(62), objRef(63)],
  );
  assert.equal(
    t.flatMap((t) => t.engine).filter((c) => (c as { path: string }).path === 'apply').length,
    2,
  );
});

test('applies bracket busy, settle with the confirmed result and deliver fact provenance before readback', async () => {
  const t = (await run('apply-named-marks')).trace;
  assert.equal(value(t, 4), true);
  assert.equal(value(t, 6), false);
  const expected = answer({ annotations: [objRef(10), baseRef(4, 2)] });
  assert.deepEqual(resolved(t, 5), expected);
  assert.deepEqual(value(t, 7), expected);
  const callbacks = t.flatMap((t) => t.callbacks).filter((c) => c.fn === 'applied');
  assert.equal(callbacks.length, 1);
  assert.deepEqual(callbacks[0]!.args, [
    {
      result: expected,
      origin: {
        kind: 'local',
        sessionId: 's1',
        sub: null,
        ts: 0,
        serverId: null,
        tx: { id: 'redaction-apply', index: 0, count: 1 },
      },
    },
  ]);
  const all = (await run('apply-all-pages')).trace;
  assert.deepEqual(value(all, 5), answer({ pages: [pr(1), pr(2)] }, 3));
  assert.deepEqual(value(all, 6), []);
  assert.deepEqual(
    all.flatMap((t) => t.callbacks).map((c) => c.fn),
    ['first', 'second'],
  );
});

test('apply refusal preserves confirmed history and marks; the current Unsupported service can recover', async () => {
  const t = (await run('apply-refused')).trace;
  assert.deepEqual(value(t, 5), answer({ pages: [pr(1), pr(2)] }, 1));
  assert.equal(value(t, 6), false);
  assert.deepEqual(
    value<{ ref: unknown }[]>(t, 7).map((m) => m.ref),
    [objRef(10)],
  );
  assert.equal(t.flatMap((t) => t.callbacks).filter((c) => c.fn === 'applied').length, 1);
  const p = (await run('apply-refusals-before-the-engine')).trace;
  const calls = p.flatMap((t) => t.engine) as { doc: string; path: string }[];
  assert.equal(calls.filter((c) => c.path === 'redaction.apply' && c.doc !== 'c').length, 0);
  assert.equal(calls.filter((c) => c.path === 'redaction.apply' && c.doc === 'c').length, 2);
  assert.equal(value(p, 10), 3);
  assert.equal(value(p, 11), false);
  assert.deepEqual(value(p, 13), answer({ pages: [pr(1), pr(2)] }, 0, 'recovered-apply'));
});

test('queued applies keep a stable call order and preserve an unaffected page', async () => {
  const t = (await run('apply-queue-and-unchanged-page')).trace;
  const calls = t.flatMap((t) => t.engine) as { path: string; args: unknown[] }[];
  assert.deepEqual(
    calls.filter((c) => c.path === 'redaction.apply').map((c) => c.args),
    [[{ annotations: [objRef(10)] }], [{ annotations: [objRef(11)] }]],
  );
  assert.deepEqual(
    value<{ ref: unknown }[]>(t, 7).map((m) => m.ref),
    [baseRef(4, 2)],
  );
  assert.deepEqual(value(t, 8), answer({ annotations: [objRef(11)] }, 0, 'second'));
  assert.equal(value(t, 9), false);
  const callbacks = t.flatMap((t) => t.callbacks).filter((c) => c.fn === 'applied');
  assert.equal(callbacks.length, 2);
});

test('local and remote applies both publish facts, unrelated events are ignored and close releases subscriptions', async () => {
  const t = (await run('remote-applies-arrive-as-events')).trace;
  const callbacks = t.flatMap((t) => t.callbacks).filter((c) => c.fn === 'applied');
  assert.equal(callbacks.length, 2);
  assert.deepEqual(value(t, 4), value(t, 6));
  assert.deepEqual(value(t, 8), answer({ pages: [pr(1), pr(2)] }, 4, 'remote-apply'));
  assert.deepEqual(value(t, 9), []);
  const c = (await run('close-during-apply')).trace;
  assert.equal(value(c, 4), true);
  assert.equal(value(c, 8), false);
  assert.equal(value(c, 9), null);
  assert.equal(c.flatMap((t) => t.callbacks).length, 0);
  const bad = (await run('a-throwing-listener-is-isolated')).trace;
  assert.deepEqual(
    bad.flatMap((t) => t.callbacks).map((c) => c.fn),
    ['bad', 'good'],
  );
  assert.deepEqual(resolved(bad, 4), answer({ pages: [pr(1), pr(2)] }));
});

const collateral: Record<string, unknown[]> = {
  'no-document': [],
  'no-pages': [],
  'no-marks': [],
  'rect-mark-overlaps': [objRef(2)],
  'touching-edges-do-not-count': [],
  'contained-and-containing': [objRef(2), objRef(3)],
  'sliver-overlap': [objRef(2), objRef(3)],
  'quads-replace-the-rect': [objRef(2), objRef(4)],
  'tilted-quad-box': [objRef(2), objRef(3)],
  'one-annotation-counts-once': [objRef(3)],
  'marks-are-never-collateral': [],
  'filter-by-id': [objRef(4)],
  'filter-matches-nothing': [],
  'empty-filter': [],
  'null-filter-is-all': [objRef(3)],
  'filter-by-nm-and-index': [objRef(4)],
  'pages-in-document-order': [objRef(4, 2), objRef(2)],
  'nan-geometry-never-intersects': [objRef(4)],
  'negative-and-fractional': [objRef(2)],
};
for (const c of pure.cases)
  test(`${c.name}: exact current pure outcome and immutable input`, () => {
    const before = canonical(c.args);
    const decode = (v: unknown): unknown =>
      Array.isArray(v)
        ? v.map(decode)
        : v && typeof v === 'object' && '$number' in v
          ? Number(v.$number)
          : v && typeof v === 'object'
            ? Object.fromEntries(Object.entries(v).map(([k, x]) => [k, decode(x)]))
            : v;
    const out = Reflect.apply(pure.functions[c.fn]!, undefined, decode(c.args) as unknown[]);
    if (c.fn === 'estimateCollateral')
      assert.deepEqual(out, { count: collateral[c.name]!.length, refs: collateral[c.name] });
    else if (c.fn === 'initialRedactionState')
      assert.deepEqual(out, { applying: false, lastResult: null });
    else if (c.fn === 'startApply')
      assert.deepEqual(out, { ...(c.args[0] as object), applying: true });
    else if (c.fn === 'finishApply')
      assert.deepEqual(out, { ...(c.args[0] as object), applying: false });
    else if (c.fn === 'setLastResult')
      assert.deepEqual(out, { ...(c.args[0] as object), lastResult: c.args[1] });
    else if (c.fn === 'markOf') {
      const dto = c.args[0] as Extract<Annotation, { subtype: 'redact' }>;
      assert.deepEqual(out, {
        ref: dto.ref,
        page: dto.page,
        pageIndex: 0,
        kind: dto.quadPoints.length ? 'text' : 'area',
        bounds: dto.rect,
        overlayText: dto.overlayText ?? null,
        repeat: dto.repeat ?? false,
      });
    } else assert.fail('every function needs an independent assertion');
    assert.deepEqual(canonical(c.args), before);
  });

import { AnnotationSchema } from '../../../packages/engine/core/src/annotation/kinds';
import * as model from '../../../packages/plugin/redaction/src/model';
test('all successful annotation answers match the actual authored operation fields, result cardinality and provenance', async () => {
  for (const c of scenarios.cases) {
    const r = await run(c.name);
    for (const [id, document] of Object.entries(c.engine?.documents ?? {})) {
      const responses = (document.responses ?? []).filter((s) => s.path === 'apply');
      let cursor = 0;
      const calls = (
        r.trace.flatMap((t) => t.engine) as {
          doc: string;
          path: string;
          args: [ChangeSet, { opId: string }];
        }[]
      ).filter((call) => call.doc === id && call.path === 'apply');
      for (const call of calls) {
        const response = responses[cursor++];
        assert.ok(response, `${c.name}: explicit apply answer`);
        if (response.error) continue;
        const value = response.value as ChangeResult;
        const ops = call.args[0].ops;
        assert.equal(value.items.length, ops.length, `${c.name}: result cardinality`);
        assert.equal(value.meta.opId, call.args[1].opId);
        assert.equal(response.events?.length, ops.length);
        for (const [i, op] of ops.entries()) {
          const item = value.items[i]!;
          assert.equal(item.type, op.type);
          assert.equal(item.meta.opId, value.meta.opId);
          const event: DocumentEvent = response.events![i]!;
          assert.ok('origin' in event);
          assert.deepEqual(event.origin.tx, { id: value.meta.opId, index: i, count: ops.length });
          if ('annotation' in item) {
            AnnotationSchema.parse(item.annotation);
            const entries = new Map(Object.entries(item.annotation));
            if (op.type === 'annotations.create')
              for (const [key, value] of Object.entries(op.data))
                assert.deepEqual(entries.get(key), value, `${c.name}: create ${key}`);
            if (op.type === 'annotations.update')
              for (const [key, value] of Object.entries(op.patch))
                assert.deepEqual(entries.get(key), value, `${c.name}: patch ${key}`);
          }
        }
      }
    }
  }
});
test('the actual named transitions preserve object identity on their no-op branches', () => {
  const idle = model.initialRedactionState(),
    busy = model.startApply(idle),
    done = model.setLastResult(idle, answer({ pages: [pr(1)] }));
  assert.equal(model.startApply(busy), busy);
  assert.equal(model.finishApply(idle), idle);
  assert.equal(model.setLastResult(done, done.lastResult!), done);
  assert.equal(model.startApply(done).lastResult, done.lastResult);
  assert.equal(model.finishApply({ ...done, applying: true }).lastResult, done.lastResult);
});

import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
test('every original redaction row maps exactly or has a source-evidenced retirement, with complete actual current inputs', async () => {
  const digest = (value: unknown) =>
    createHash('sha256').update(JSON.stringify(value)).digest('hex');
  const history = new URL('../historical/', import.meta.url);
  const originals = JSON.parse(
    await readFile(new URL('task-12b2b3/original-inputs.json', history), 'utf8'),
  ) as { source: string; file: string; name: string; input: unknown; inputSha256: string }[];
  const old = originals.filter((r) => r.source === 'redaction');
  assert.equal(old.length, 50);
  const accounting = JSON.parse(
    await readFile(new URL('task-12b2b3-ii/accounting.json', history), 'utf8'),
  ) as {
    original: number;
    current: number;
    mapped: number;
    wholeCaseRetirements: number;
    added: number;
    mappings: {
      before: { file: string; name: string; inputSha256: string };
      after: { file: string; name: string; inputSha256: string } | null;
      reason: string;
      evidence: string[];
      commits: string[];
    }[];
    additions: { file: string; name: string; inputSha256: string }[];
    sourceEvidence: { path: string; sha256: string; commit: string }[];
  };
  assert.deepEqual(
    [
      accounting.original,
      accounting.current,
      accounting.mapped,
      accounting.wholeCaseRetirements,
      accounting.added,
    ],
    [50, 57, 49, 1, 8],
  );
  const current = suites.flatMap((s) =>
    s.cases.map((c) => ({
      file: s.file,
      name: c.name,
      input: canonical(c),
      inputSha256: digest(canonical(c)),
    })),
  );
  assert.equal(new Set(current.map((r) => r.file + ':' + r.name)).size, 57);
  const saved = JSON.parse(
    await readFile(new URL('task-12b2b3-ii/current-inputs.json', history), 'utf8'),
  );
  assert.deepEqual(current, saved);
  for (const mapping of accounting.mappings) {
    const before = old.find(
      (r) => r.file === mapping.before.file && r.name === mapping.before.name,
    );
    assert.ok(before);
    assert.equal(before.inputSha256, mapping.before.inputSha256);
    assert.equal(digest(before.input), before.inputSha256);
    assert.ok(mapping.reason && mapping.evidence.length && mapping.commits.length);
    if (mapping.after) {
      const after = current.find(
        (r) => r.file === mapping.after!.file && r.name === mapping.after!.name,
      );
      assert.ok(after);
      assert.equal(after.inputSha256, mapping.after.inputSha256);
    } else assert.equal(mapping.before.name, 'unknown-action');
  }
  for (const addition of accounting.additions) {
    const actual = current.find((r) => r.file === addition.file && r.name === addition.name);
    assert.ok(actual);
    assert.equal(actual.inputSha256, addition.inputSha256);
    assert.equal(
      old.some((r) => r.name === addition.name),
      false,
    );
  }
  assert.equal(
    accounting.mappings.filter((m) => m.after).length + accounting.additions.length,
    current.length,
  );
  for (const evidence of accounting.sourceEvidence) {
    const actual = await readFile(new URL('../../../' + evidence.path, import.meta.url));
    assert.equal(createHash('sha256').update(actual).digest('hex'), evidence.sha256, evidence.path);
    assert.match(evidence.commit, /^[a-f0-9]{40}$/);
  }
});

test('initial permission twins expose current service grants and the composed tool follows actual activation', async () => {
  const t = (await run('initial-state-and-twins')).trace;
  assert.deepEqual(
    Array.from({ length: 12 }, (_, i) => value(t, i + 7)),
    [false, null, true, true, false, true, true, false, false, false, true, true],
  );
  const tool = (await run('the-redact-tool')).trace;
  assert.deepEqual(
    [2, 4, 11, 13].map((i) => value(tool, i)),
    ['pointer', 'redact', 'pointer', 'redact'],
  );
  assert.deepEqual(
    value<{ bounds: unknown; kind: string }[]>(tool, 9).map((m) => ({
      bounds: m.bounds,
      kind: m.kind,
    })),
    [{ bounds: { x: 300, y: 200, width: 100, height: 100 }, kind: 'area' }],
  );
  const noSelection = (await run('without-a-selection-plugin')).trace;
  assert.equal(value(noSelection, 4), 'pointer');
  const pages = (await run('apply-pages')).trace;
  assert.equal(value(pages, 3), false);
  assert.deepEqual(value(pages, 5), answer({ pages: [pr(2)] }, 1));
  const collateral = (await run('estimate-collateral')).trace;
  assert.deepEqual(
    [2, 3, 4, 5, 6, 7].map((i) => value(collateral, i)),
    [
      { count: 3, refs: [objRef(1), objRef(2), objRef(3)] },
      { count: 2, refs: [objRef(1), objRef(2)] },
      { count: 1, refs: [objRef(3)] },
      { count: 0, refs: [] },
      { count: 0, refs: [] },
      { count: 0, refs: [] },
    ],
  );
});
