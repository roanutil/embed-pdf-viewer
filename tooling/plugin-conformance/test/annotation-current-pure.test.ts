import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { canonical, snapshotHooks } from '../src/canonical';
import type { FunctionSuite } from '../src/vectors';
import * as repository from '../src/suites/annotation-repository';
import * as pure from '../src/suites/annotation-pure';
import { AnnotationSchema } from '../../../packages/engine/core/src/annotation/kinds';
import { annotationOfNew, shapeOf, textOf } from '../../../packages/core/annotation/src/record';
import { annotationKey } from '@embedpdf/core';
import type {
  AnnotationDraft,
  CommentThread,
  RichTextParagraph,
} from '@embedpdf/engine-core/runtime';
import type { ModelAnnotation } from '../../../packages/core/annotation/src/types';
import type {
  TextItem,
  ANNOTATION_DEFAULTS,
} from '../../../packages/plugin/annotation/src/contract';
import {
  initialAnnotationState,
  withSession,
  setGhostAt,
  withSnap,
} from '../../../packages/plugin/annotation/src/model';
import { initialSession } from '../../../packages/core/annotation/src/update';
import type { ResolvedTool } from '../../../packages/plugin/annotation/src/tools/definitions';
const suites = [...repository.suites, ...pure.suites] as FunctionSuite[];
const expectedFailures = new Map<string, [string, string]>([
  ['fromDTO/no-ref', ['TypeError', "undefined (reading 'kind')"]],
  ['fromDTO/null-ref', ['TypeError', "null (reading 'kind')"]],
  ['buildToolRegistry/unknown-base', ['Error', "tool 'nope' extends an unknown tool"]],
  ...[
    ['bad-default', 'square', 'fontSize', 'square'],
    ['bad-default-line', 'bad-line', 'icon', 'line'],
    ['bad-default-polyline', 'bad-polyline', 'icon', 'polyline'],
    ['bad-default-ink', 'bad-ink', 'icon', 'ink'],
    ['bad-default-underline', 'bad-underline', 'strokeWidth', 'underline'],
    ['bad-default-redact', 'bad-redact', 'strokeWidth', 'redact'],
    ['bad-default-link', 'bad-link', 'color', 'link'],
    ['bad-default-file-attachment', 'bad-file-attachment', 'strokeWidth', 'file-attachment'],
    ['bad-default-widget-choice', 'bad-widget-choice', 'opacity', 'widget-choice'],
    ['bad-default-widget-toggle', 'bad-widget-toggle', 'fontSize', 'widget-toggle'],
  ].map(([label, id, field, kind]): [string, [string, string]] => [
    `buildToolRegistry/${label}`,
    ['Error', `tool '${id}' does not support default '${field}' for '${kind}'`],
  ]),
  ['iconPlacement/attachment-no-file', ['Error', 'requires a file payload']],
  ['rangeProps/no-color', ['TypeError', "undefined (reading 'toLowerCase')"]],
  ...[
    ['square-negative', 'box'],
    ['square-nan', 'box'],
    ['square-negative-height', 'box'],
    ['square-rotation-nan', 'rotation'],
    ['polygon-short', 'vertices'],
    ['polyline-nan', 'vertices'],
    ['ink-nan', 'inkList'],
    ['squiggly-empty', 'quadPoints'],
    ['redact-area-nan', 'rect'],
  ].map(([label, path]): [string, [string, string]] => [
    `parseAnnotationDraft/${label}`,
    ['ZodError', path!],
  ]),
]);
function decode(v: unknown): unknown {
  if (Array.isArray(v)) return v.map(decode);
  if (!v || typeof v !== 'object') return v;
  const o = v as Record<string, unknown>;
  if ('$number' in o) return Number(o.$number);
  if ('$undefined' in o) return undefined;
  if ('$bytes' in o) return new Uint8Array(Buffer.from(o.$bytes as string, 'base64'));
  if ('$set' in o) return new Set((o.$set as unknown[]).map(decode));
  return Object.fromEntries(Object.entries(o).map(([k, v]) => [k, decode(v)]));
}
function run<T = unknown>(name: string): T {
  const suite = suites.find((s) => s.cases.some((c) => c.name === name));
  assert.ok(suite, name);
  const c = suite.cases.find((c) => c.name === name)!;
  return suite.functions[c.fn]!(...(decode(canonical(c.args)) as never[])) as T;
}
function nonfinite(v: unknown): boolean {
  if (typeof v === 'number') return !Number.isFinite(v);
  if (Array.isArray(v)) return v.some(nonfinite);
  return !!v && typeof v === 'object' && Object.values(v).some(nonfinite);
}

test('850 historical definitions map exactly to current targets or eight evidenced retirements', async () => {
  const history = JSON.parse(
    await readFile(new URL('../historical/task-12b2b1a/accounting.json', import.meta.url), 'utf8'),
  );
  let before = 0,
    after = 0,
    retired = 0;
  for (const [i, module] of [repository, pure].entries()) {
    const suite = module.suites[0] as FunctionSuite,
      row = history.suites[i];
    assert.equal(suite.file, row.file);
    assert.equal(
      row.cases.filter((entry: { task11InputDrift: boolean }) => entry.task11InputDrift).length,
      i === 0 ? 573 : 23,
    );
    assert.deepEqual(
      module.migration,
      row.cases.map(
        ({ before, after, reason }: { before: string; after: string | null; reason?: string }) => ({
          before,
          after,
          ...(reason ? { reason } : {}),
        }),
      ),
    );
    assert.equal(new Set(module.migration.map((m) => m.before)).size, row.before);
    const names = new Set(suite.cases.map((c) => c.name));
    assert.equal(names.size, suite.cases.length);
    for (const m of module.migration) {
      if (m.after === null) {
        assert.ok(m.reason);
        retired++;
      } else assert.ok(names.has(m.after), m.before);
    }
    assert.deepEqual(
      [...names].filter((n) => !module.migration.some((m) => m.after === n)),
      row.added,
    );
    before += row.before;
    after += suite.cases.length;
  }
  assert.equal(before, 850);
  assert.equal(after, 843);
  assert.equal(retired, 8);
});

test('all843 current calls have exactly their named outcomes, immutable args and no diagnostics', () => {
  let calls = 0,
    failures = 0,
    nonfiniteResults = 0;
  const observed: unknown[][] = [];
  const original = { error: console.error, warn: console.warn };
  console.error = (...a) => observed.push(a);
  console.warn = (...a) => observed.push(a);
  try {
    for (const suite of suites)
      for (const c of suite.cases) {
        assert.equal(typeof suite.functions[c.fn], 'function', c.name);
        const before = canonical(c.args);
        let result: unknown;
        try {
          result = run(c.name);
          assert.ok(!expectedFailures.has(c.name), `${c.name}: expected refusal returned`);
        } catch (error) {
          const expected = expectedFailures.get(c.name);
          assert.ok(expected, `${c.name}: unexpected ${String(error)}`);
          assert.ok(error instanceof Error);
          assert.equal(error.name, expected[0], c.name);
          assert.ok(error.message.includes(expected[1]), `${c.name}: ${error.message}`);
          failures++;
        }
        if (!expectedFailures.has(c.name))
          assert.ok(
            !/"(?:diagnostics|errors)":\[\{|"\$error":/.test(
              JSON.stringify(canonical(result, snapshotHooks)) ?? '',
            ),
            c.name,
          );
        if (nonfinite(result)) {
          assert.ok(nonfinite(decode(before)), `${c.name}: finite input produced nonfinite output`);
          nonfiniteResults++;
        }
        assert.deepEqual(canonical(c.args), before, `${c.name}: input mutated`);
        calls++;
      }
  } finally {
    console.error = original.error;
    console.warn = original.warn;
  }
  assert.deepEqual(observed, []);
  assert.equal(calls, 843);
  assert.equal(failures, expectedFailures.size);
  assert.equal(failures, 24);
  assert.equal(nonfiniteResults, 7);
});

test('current records pass the real read schema except explicitly malformed historical edges', () => {
  let checked = 0,
    malformed = 0;
  const expected =
    /callout-one-point|square-rotation-nan|square-empty-interior|fromDTO\/null-ref|richDocOf\/dto-plain|buildCommentThreads\/(everything|mine|mine-absent)$/;
  const visit = (v: unknown, name: string): void => {
    if (!v || typeof v !== 'object') return;
    const o = v as Record<string, unknown>;
    if ('subtype' in o && 'ref' in o && 'page' in o) {
      checked++;
      const result = AnnotationSchema.safeParse(o);
      if (!result.success) {
        assert.match(name, expected, `${name}: ${result.error.message}`);
        malformed++;
      } else {
        assert.deepEqual(result.data.page, result.data.ref.page, name);
        assert.ok(
          !('flags' in o || 'geom' in o || 'data' in o || 'inReplyTo' in o || 'created' in o),
          name,
        );
      }
    }
    for (const value of Object.values(v)) visit(value, name);
  };
  for (const s of suites) for (const c of s.cases) visit(c.args, c.name);
  assert.ok(checked > 690);
  assert.equal(malformed, 24);
});

test('record conversion keeps stable refs and actual rotated/upright/page-offset boxes', () => {
  const base = run<ModelAnnotation>('fromDTO/nm-ref');
  assert.equal(base.id, 'base:1:3');
  assert.equal(base.id, annotationKey(base.annotation.ref));
  const rotated = run<ModelAnnotation>('fromDTO/square-rotated');
  assert.deepEqual(rotated.apBox, { x: 110, y: 110, width: 80, height: 80 });
  assert.equal(rotated.apRot, 30);
  const upright = run<{ box: unknown }>(
    'annotationPatchBetween/toPatch/free-text-unrotated-box-no-rotation',
  );
  assert.deepEqual(upright.box, { x: 110, y: 110, width: 80, height: 80 });
  const offset = run<ModelAnnotation>('fromDTO/square-offset-crop');
  assert.deepEqual(offset.annotation.page, { kind: 'objectNumber', objectNumber: 2 });
  assert.deepEqual(shapeOf(offset.annotation), {
    kind: 'box',
    box: { x: 50, y: 320, width: 100, height: 100 },
    rotation: 0,
    ellipse: false,
  });
});

test('geometry writes translate boxes, vertices, measurement pins and callout lines', () => {
  for (const label of [
    'square-rotated',
    'line',
    'polygon-dimension',
    'ink',
    'highlight',
    'callout-rotated-distinct-box',
  ]) {
    const before = repository.DTOS.find(([name]) => name === label)![1],
      after = run<ModelAnnotation>(`withShape/toScopedPatch/${label}/geometry`).annotation;
    assert.notDeepEqual(shapeOf(after), shapeOf(before), label);
    if (after.subtype === 'polygon') assert.deepEqual(after.captionCenter, { x: 157, y: 127 });
    if (after.subtype === 'free-text') {
      assert.equal(after.box.x, 167);
      assert.equal(after.box.y, 107);
      assert.deepEqual(after.calloutLine?.[0], { x: 67, y: 287 });
    }
  }
});

test('field edits retain accepted color/border/URI and ignore incompatible, unknown and empty values', () => {
  const color = run<ModelAnnotation>('withValues/toScopedPatch/square/color').annotation;
  assert.ok('color' in color);
  assert.equal(color.color, '#123456');
  const border = run<ModelAnnotation>('withValues/toScopedPatch/square/border').annotation;
  assert.ok('borderStyle' in border);
  assert.equal(border.borderStyle, 'dashed');
  assert.deepEqual(border.dashArray, [6, 2]);
  const link = run<ModelAnnotation>('withValues/toScopedPatch/link/link').annotation;
  assert.ok(link.subtype === 'link');
  assert.deepEqual(link.target, { kind: 'uri', uri: 'https://docs.example' });
  assert.ok(!('target' in run<ModelAnnotation>('withValues/toScopedPatch/square/link').annotation));
  for (const suffix of ['unknown', 'empty']) {
    const s = repository.suites[0] as FunctionSuite,
      c = s.cases.find((c) => c.name === `withValues/toScopedPatch/square/${suffix}`)!;
    assert.equal(s.functions[c.fn]!(...(c.args as never[])), c.args[0]);
  }
});

test('draw drafts preserve current URI, reply, cloud, dash, endings and engine measurement label', () => {
  const link = run<AnnotationDraft>('draftOf/toCreateDraft/link');
  assert.ok(link.subtype === 'link');
  assert.deepEqual(link.target, { kind: 'uri', uri: 'https://example.com' });
  assert.deepEqual(run<AnnotationDraft>('draftOf/toCreateDraft/link-attached').reply, {
    to: { kind: 'objectNumber', page: { kind: 'objectNumber', objectNumber: 1 }, objectNumber: 1 },
    type: 'group',
  });
  const dash = run<AnnotationDraft>('draftOf/toCreateDraft/square-dashed');
  assert.ok(dash.subtype === 'square');
  assert.equal(dash.borderStyle, 'dashed');
  assert.deepEqual(dash.dashArray, [4, 2]);
  const cloud = run<AnnotationDraft>('draftOf/toCreateDraft/styled/square-cloudy');
  assert.ok(cloud.subtype === 'square');
  assert.equal(cloud.cloudyIntensity, 2);
  const dimension = run<AnnotationDraft>('draftOf/toCreateDraft/line-dimension');
  assert.ok(dimension.subtype === 'line');
  assert.equal(dimension.intent, 'line-dimension');
  assert.equal(dimension.measure?.subtype, 'rectilinear');
  assert.equal(dimension.captionEnabled, true);
  assert.deepEqual(dimension.leader, { length: 12, extension: 5, offset: 0 });
  const written = annotationOfNew(dimension, {
    ref: {
      kind: 'objectNumber',
      page: { kind: 'objectNumber', objectNumber: 1 },
      objectNumber: 100001,
    },
  });
  assert.equal(written.contents, '2.00 m');
});

test('live rich text projects paragraphs and all three toggles rather than empty output', () => {
  const rich = run<TextItem[]>('buildTextItems/rich');
  assert.equal(rich.length, 3);
  assert.deepEqual(
    rich.map((i) => i.id),
    ['obj:11', 'obj:12', 'obj:13'],
  );
  assert.equal(rich[0]!.css.fontWeight, 700);
  assert.equal(rich[0]!.css.fontStyle, 'italic');
  assert.equal(rich[0]!.css.textDecoration, 'underline');
  assert.deepEqual(
    rich[0]!.richText.paragraphs[0]!.runs.map((r) => r.text),
    ['Hel', 'lo'],
  );
  const page = run<TextItem[]>('buildTextItems/page');
  assert.equal(page.length, 5);
  assert.ok(page.some((i) => i.id === 'obj:2' && i.editing));
  assert.ok(!page.some((i) => i.id === 'obj:6' || i.id === 'obj:7'));
  assert.equal(run<TextItem[]>('buildTextItems/other-page').length, 1);
  const styled = run<ModelAnnotation>('withValues/toScopedPatch/free-text-rich/format');
  assert.deepEqual(textOf(styled.annotation), {
    fontFamily: 'helvetica',
    fontSize: 12,
    fontColor: '#ef4444',
    textAlign: 'left',
    bold: true,
    italic: true,
    underline: true,
  });
  const commit = run<{ richText: { paragraphs: RichTextParagraph[] } }>(
    'textCommitPatch/strips-defaults',
  );
  assert.equal(commit.richText.paragraphs[0]!.align, undefined);
  assert.equal(commit.richText.paragraphs[1]!.align, 'center');
});

test('tool defaults preserve URI, flags and ghost and accept current supported fields', () => {
  const tools = run<Map<string, ResolvedTool>>('buildToolRegistry/widget-kinds');
  assert.deepEqual(tools.get('docs-link')!.defaults!.target, { kind: 'uri', uri: 'https://docs' });
  assert.equal(tools.get('field-text')!.defaults!.borderStyle, 'solid');
  const configured = run<Map<string, ResolvedTool>>('buildToolRegistry/configure-built-in');
  assert.equal(configured.get('square')!.flags?.locked, true);
  assert.equal(configured.get('ink')!.defaults!.strokeWidth, 9);
  const presets = run<Map<string, ResolvedTool>>('buildToolRegistry/presets');
  assert.deepEqual(presets.get('big-free')!.ghost, { opacity: 0.4 });
  assert.equal(presets.get('thick-arrow')!.defaults!.strokeWidth, 12);
  assert.equal(
    run<Map<string, ResolvedTool>>('buildToolRegistry/bad-default-free-text').get('bad-free-text')!
      .defaults!.blendMode,
    'multiply',
  );
  assert.equal(run('toolTouchOf/draw'), 'draw');
  assert.equal(run('toolTouchOf/select'), 'draw');
  assert.equal(run('toolTouchOf/place'), 'tap');
});

test('named transitions preserve identity and settings use actual chrome/snap members', () => {
  const initial = initialAnnotationState();
  assert.equal(withSession(initial, initial.session), initial);
  assert.equal(setGhostAt(initial, null), initial);
  assert.equal(withSnap(initial, initial.session.snap), initial);
  const state = run<ReturnType<typeof initialAnnotationState>>('withSession/set-model');
  assert.deepEqual(state.session.objectNumbers, [100001, 100002, 100003, 100004, 100005]);
  assert.equal('byId' in state.session, false);
  const settings = run<typeof ANNOTATION_DEFAULTS>('mergeSettings/configured');
  assert.equal(settings.chrome.rotationHandle.stalk, false);
  assert.equal(settings.chrome.guides.enabled, false);
  assert.deepEqual(settings.snap.rotationAngles, [0, 45]);
  const chrome = run<typeof settings.chrome>('mergeSettings/deep');
  assert.equal(chrome.rotationHandle.offset, 40);
  assert.equal(chrome.rotationHandle.hitSize, 24);
  assert.equal(chrome.guides.color, '#555555');
  assert.equal('knob' in chrome, false);
  assert.equal(initial.session, initialSession);
});

test('threads resolve canonical replies, real dates, grouped parts and input-order ties', () => {
  const threads = run<CommentThread[]>('buildCommentThreads/everything'),
    root = threads.find((t) => annotationKey(t.root.ref) === 'obj:1')!;
  assert.ok(root);
  assert.deepEqual(
    root.replies.map((r) => annotationKey(r.ref)),
    ['obj:3', 'obj:2', 'obj:4', 'obj:5'],
  );
  assert.deepEqual(
    root.groupedParts.map((r) => annotationKey(r.ref)),
    ['obj:16'],
  );
  const order = run<CommentThread[]>('buildCommentThreads/nm-refs-and-ordering');
  assert.equal(order[0]!.root.ref.kind, 'baseIndex');
  assert.equal(annotationKey(order[0]!.replies[0]!.ref), 'obj:63');
  const dated = order.find((t) => annotationKey(t.root.ref) === 'obj:64')!;
  assert.deepEqual(
    dated.replies.map((r) => annotationKey(r.ref)),
    ['obj:85', 'base:1:80', 'obj:80', 'obj:90', 'obj:70', 'obj:81'],
  );
  assert.ok(threads.some((t) => annotationKey(t.root.ref) === 'obj:17'));
  assert.ok(
    !threads.some((t) => ['obj:21', 'obj:22', 'obj:23'].includes(annotationKey(t.root.ref))),
  );
});

test('child link geometry receives current shape/style and colors use current component functions', () => {
  assert.deepEqual(run('linkChildRects/callout'), [{ x: 150, y: 100, width: 150, height: 150 }]);
  assert.deepEqual(run('linkChildRects/highlight'), [{ x: 50, y: 55, width: 150, height: 15 }]);
  assert.equal(run('colorOf/colorToCss/clamped'), '#ff000d');
  assert.deepEqual(run('rgbOf/cssToColor/six'), { r: 170, g: 187, b: 204 });
});

test('full patch replacement writes current style/rotation and excludes engine metadata', () => {
  const rotated = run<Record<string, unknown>>('annotationPatchBetween/toPatch/square-rotated');
  assert.equal(rotated.rotation, 30);
  assert.deepEqual(rotated.box, { x: 110, y: 110, width: 80, height: 80 });
  const cloud = run<Record<string, unknown>>('annotationPatchBetween/toPatch/styled/square-cloudy');
  assert.equal(cloud.cloudyIntensity, 2);
  assert.equal(cloud.color, '#ef4444');
  assert.equal(cloud.contents, null);
  for (const key of ['ref', 'page', 'rect', 'author', 'createdAt', 'modifiedAt', 'hasAppearance'])
    assert.ok(!(key in cloud), key);
});
