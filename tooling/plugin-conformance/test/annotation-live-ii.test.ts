import assert from 'node:assert/strict';
import { test } from 'node:test';
import { runScenario, type ScenarioCase, type TraceEntry } from '../src/scenario';
import { suites as comments } from '../src/suites/annotation-comments';
import { suites as grouping } from '../src/suites/annotation-grouping';
import { suites as hover } from '../src/suites/annotation-hover';
import { suites as markup } from '../src/suites/annotation-markup';
import { suites as tools } from '../src/suites/annotation-tools';
import { expectedRefusals } from './annotation-live-ii-refusals';
const suites = [...comments, ...grouping, ...hover, ...markup, ...tools];
const runs = new Map<string, { input: ScenarioCase; trace: TraceEntry[] }>();
function messages(value: unknown): string[] {
  if (Array.isArray(value)) return value.flatMap(messages);
  if (!value || typeof value !== 'object') return [];
  const record = value as Record<string, unknown>;
  if ('$error' in record) return [(record.$error as { message: string }).message];
  if (typeof record.code === 'string' && typeof record.message === 'string')
    return [record.message];
  return Object.entries(record).flatMap(([key, value]) => [
    ...(key === 'failed' && Array.isArray(value) && value.length ? [`failed:${value.length}`] : []),
    ...(key === 'diagnostics' && Array.isArray(value) && value.length
      ? value.map((x) => `diagnostic:${JSON.stringify(x)}`)
      : messages(value)),
  ]);
}
export function diagnostics(trace: TraceEntry[]): [number, string, string[]][] {
  return trace.flatMap((entry, i) =>
    (['outcome', 'settled', 'reports', 'callbacks'] as const).flatMap((channel) => {
      const found = messages(entry[channel]);
      for (const cb of channel === 'callbacks' ? entry.callbacks : [])
        if (cb.fn.startsWith('console.') && messages(cb.args).length === 0)
          found.push(`${cb.fn}:${JSON.stringify(cb.args)}`);
      return found.length ? [[i, channel, found] as [number, string, string[]]] : [];
    }),
  );
}
test('every current remaining live case has only exact named refusals and no unmatched calls', async (t) => {
  await new Promise<void>((resolve) => setImmediate(resolve));
  const unexpected: unknown[][] = [],
    savedError = console.error,
    savedWarn = console.warn;
  console.error = (...args: unknown[]) => unexpected.push(args);
  console.warn = (...args: unknown[]) => unexpected.push(args);
  try {
    for (const suite of suites) {
      assert.equal(suite.kind, 'scenario');
      if (suite.kind !== 'scenario') continue;
      for (const input of suite.cases)
        await t.test(input.name, async () => {
          assert.ok(!runs.has(input.name), `duplicate ${input.name}`);
          const before = {
            entropy: Object.getOwnPropertyDescriptor(globalThis.crypto, 'getRandomValues'),
            prototype: Object.getPrototypeOf(globalThis.crypto),
            error: console.error,
            warn: console.warn,
          };
          const result = await runScenario(input);
          assert.deepEqual(
            Object.getOwnPropertyDescriptor(globalThis.crypto, 'getRandomValues'),
            before.entropy,
            input.name,
          );
          assert.equal(Object.getPrototypeOf(globalThis.crypto), before.prototype, input.name);
          assert.equal(console.error, before.error, input.name);
          assert.equal(console.warn, before.warn, input.name);
          runs.set(input.name, { input, trace: result.trace });
          assert.deepEqual(result.unmatched, [], input.name);
          assert.deepEqual(
            diagnostics(result.trace),
            expectedRefusals[input.name] ?? [],
            input.name,
          );
        });
    }
    assert.deepEqual(unexpected, []);
  } finally {
    console.error = savedError;
    console.warn = savedWarn;
  }
});
import { AnnotationSchema } from '../../../packages/engine/core/src/annotation/kinds';
import {
  annotationOfDraft,
  type Annotation,
  type AnnotationDraft,
  type ChangeOp,
  type ChangeResult,
} from '@embedpdf/engine-core/runtime';
import { annotationAfter } from '../../../packages/core/annotation/src/record';
function sameNumbers(actual: unknown, expected: unknown, path = ''): void {
  if (typeof actual === 'number' && typeof expected === 'number') {
    if (Number.isNaN(actual) && Number.isNaN(expected)) return;
    assert.ok(
      Math.abs(actual - expected) < 1e-8 * Math.max(1, Math.abs(expected)),
      `${path}: ${actual} != ${expected}`,
    );
    return;
  }
  if (Array.isArray(actual) && Array.isArray(expected)) {
    assert.equal(actual.length, expected.length, path);
    actual.forEach((x, i) => sameNumbers(x, expected[i], `${path}[${i}]`));
    return;
  }
  if (actual && expected && typeof actual === 'object' && typeof expected === 'object') {
    const left = actual as Record<string, unknown>,
      right = expected as Record<string, unknown>;
    assert.deepEqual(Object.keys(left).sort(), Object.keys(right).sort(), path);
    for (const key of Object.keys(right)) sameNumbers(left[key], right[key], `${path}.${key}`);
    return;
  }
  assert.deepEqual(actual, expected, path);
}
function result(name: string) {
  const value = runs.get(name);
  assert.ok(value, name);
  return value;
}
function read<T = unknown>(name: string, method: string, which = -1): T {
  const run = result(name),
    indexes = run.input.steps.flatMap((step, i) =>
      step.op === 'call' && step.method === method ? [i] : [],
    );
  const index = indexes.at(which);
  assert.notEqual(index, undefined, `${name}:${method}`);
  const outcome = run.trace[index!]!.outcome;
  assert.ok(outcome);
  if ('returned' in outcome) return outcome.returned as T;
  assert.ok('promise' in outcome, `${name}:${method} threw`);
  const settled = run.trace
    .flatMap((entry) => entry.settled)
    .find((entry) => entry.promise === outcome.promise);
  assert.ok(settled && 'resolved' in settled, `${name}:${method} unresolved/rejected`);
  return settled.resolved as T;
}
function engineCalls(trace: TraceEntry[]) {
  return trace
    .flatMap((entry) => entry.engine)
    .filter(
      (entry): entry is { doc: string; path: string; args: unknown[] } =>
        !!entry && typeof entry === 'object' && 'path' in entry && 'args' in entry,
    );
}
function ops(name: string): ChangeOp[][] {
  return engineCalls(result(name).trace).flatMap((entry) =>
    entry.path === 'apply' ? [(entry.args[0] as { ops: ChangeOp[] }).ops] : [],
  );
}
test('every owned mutation has exact current ops, result kind, refs, schema fields and pre-settlement event provenance', () => {
  let count = 0;
  for (const [name, run] of runs) {
    const documents = run.input.engine!.documents!;
    const cursors = new Map<string, number>();
    const records = new Map<string, Annotation>();
    for (const [document, spec] of Object.entries(documents))
      for (const response of spec.responses ?? [])
        if (response.path === 'annotations.list')
          for (const annotation of (response.value as { annotations: Annotation[] }).annotations)
            records.set(document + JSON.stringify(annotation.ref), annotation);
    for (const [document, spec] of Object.entries(documents))
      for (const response of spec.responses ?? [])
        if (response.path === 'page.annotations.list')
          for (const annotation of (response.value as { annotations: Annotation[] }).annotations)
            if (!records.has(document + JSON.stringify(annotation.ref)))
              records.set(document + JSON.stringify(annotation.ref), annotation);
    for (const entry of engineCalls(run.trace)) {
      if (entry.path !== 'apply') continue;
      count++;
      const document = entry.doc!,
        index = cursors.get(document) ?? 0;
      cursors.set(document, index + 1);
      const response = documents[document]!.responses!.filter(
        (response) => response.path === 'apply',
      )[index];
      assert.ok(response, `${name}:${index}`);
      if (response.error || (!response.value && response.defer)) continue;
      const args = entry.args as [{ ops: ChangeOp[] }, { opId: string }],
        answer = response.value as ChangeResult;
      assert.equal(answer.meta.opId, args[1].opId, name);
      assert.equal(answer.items.length, args[0].ops.length, name);
      assert.equal(response.events!.length, answer.items.length, name);
      for (const [i, op] of args[0].ops.entries()) {
        const item = answer.items[i]!,
          event = response.events![i]!;
        assert.equal(item.type, op.type, `${name}:${index} op kind`);
        assert.ok('origin' in event && 'meta' in event, name);
        assert.deepEqual(
          event.origin.tx,
          { id: args[1].opId, index: i, count: answer.items.length },
          name,
        );
        assert.equal(event.origin.kind, 'local', name);
        assert.deepEqual(event.meta, item.meta, name);
        if (!('annotation' in item)) continue;
        const parsed = AnnotationSchema.safeParse(item.annotation);
        assert.ok(parsed.success, `${name}: ${JSON.stringify(parsed.error)}`);
        assert.deepEqual(parsed.data, item.annotation, `${name}: undeclared read fields`);
        if (op.type === 'annotations.create') {
          assert.deepEqual(
            item.annotation.ref,
            { kind: 'objectNumber', page: op.page, objectNumber: op.objectNumber },
            name,
          );
          assert.ok(typeof op.objectNumber === 'number', name);
          const day = op.objectNumber - 10001 + 7;
          const at = `2024-01-${String(day).padStart(2, '0')}T00:00:00Z`;
          const expected = annotationOfDraft(op.data, {
            ref: item.annotation.ref,
            ...(name === 'reply-edit-status-marked'
              ? {
                  attribution: {
                    userId: 'me',
                    createdBy: 'me',
                    modifiedBy: 'me',
                    createdAt: at,
                    modifiedAt: at,
                  },
                }
              : {}),
          });
          const omit = (record: Annotation) =>
            Object.fromEntries(
              Object.entries(record).filter(
                ([key]) =>
                  !['rect', 'file', 'nm', 'hasAppearance', 'appearanceState'].includes(key),
              ),
            );
          sameNumbers(omit(item.annotation), omit(expected), `${name}: create data`);
          if (op.data.subtype === 'stamp') assert.ok(op.resources?.appearance, name);
          if (op.data.subtype === 'file-attachment') assert.ok(op.resources?.file, name);
        }
        if (op.type === 'annotations.update') {
          assert.deepEqual(item.annotation.ref, op.ref, `${name}: update ref`);
          const before = records.get(document + JSON.stringify(op.ref));
          assert.ok(before, `${name}: prior update record`);
          sameNumbers(
            annotationAfter(before, op.patch),
            item.annotation,
            `${name}: patch readback`,
          );
        }
        records.set(document + JSON.stringify(item.annotation.ref), item.annotation);
      }
    }
    for (const [document, index] of cursors)
      assert.equal(
        documents[document]!.responses!.filter((response) => response.path === 'apply').length,
        index,
        `${name}:${document} unused answer`,
      );
  }
  assert.ok(count > 70, `only ${count} writes`);
});
import type { CommentThread } from '@embedpdf/engine-core/runtime';
import type { AnnotationState } from '../../../packages/plugin/annotation/src/model';
import { objRef, pr } from '../src/suites/annotation-live-fixtures';
const numbers = (annotations: readonly Annotation[]) =>
  annotations.map((annotation) => {
    assert.equal(annotation.ref.kind, 'objectNumber');
    return annotation.ref.kind === 'objectNumber' ? annotation.ref.objectNumber : -1;
  });
function session(name: string, index = -1) {
  const state = result(name).trace.at(index)?.state as { plugins: Record<string, AnnotationState> };
  assert.ok(state?.plugins['annotation::a'], name);
  return state.plugins['annotation::a']!.session;
}
function methodSessions(name: string, method: string) {
  const run = result(name);
  return run.input.steps.flatMap((step, i) =>
    step.op === 'call' && step.method === method ? [session(name, i)] : [],
  );
}
test('threads retain reading order, complete member resolution and review identity', () => {
  const name = 'threads-and-members',
    threads = read<CommentThread[]>(name, 'comments.listThreads');
  assert.deepEqual(numbers(threads.map((thread) => thread.root)), [2, 3, 1, 8]);
  for (let i = 0; i < 3; i++)
    assert.deepEqual(read<CommentThread>(name, 'comments.getThread', i).root.ref, objRef(1));
  assert.equal(read(name, 'comments.getThread'), null);
  const root = threads[2]!;
  assert.deepEqual(numbers(root.replies), [4]);
  assert.deepEqual(numbers(root.groupedParts), [5]);
  assert.deepEqual(root.review.mine?.ref, objRef(6));
  assert.equal(root.review.byReviewer.them?.state, 'rejected');
  const lost = read<CommentThread[]>('threads-off-the-layout', 'comments.listThreads');
  assert.deepEqual(numbers(lost.map((thread) => thread.root)).slice(-2), [4, 5]);
  assert.equal(read('threads-and-members', 'getStatus'), 'ready');
});
test('successful replies, text, reviewer chains and marked states carry current fields and announce exactly once', () => {
  const name = 'reply-edit-status-marked',
    thread = read<CommentThread>(name, 'comments.getThread');
  assert.deepEqual(numbers(thread.replies), [4, 10001]);
  assert.equal(thread.replies[0]!.contents, 'edited');
  assert.equal(thread.replies[1]!.contents, 'thanks');
  assert.equal(thread.replies[1]!.noZoom, true);
  assert.equal(thread.replies[1]!.noRotate, true);
  assert.equal(thread.review.mine?.state, 'completed');
  assert.deepEqual(thread.review.mine?.ref, objRef(10002));
  assert.deepEqual(thread.review.markedBy, []);
  assert.deepEqual(read<CommentThread>(name, 'comments.getThread', 0).review.markedBy, ['me']);
  assert.equal(thread.review.mine?.at, '2024-01-08T00:00:00Z');
  const writes = ops(name);
  assert.equal(writes.length, 5);
  const statuses = writes.slice(2).flat();
  assert.equal(statuses.length, 3);
  assert.ok(statuses.every((op) => op.type === 'annotations.create'));
  const [review, marked, unmarked] = statuses as Extract<
    ChangeOp,
    { type: 'annotations.create' }
  >[];
  assert.ok(
    review?.data.subtype === 'text' &&
      marked?.data.subtype === 'text' &&
      unmarked?.data.subtype === 'text',
  );
  assert.deepEqual(review!.data.reply, { to: objRef(6) });
  assert.equal(review!.data.state, 'completed');
  assert.deepEqual(marked!.data.reply, { to: objRef(1) });
  assert.equal(marked!.data.state, 'marked');
  assert.equal(unmarked!.data.state, 'unmarked');
  const callbacks = result(name)
    .trace.flatMap((entry) => entry.callbacks)
    .filter((entry) => entry.fn === 'threadChanged');
  assert.deepEqual(
    callbacks.map((entry) => (entry.args[0] as { change: string }).change),
    ['reply', 'text', 'status', 'marked', 'marked'],
  );
  assert.deepEqual(ops('status-without-a-previous-one')[0]![0]!.type, 'annotations.create');
  const status = ops('status-without-a-previous-one')[0]![0]!;
  assert.ok(status.type === 'annotations.create');
  assert.deepEqual(status.data.reply, { to: objRef(1) });
});
test('thread deletion preflights every member and uses one successful atomic root delete with full changed refs', () => {
  const name = 'remove-and-remove-thread-atomically',
    writes = ops(name);
  assert.deepEqual(
    writes.map((ops) => ops.map((op) => op.type)),
    [['annotations.delete'], ['annotations.delete'], ['annotations.delete']],
  );
  assert.deepEqual(writes[1]![0], { type: 'annotations.delete', ref: objRef(15) });
  const remaining = read<Annotation[]>(name, 'list');
  assert.ok(!numbers(remaining).includes(15) && !numbers(remaining).includes(16));
  assert.ok([1, 4, 5, 6, 7, 10, 11, 12, 13, 14].every((n) => numbers(remaining).includes(n)));
  const result1 = read<{ deleted: unknown[]; failed: unknown[] }>(name, 'comments.deleteThread', 0);
  assert.equal(result1.deleted.length, 0);
  assert.equal(result1.failed.length, 1);
  const successful = read<{ deleted: unknown[]; failed: unknown[] }>(
    name,
    'comments.deleteThread',
    2,
  );
  assert.deepEqual(successful.deleted, [objRef(16), objRef(15)]);
  assert.deepEqual(successful.failed, []);
  const refused = read<{ deleted: unknown[]; failed: unknown[] }>(name, 'comments.deleteThread', 3);
  assert.deepEqual(refused.deleted, []);
  assert.equal(refused.failed.length, 1);
});
test('grouping preserves subordinate refs and atomic group, ungroup and replacement outcomes', () => {
  assert.deepEqual(
    read<Annotation[]>('a-pair-groups', 'selection.list').map((annotation) => annotation.reply),
    [null, { to: objRef(1), type: 'group' }],
  );
  assert.deepEqual(
    ops('group-and-ungroup').map((group) => group.length),
    [2, 2],
  );
  assert.ok(
    read<Annotation[]>('group-and-ungroup', 'list').every(
      (annotation) => annotation.reply === null,
    ),
  );
  const pair = read<Annotation[]>('replace-text-creates-a-group', 'list');
  assert.deepEqual(
    pair.map((annotation) => annotation.subtype),
    ['caret', 'strikeout'],
  );
  assert.deepEqual(pair[1]!.reply, { to: objRef(10001), type: 'group' });
  assert.deepEqual(pair[0]!.rect, { x: 87, y: 51, width: 6, height: 6.25 });
  assert.deepEqual(
    numbers(read<Annotation[]>('replace-text-creates-a-group', 'selection.list')),
    [10001, 10002],
  );
  assert.deepEqual(
    ops('replace-text-creates-a-group').map((group) => group.length),
    [2],
  );
  assert.deepEqual(read('replace-text-refusal-is-atomic', 'list'), []);
  assert.deepEqual(session('replace-text-refusal-is-atomic').selected, []);
  assert.ok(
    ops('replace-text-refusal-is-atomic').every(
      (group) => group.length === 2 && group.every((op) => op.type === 'annotations.create'),
    ),
  );
  assert.equal(read('group-refused-per-record', 'selection.canGroup', 0), false);
  assert.equal(read('group-refused-per-record', 'selection.canGroup'), true);
});
test('hover diffs once and the real actions pump executes only eligible enter and exit trees', () => {
  const name = 'hover-state-on-change-only',
    states = methodSessions(name, 'hoverAt');
  assert.deepEqual(
    states.map((state) => state.hovered),
    ['obj:1', 'obj:1', 'obj:2', null, null],
  );
  const run = result(name),
    second = run.input.steps.findIndex(
      (step, i) => i > 2 && step.op === 'call' && step.method === 'hoverAt',
    );
  assert.deepEqual(run.trace[second]!.stateChanges, []);
  const pump = result('hover-feeds-the-actions-pump');
  const callbacks = pump.trace
    .flatMap((entry) => entry.callbacks)
    .filter((callback) => callback.fn === 'exec.named');
  assert.deepEqual(
    callbacks.map((callback) => (callback.args[0] as { name: string }).name),
    ['enter-1', 'exit-1', 'enter-2', 'exit-6', 'enter-2'],
  );
  for (const callback of callbacks) {
    const context = callback.args[1] as {
      origin: string;
      source: { kind: string };
      event: { scope: string };
    };
    assert.equal(context.origin, 'hover');
    assert.equal(context.source.kind, 'annotation');
    assert.equal(context.event.scope, 'annotation');
  }
  const events = pump.trace
    .flatMap((entry) => entry.callbacks)
    .filter((callback) => callback.fn === 'actions.executed');
  assert.equal(events.length, 5);
  for (const event of events)
    assert.deepEqual(
      (event.args[0] as { result: { diagnostics: unknown[] } }).result.diagnostics,
      [],
    );
  assert.ok(
    pump.trace
      .slice(-2)
      .every((entry) => entry.callbacks.every((callback) => callback.fn !== 'exec.named')),
  );
});
test('markup commits real glyph quads, end anchors, per-page groups and live preview visibility', () => {
  const highlights = read<Annotation[]>('highlight-by-drag', 'list');
  assert.deepEqual(
    highlights.map((annotation) => annotation.subtype),
    ['highlight', 'squiggly'],
  );
  assert.equal(highlights[0]!.subtype, 'highlight');
  if (highlights[0]!.subtype === 'highlight')
    assert.deepEqual(highlights[0]!.quadPoints, [
      {
        upperLeft: { x: 50, y: 50 },
        upperRight: { x: 116, y: 50 },
        lowerLeft: { x: 50, y: 62 },
        lowerRight: { x: 116, y: 62 },
      },
      {
        upperLeft: { x: 50, y: 70 },
        upperRight: { x: 86, y: 70 },
        lowerLeft: { x: 50, y: 82 },
        lowerRight: { x: 86, y: 82 },
      },
    ]);
  for (const name of [
    'replace-text-anchors-at-the-end-glyph',
    'create-from-selection-anchors-at-the-end-glyph',
  ]) {
    const pair = read<Annotation[]>(name, 'list');
    assert.equal(pair.length, 2);
    assert.deepEqual(pair[0]!.rect, { x: 65, y: 56, width: 6, height: 6.25 });
    assert.deepEqual(pair[1]!.reply, { to: objRef(10001), type: 'group' });
  }
  const across = read<Annotation[]>('replace-text-across-pages', 'list');
  assert.deepEqual(
    across.map((annotation) => annotation.page.objectNumber),
    [1, 1, 2, 2],
  );
  assert.deepEqual(
    ops('replace-text-across-pages').map((group) => group.length),
    [2, 2],
  );
  assert.equal(read('markup-from-selection', 'applyToolToSelection', 0), false);
  assert.equal(read('markup-from-selection', 'applyToolToSelection', 1), true);
  assert.equal(read('markup-from-selection', 'hasSelection'), false);
  assert.deepEqual(read('create-from-selection-text-edits', 'createFromSelection', 0), {
    annotations: [],
  });
  assert.equal(read('create-from-selection-text-edits', 'hasSelection'), true);
  assert.deepEqual(
    result('preview-follows-the-tool').input.steps.flatMap((step, i) =>
      step.op === 'call' && step.method === 'isHighlightVisible'
        ? [(result('preview-follows-the-tool').trace[i]!.outcome as { returned: boolean }).returned]
        : [],
    ),
    [false, false, true, false],
  );
  assert.equal(ops('preview-follows-the-tool').length, 0);
  assert.equal(session('preview-follows-the-tool').preview, null);
});
test('tool gestures create the intended shapes, preserve home-page geometry and separate typing dismissal from drawing', () => {
  const drawn = read<Annotation[]>('draw-shapes', 'list');
  assert.deepEqual(
    drawn.map((annotation) => annotation.subtype),
    ['square', 'square', 'free-text', 'line'],
  );
  assert.ok(
    drawn[0]!.subtype === 'square' &&
      drawn[1]!.subtype === 'square' &&
      drawn[2]!.subtype === 'free-text' &&
      drawn[3]!.subtype === 'line',
  );
  assert.deepEqual(drawn[0]!.box, { x: 100, y: 100, width: 50, height: 30 });
  assert.deepEqual(drawn[1]!.box, { x: 260, y: 270, width: 80, height: 60 });
  assert.equal(drawn[2]!.rotation, 270);
  assert.deepEqual(drawn[3]!.linePoints, { start: { x: 10, y: 50 }, end: { x: 90, y: 50 } });
  assert.equal(read('draw-shapes', 'text.getEditing'), null);
  assert.equal(read('draw-shapes', 'draft.get'), null);
  assert.deepEqual(read('draw-without-authority', 'list'), []);
  assert.equal(ops('draw-without-authority').length, 0);
  const moved = read<Annotation[]>('edit-gestures', 'list')[0]!;
  assert.ok(moved.subtype === 'square');
  assert.deepEqual(moved.box, { x: 125, y: 112, width: 100, height: 100 });
  assert.deepEqual(read('edit-gestures', 'selection.list'), []);
  assert.equal(read('edit-gestures', 'draft.get'), null);
  const firstRead = read<unknown>('edit-gestures', 'draft.get', 0);
  assert.equal(firstRead, null);
});
test('ink buffering, page switches, zero-window strokes and cancel have concrete current results', () => {
  const ink = read<Annotation[]>('ink-strokes-group', 'list');
  assert.equal(ink.length, 4);
  assert.ok(ink.every((annotation) => annotation.subtype === 'ink'));
  assert.deepEqual(
    ink.map((annotation) => annotation.page.objectNumber),
    [1, 1, 2, 1],
  );
  const [grouped, , page2, highlighter] = ink as Extract<Annotation, { subtype: 'ink' }>[];
  assert.deepEqual(grouped!.inkList, [
    [
      { x: 10, y: 10 },
      { x: 20, y: 15 },
    ],
    [
      { x: 40, y: 40 },
      { x: 50, y: 45 },
    ],
  ]);
  assert.deepEqual(page2!.inkList, [
    [
      { x: 5, y: 5 },
      { x: 60, y: 70 },
    ],
  ]);
  assert.equal(highlighter!.color, '#ffcd45');
  assert.equal(highlighter!.blendMode, 'multiply');
  assert.equal(read('ink-strokes-group', 'draft.get'), null);
  assert.equal(ops('ink-strokes-group').length, 4);
  assert.deepEqual(read('ink-cancel-inside-the-grouping-window', 'list'), []);
  assert.equal(ops('ink-cancel-inside-the-grouping-window').length, 0);
  assert.equal(read<Annotation[]>('hover-drives-only-a-multi-click-draft', 'list').length, 2);
  assert.equal(ops('hover-drives-only-a-multi-click-draft').length, 2);
});
test('polygon finish and callout clicks produce distinct current records and cancel clears the poly draft', () => {
  const name = 'poly-and-callout-clicks',
    annotations = read<Annotation[]>(name, 'list');
  assert.deepEqual(
    annotations.map((annotation) => annotation.subtype),
    ['polygon', 'free-text'],
  );
  assert.ok(annotations[0]!.subtype === 'polygon' && annotations[1]!.subtype === 'free-text');
  assert.deepEqual(annotations[0]!.vertices, [
    { x: 10, y: 10 },
    { x: 60, y: 20 },
    { x: 40, y: 60 },
  ]);
  assert.equal(annotations[1]!.intent, 'free-text-callout');
  assert.deepEqual(annotations[1]!.calloutLine, [
    { x: 300, y: 300 },
    { x: 340, y: 330 },
    { x: 410, y: 400 },
  ]);
  assert.equal(read(name, 'draft.get', 0), null);
  assert.equal(ops(name).length, 2);
  const drag = methodSessions('marquee-starts-at-exactly-4px', 'listChromeNodes');
  assert.ok(drag.every((state) => state.draft?.kind === 'marquee'));
  const low = result('handlers-outrank-a-lower-handler')
    .trace.flatMap((entry) => entry.callbacks)
    .filter((entry) => entry.fn === 'low.onDown');
  assert.deepEqual(low, []);
});
test('links retain one stable child per parent, current URI actions, atomic geometry sync and refused rollback', () => {
  assert.deepEqual(read('attached-links', 'links.get', 1), { kind: 'uri', uri: 'https://a' });
  assert.deepEqual(read('attached-links', 'links.get', 2), { kind: 'uri', uri: 'https://b' });
  assert.equal(read('attached-links', 'links.get', 3), null);
  assert.deepEqual(numbers(read<Annotation[]>('attached-links', 'list')), [1, 9, 10]);
  assert.equal(
    read('rapid-link-sets-use-pending-identity', 'links.get') &&
      ops('rapid-link-sets-use-pending-identity').length,
    2,
  );
  assert.deepEqual(read('rapid-link-sets-use-pending-identity', 'links.get'), {
    kind: 'uri',
    uri: 'https://2',
  });
  const queued = read<Annotation[]>('link-updates-queue-across-parents', 'list');
  assert.deepEqual(numbers(queued), [1, 2, 10001, 10002]);
  assert.deepEqual(read('link-updates-queue-across-parents', 'links.get', 0), {
    kind: 'uri',
    uri: 'https://4',
  });
  assert.deepEqual(read('link-updates-queue-across-parents', 'links.get', 1), {
    kind: 'uri',
    uri: 'https://b2',
  });
  const geometry = read<Annotation[]>('links-follow-geometry-and-props', 'list');
  const parent = geometry.find(
    (annotation) => annotation.ref.kind === 'objectNumber' && annotation.ref.objectNumber === 1,
  )!;
  const child = geometry.find(
    (annotation) => annotation.ref.kind === 'objectNumber' && annotation.ref.objectNumber === 20,
  )!;
  assert.deepEqual(parent.rect, { x: 110, y: 100, width: 100, height: 100 });
  assert.deepEqual(child.rect, parent.rect);
  assert.deepEqual(
    ops('links-follow-geometry-and-props').map((group) => group.length),
    [2, 2, 1],
  );
  const markupLinks = geometry.filter(
    (annotation) =>
      annotation.reply?.to.kind === 'objectNumber' && annotation.reply.to.objectNumber === 2,
  );
  assert.equal(markupLinks.length, 2);
  assert.ok(
    !geometry.some(
      (annotation) =>
        annotation.reply?.to.kind === 'objectNumber' && annotation.reply.to.objectNumber === 3,
    ),
  );
});
test('script results assert successful lowering, unloaded staging failure, explicit invalid inputs and stop-after-refusal', () => {
  const name = 'script-commits',
    first = read<{ results: unknown[] }>(name, 'commitScriptEffects', 0),
    second = read<{ results: unknown[] }>(name, 'commitScriptEffects', 1);
  assert.deepEqual(first.results, [
    { annotObjectNumber: 1, status: 'applied' },
    { annotObjectNumber: 2, status: 'applied' },
    { annotObjectNumber: 40, status: 'failed', error: 'no annotation obj:40' },
    { annotObjectNumber: 1, status: 'skipped' },
  ]);
  assert.deepEqual(second.results, [
    { annotObjectNumber: 41, status: 'failed', error: 'annotation not resolved (page not loaded)' },
    { annotObjectNumber: 42, status: 'failed', error: 'annotation not resolved (page not loaded)' },
    { annotObjectNumber: 44, status: 'failed', error: 'annotation not resolved (page not loaded)' },
    {
      annotObjectNumber: 3,
      status: 'failed',
      error: 'transparent stroke colours are not supported',
    },
    { annotObjectNumber: 3, status: 'failed', error: 'intended script refusal' },
    { annotObjectNumber: 2, status: 'skipped' },
    { annotObjectNumber: 1, status: 'skipped' },
  ]);
  const [square, circle] = read<Annotation[]>(name, 'list');
  assert.ok(square?.subtype === 'square' && circle?.subtype === 'circle');
  assert.equal(square.color, '#ff0080');
  assert.equal(square.hidden, true);
  assert.deepEqual(square.box, { x: 10, y: 760, width: 20, height: 20 });
  assert.deepEqual(square.rect, square.box);
  assert.equal(circle.color, '#808080');
  assert.equal(circle.interiorColor, '#80664d');
  assert.equal(ops(name).length, 3);
  assert.deepEqual(read('attachment-download', 'downloadResource', 0), { $bytes: 'AQID' });
});
test('malformed binary sources fail only in their isolated named case and recover with valid arm, preview and disarm', () => {
  const name = 'malformed-stamp-source-inputs';
  assert.equal(read(name, 'stamps.isArmed', 0), false);
  assert.equal(read(name, 'stamps.isArmed', 1), true);
  assert.equal(read(name, 'stamps.isArmed', 2), false);
  const preview = read<{ image: unknown } | null>(name, 'renderArmedStampPreview');
  assert.ok(preview);
  assert.equal(read(name, 'draft.get'), null);
  assert.deepEqual(read(name, 'selection.list'), []);
  assert.equal(read(name, 'text.getEditing'), null);
  assert.equal(ops(name).length, 0);
  assert.deepEqual(session(name).selected, []);
  assert.equal(session(name).draft, null);
  const errors = result(name)
    .trace.flatMap((entry) => entry.settled)
    .flatMap((entry) => (entry.rejected ? [entry.rejected] : []));
  assert.equal(errors.length, 6);
  for (const error of errors)
    assert.equal((error as { $error: { name: string } }).$error.name, 'TypeError');
});
test('stamp and attachment placement retain resources, intrinsic geometry, authority and independent visible results', () => {
  const notes = read<Annotation[]>('note-placement-and-ghost', 'list');
  assert.equal(notes.length, 1);
  assert.deepEqual(notes[0]!.rect, { x: 90, y: 90, width: 20, height: 20 });
  assert.equal(notes[0]!.noZoom, true);
  assert.equal(notes[0]!.noRotate, true);
  const clamped = read<Annotation[]>('page-box-of-an-offset-crop', 'list').at(-1)!;
  assert.equal(clamped.subtype, 'text');
  assert.equal(clamped.page.objectNumber, 2);
  assert.deepEqual(clamped.rect, { x: 380, y: 480, width: 20, height: 20 });
  const files = read<Annotation[]>('attachment-placement', 'list');
  assert.equal(files.length, 1);
  assert.ok(files[0]!.subtype === 'file-attachment');
  assert.equal(files[0]!.file?.name, 'a.bin');
  assert.deepEqual(files[0]!.rect, { x: 40, y: 40, width: 20, height: 20 });
  const stamps = read<Annotation[]>('stamp-prompt-through-the-file-picker', 'list');
  assert.equal(stamps.length, 2);
  assert.ok(stamps.every((annotation) => annotation.subtype === 'stamp'));
  assert.ok(stamps[0]!.subtype === 'stamp' && stamps[1]!.subtype === 'stamp');
  assert.deepEqual(stamps[0]!.box, { x: 50, y: 75, width: 100, height: 50 });
  assert.equal(stamps[1]!.rotation, 90);
  assert.equal(read<Annotation[]>('armed-stamps', 'list').length, 3);
  assert.equal(read('armed-stamps', 'stamps.isArmed'), false);
  assert.equal(read<Annotation[]>('armed-stamps-need-the-stamp-tag', 'list').length, 7);
  assert.equal(read('armed-stamps-need-the-stamp-tag', 'stamps.isArmed'), true);
  const rotate = read<Annotation[]>('placement-rotation-follows-upright', 'list');
  assert.deepEqual(
    rotate.map((annotation) => ('rotation' in annotation ? annotation.rotation : null)),
    [null, null, null, null],
  );
  const touch = result('touch-consent').trace.flatMap((entry, i) =>
    result('touch-consent').input.steps[i]!.op === 'call' &&
    (result('touch-consent').input.steps[i] as { method?: string }).method === 'wouldClaimTouch'
      ? [(entry.outcome as { returned: boolean }).returned]
      : [],
  );
  assert.deepEqual(touch, [false, true, true, false, false, false]);
});
test('appearance projection returns the render image/version and the render owner snaps its current lattice', () => {
  const pictures = read<
    {
      ref: unknown;
      version: number;
      rect: unknown;
      image: { source: unknown; blob: unknown; objectUrl: unknown };
    }[]
  >('baked-appearances-through-render', 'renderAppearances', 0);
  assert.equal(pictures.length, 1);
  assert.deepEqual(pictures[0]!.ref, objRef(1));
  assert.equal(pictures[0]!.version, 0);
  assert.deepEqual(pictures[0]!.rect, { x: 100, y: 100, width: 100, height: 100 });
  assert.deepEqual(pictures[0]!.image.source, { kind: 'bytes', bytes: { $bytes: 'AQID' } });
  assert.ok(pictures[0]!.image.blob && pictures[0]!.image.objectUrl);
  assert.deepEqual(read('baked-appearances-through-render', 'renderAppearances', 1), []);
  assert.deepEqual(read('baked-appearances-through-render', 'renderAppearances', 2), []);
  assert.deepEqual(
    [0, 1, 2].map((i) => read('appearance-scale-owned-by-render', 'getAppearanceScale', i)),
    [1, 2, 4],
  );
});
import { createKernel } from '@embedpdf/core';
import { createFakeEngine } from '../src/fake-engine';
import { annotationPlugin } from '../../../packages/plugin/annotation/src/annotation.plugin';
import { AnnotationToken } from '../../../packages/plugin/annotation/src/host-contract';
import { interactionPlugin } from '../../../packages/plugin/interaction/src';
import { renderPlugin } from '../../../packages/plugin/render/src/render.plugin';
import { doc, hydrate, square } from '../src/suites/annotation-live-fixtures';
test('actual render cancellation preserves empty annotation projection and aborts in-flight engine reads', async () => {
  const reports: unknown[] = [];
  const fake = createFakeEngine(
    {
      documents: {
        a: doc([
          hydrate([square(1)]),
          { path: 'page.annotations.renderAppearances', pon: 1, defer: 'appearances' },
        ]),
      },
    },
    (value) => value,
    (error) => reports.push(error),
  );
  const kernel = createKernel({
    engine: fake.engine,
    plugins: [interactionPlugin(), renderPlugin(), annotationPlugin()],
    report: (error) => reports.push(error),
  });
  try {
    await kernel.start();
    await kernel.documents.open({ kind: 'bytes', id: 'a', bytes: new Uint8Array() });
    await new Promise<void>((resolve) => setImmediate(resolve));
    const annotation = kernel.capability(AnnotationToken, 'a');
    assert.ok(annotation);
    const early = new AbortController();
    early.abort(new Error('intended early cancellation'));
    assert.deepEqual(await annotation.renderAppearances(pr(1), 1, early.signal), []);
    const late = new AbortController();
    const pending = annotation.renderAppearances(pr(1), 2, late.signal);
    await new Promise<void>((resolve) => setImmediate(resolve));
    late.abort(new Error('intended late cancellation'));
    assert.deepEqual(await pending, []);
    assert.ok(
      fake.log.some((entry) => 'abort' in entry),
      'the live render request was aborted',
    );
    assert.deepEqual(await annotation.renderAppearances(pr(99), 1), []);
    assert.deepEqual(fake.unmatched, []);
    assert.deepEqual(reports, []);
  } finally {
    await kernel.destroy();
  }
});
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { canonical } from '../src/canonical';
const digest = (value: string | Uint8Array) => createHash('sha256').update(value).digest('hex');
test('every original66 definition and canonical input maps exactly to the current67 cases', async () => {
  const root = new URL('../historical/task-12b2b1b-ii/', import.meta.url);
  const manifest = JSON.parse(await readFile(new URL('accounting.json', root), 'utf8')) as {
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
      originalSourceSha256: string;
      cases: {
        before: string;
        after: string;
        oldCanonicalInputSha256: string;
        currentCanonicalInputSha256: string;
        task11InputDrift: boolean;
        migrationNotes: string;
      }[];
      added: { name: string; currentCanonicalInputSha256: string; replacementFor: null }[];
    }[];
  };
  assert.equal(manifest.base, 'bbbdb2678');
  assert.deepEqual(
    [
      manifest.before,
      manifest.after,
      manifest.mapped,
      manifest.retired,
      manifest.renamed,
      manifest.added,
    ],
    [66, 67, 66, 0, 6, 1],
  );
  let original = 0,
    current = 0,
    renamed = 0,
    drifts = 0,
    added = 0;
  for (const row of manifest.suites) {
    const suite = suites.find((suite) => suite.file === row.file);
    assert.ok(suite && suite.kind === 'scenario');
    const source = await readFile(
      new URL(row.file.split('/')[1]!.replace('.json', '.ts.txt'), root),
      'utf8',
    );
    assert.equal(digest(source), row.originalSourceSha256);
    assert.equal(row.before, row.cases.length);
    assert.equal(suite.cases.length, row.after);
    assert.equal(new Set(row.cases.map((row) => row.before)).size, row.before);
    for (const entry of row.cases) {
      assert.ok(source.includes(`name: '${entry.before}'`), entry.before);
      const input: ScenarioCase | undefined = suite.cases.find(
        (candidate) => candidate.name === entry.after,
      );
      assert.ok(input, entry.after);
      assert.equal(
        digest(JSON.stringify(canonical(input))),
        entry.currentCanonicalInputSha256,
        entry.after,
      );
      assert.match(entry.oldCanonicalInputSha256, /^[a-f0-9]{64}$/);
      assert.ok(entry.migrationNotes.length > 30);
      renamed += Number(entry.before !== entry.after);
      drifts += Number(entry.task11InputDrift);
    }
    for (const entry of row.added) {
      assert.equal(entry.replacementFor, null);
      const input: ScenarioCase | undefined = suite.cases.find(
        (candidate) => candidate.name === entry.name,
      );
      assert.ok(input);
      assert.equal(digest(JSON.stringify(canonical(input))), entry.currentCanonicalInputSha256);
      added++;
    }
    assert.deepEqual(
      [...row.cases.map((row) => row.after), ...row.added.map((row) => row.name)].sort(),
      suite.cases.map((input) => input.name).sort(),
    );
    original += row.before;
    current += row.after;
  }
  assert.deepEqual([original, current, renamed, drifts, added], [66, 67, 6, 1, 1]);
});
test('the whole original115 family and all reviewed H3, entropy, resolver and27 mappings stay pinned', async () => {
  const root = new URL('../', import.meta.url);
  const family = JSON.parse(
    await readFile(new URL('historical/task-12b2b1b-ii/family.json', root), 'utf8'),
  ) as {
    original: number;
    current: number;
    mapped: number;
    retired: number;
    renamed: number;
    added: number;
    originalSuites: { file: string; before: number; after: number }[];
    additionalReconciliation: { current: number; historicalMappings: number };
    preserved: Record<string, string>;
  };
  assert.deepEqual(
    [family.original, family.current, family.mapped, family.retired, family.renamed, family.added],
    [115, 149, 115, 0, 9, 34],
  );
  assert.equal(family.originalSuites.length, 8);
  assert.equal(
    family.originalSuites.reduce((sum, row) => sum + row.before, 0),
    115,
  );
  assert.equal(
    family.originalSuites.reduce((sum, row) => sum + row.after, 0) +
      family.additionalReconciliation.current,
    149,
  );
  assert.equal(family.additionalReconciliation.historicalMappings, 27);
  for (const [path, sha] of Object.entries(family.preserved))
    assert.equal(digest(await readFile(new URL(path, root))), sha, path);
});
