/**
 * `packages/core/annotation` update.ts: the reducer. Replayed by
 * `crates/plugins/annotation-core/tests/conformance.rs`.
 *
 * `run(start, msgs)` layers `start` over `initialModel`, applies each message
 * in turn and records, per step, whether `update` handed back the SAME model
 * object (`same`: the "no change" a store skips a notification for), the
 * effects, and the draft and selection it left; then the final model. The
 * other exports (`rotateDraftDelta`, `calloutBox`, `calloutUprightRot`,
 * `annotsInBox`) are called directly.
 */
import * as up from '../../../../packages/core/annotation/src/update';
import { MIN_DRAG } from '../../../../packages/core/annotation/src/placement';
import { rectFromPoints } from '../../../../packages/core/annotation/src/rect';
import type { ViewEnv } from '../../../../packages/core/annotation/src/anchor';
import type { Draft, Id, Model, PointerInput } from '../../../../packages/core/annotation/src/types';
import { step } from '../../../../packages/core/annotation/test/support';

import { canonical } from '../canonical';
import { removedUpstream } from '../removed';
import type { FunctionCase, Suite } from '../vectors';
import {
  DASHED,
  PON,
  SOLID,
  annot,
  c,
  cloudy,
  ends,
  flags,
  gCaret,
  gInk,
  gLine,
  gPoly,
  gQuads,
  gRect,
  gText,
  modelOf,
  rc,
  sq,
  start,
  style,
  tq,
  tqRot,
  v,
  type Fn,
  pageRef,
  toMessage,
  type Annot,
  type Msg,
  type Vec,
} from './annotation-core-fixtures';

/** Value equality in canonical JSON: member order ignored, `-0` is `0`. */
const sorted = (x: unknown): unknown =>
  Array.isArray(x)
    ? x.map(sorted)
    : x && typeof x === 'object'
      ? Object.fromEntries(Object.keys(x).sort().map((k) => [k, sorted((x as Record<string, unknown>)[k])]))
      : x;
const sameValue = (a: unknown, b: unknown): boolean =>
  JSON.stringify(sorted(canonical(a))) === JSON.stringify(sorted(canonical(b)));

export const updateFns: Record<string, Fn> = {
  MIN_DRAG: () => MIN_DRAG,
  initialStyle: removedUpstream('initialStyle', '67180c5d8'),
  initialModel: () => up.initialModel,
  run: (s: Partial<Model>, msgs: Msg[]) => runOne(s, msgs),
  /** Many runs on one page: each run's start is `page` with `over` on top. */
  runs: (page: Partial<Model>, list: { name: string; over: Partial<Model>; msgs: Msg[] }[]) =>
    list.map(({ name, over, msgs }) => ({ name, ...runOne({ ...page, ...over }, msgs) })),
  rotateDraftDelta: (s: Partial<Model>, d: Extract<Draft, { g: 'rotate' }>) => up.rotateDraftDelta(modelOf(s), d),
  /** One page, many drafts. */
  rotateDraftDeltas: (s: Partial<Model>, ds: Extract<Draft, { g: 'rotate' }>[]) => {
    const m = modelOf(s);
    return ds.map((d) => up.rotateDraftDelta(m, d));
  },
  calloutBox: up.calloutBox,
  calloutUprightRot: up.calloutUprightRot,
  annotsInBox: (s: Partial<Model>, pon: number, a: Vec, b: Vec, inert?: Set<Id>, view?: ViewEnv) =>
    up.annotsInBox(modelOf(s), pageRef(pon), rectFromPoints(a, b), inert, view),
  /** One page, many boxes: `[pon, a, b, inert?, view?]` each. */
  annotsInBoxes: (s: Partial<Model>, calls: [number, Vec, Vec, Set<Id>?, ViewEnv?][]) => {
    const m = modelOf(s);
    return calls.map(([pon, a, b, inert, view]) =>
      up.annotsInBox(m, pageRef(pon), rectFromPoints(a, b), inert, view),
    );
  },
};

/** Apply `msgs` in turn from `s` over `initialModel`. Per step: whether
 *  `update` returned the same object, its effects, and the draft and
 *  selection left. Then the final model with `byId` cut to the entries whose
 *  value changed (added or replaced), and the ids removed. */
function runOne(s: Partial<Model>, msgs: Msg[]) {
  const first = modelOf(s);
  let m = first;
  const steps = msgs.map((msg) => {
    // Upstream's `update` returns the session and a change set (67180c5d8);
    // its own test support lays one on the model as the plugin does.
    const [next, effects] = step(m, toMessage(msg));
    const same = next === m;
    m = next;
    return { same, effects, draft: m.draft, selected: m.selected };
  });
  const { byId, ...rest } = m;
  const changed = Object.fromEntries(
    Object.entries(byId).filter(([id, a]) => !(id in first.byId) || !sameValue(a, first.byId[id])),
  );
  const removed = Object.keys(first.byId).filter((id) => !(id in byId));
  return { steps, model: { ...rest, byId: changed }, removed };
}

const PAGE = rc(0, 0, 600, 800);
type Phase = 'down' | 'move' | 'up';
const inp = (x: number, y: number, over: Partial<PointerInput> = {}): PointerInput => ({
  page: pageRef(PON),
  point: v(x, y),
  shift: false,
  ...over,
});
const ed = (phase: Phase, x: number, y: number, over: Partial<PointerInput> = {}): Msg => ({ t: 'editPointer', phase, in: inp(x, y, over) });
const mq = (phase: Phase, x: number, y: number, over: Partial<PointerInput> = {}): Msg => ({ t: 'marqueePointer', phase, in: inp(x, y, over) });
const cr = (subtype: string, phase: Phase, x: number, y: number, over: Partial<PointerInput> = {}, msg: object = {}): Msg =>
  ({ t: 'createPointer', phase, subtype, in: inp(x, y, over), ...msg }) as Msg;
/** A full drag: down, move, up. */
const drag = (subtype: string, a: Vec, b: Vec, over: Partial<PointerInput> = {}, msg: object = {}): Msg[] => [
  cr(subtype, 'down', a.x, a.y, over, msg),
  cr(subtype, 'move', b.x, b.y, over, msg),
  cr(subtype, 'up', b.x, b.y, over, msg),
];
const editDrag = (a: Vec, b: Vec, over: Partial<PointerInput> = {}): Msg[] => [ed('down', a.x, a.y, over), ed('move', b.x, b.y, over), ed('up', b.x, b.y, over)];

const TEXT = { fontFamily: 'helvetica', fontSize: 12, fontColor: '#000000', textAlign: 'left' as const };
const URI = { kind: 'uri', uri: 'https://example.com' };
const DTO = (over: object = {}) => ({ subtype: 'free-text', contents: 'hello', ...over }) as never;

/** A page with one of everything, all committed. */
const ANNOTS: Annot[] = [
  annot('hl', 'highlight', gQuads([tq(10, 400, 200, 14)])),
  sq('sq', rc(100, 100, 80, 60)),
  sq('sq2', rc(300, 100, 80, 60), { style: style({ interiorColor: '#ffffff' }) }),
  annot('ci', 'circle', gRect(rc(100, 250, 80, 60), true)),
  annot('ln', 'line', gLine(v(300, 250), v(400, 300), ends('none', 'closed-arrow'))),
  annot('pg', 'polygon', gPoly([v(100, 500), v(200, 500), v(150, 580)], true)),
  annot('pl', 'polyline', gPoly([v(300, 500), v(350, 560), v(400, 500)], false, ends('none', 'none'), 20)),
  annot('ink', 'ink', gInk([[v(450, 100), v(500, 150), v(550, 100)]])),
  annot('ft', 'free-text', gText(rc(450, 250, 100, 40)), { text: TEXT, data: DTO() }),
  annot('callout', 'free-text', gText(rc(200, 600, 100, 40), { tip: v(100, 550), ending: 'open-arrow' }), { text: TEXT }),
  annot('stamp', 'stamp', gRect(rc(450, 400, 80, 40)), { source: 'baked', apBox: rc(450, 400, 80, 40) }),
  annot('note', 'text', gRect(rc(20, 20, 20, 20)), { source: 'baked', apBox: rc(20, 20, 20, 20) }),
  annot('caret', 'caret', gCaret(rc(250, 400, 6, 6))),
  annot('link', 'link', gRect(rc(20, 700, 100, 20)), { link: URI as never }),
  annot('redact', 'redact', gRect(rc(300, 650, 100, 50))),
  sq('locked', rc(100, 650, 60, 40), { flags: flags({ locked: true }) }),
  sq('lockedc', rc(200, 700, 20, 20), { flags: flags({ lockedContents: true }) }),
  sq('baked', rc(450, 500, 60, 40), { source: 'baked', apBox: rc(448, 498, 64, 44) }),
  annot('widget', 'widget-text', gRect(rc(20, 550, 80, 20)), { source: 'baked', text: TEXT }),
  sq('nozoom', rc(560, 700, 30, 30), { flags: flags({ noZoom: true, noRotate: true }) }),
  annot('child', 'link', gRect(rc(100, 100, 80, 60)), { group: 'sq', irt: 'sq', data: { subtype: 'link', target: URI } as never }),
  sq('other-page', rc(100, 100, 80, 60), { page: pageRef(2) }),
];
const S = (over: Partial<Model> = {}) => start(ANNOTS, over);
const GROUP = start([
  sq('p', rc(100, 100, 80, 60)),
  annot('m1', 'strikeout', gQuads([tq(100, 200, 80, 12)]), { group: 'p', irt: 'p' }),
  sq('m2', rc(250, 100, 40, 40), { group: 'p' }),
  sq('solo', rc(400, 100, 40, 40)),
  annot('tmp:1', 'caret', gCaret(rc(10, 10, 5, 5)), { ref: null }),
  annot('tmp:2', 'strikeout', gQuads([tq(10, 20, 30, 10)]), { ref: null, irt: 'tmp:1', group: 'tmp:1' }),
]);

type Case = [string, Partial<Model>, Msg[]];
const RUNS: Case[] = [
  // ── edit: down on every target kind ──
  ['edit-down-empty', S({ selected: ['sq'] }), [ed('down', 250, 180)]],
  ['edit-down-body', S(), [ed('down', 101, 130)]],
  ['edit-down-body-shift-add', S({ selected: ['sq2'] }), [ed('down', 101, 130, { shift: true })]],
  ['edit-down-body-shift-drop', S({ selected: ['sq', 'sq2'] }), [ed('down', 101, 130, { shift: true })]],
  ['edit-down-selected-keeps', S({ selected: ['sq', 'sq2'] }), [ed('down', 101, 130)]],
  ['edit-down-anchored', S(), [ed('down', 100, 405)]],
  ['edit-down-group-member', GROUP, [ed('down', 101, 130)]],
  ['edit-down-group-member-shift-drop', { ...GROUP, selected: ['p', 'm1', 'm2'] }, [ed('down', 251, 120, { shift: true })]],
  ['edit-down-handle', S({ selected: ['sq'] }), [ed('down', 180, 160)]],
  ['edit-down-handle-view', S({ selected: ['nozoom'] }), [ed('down', 575, 715, { zoom: 2 })]],
  ['edit-down-knob', S({ selected: ['sq'] }), [ed('down', 140, 76)]],
  ['edit-down-knob-view', S({ selected: ['sq'] }), [ed('down', 140, 76, { displayRotation: 90 })]],
  ['edit-down-group-knob', S({ selected: ['sq', 'sq2'] }), [ed('down', 240, 76)]],
  ['edit-down-group-handle', S({ selected: ['sq', 'sq2'] }), [ed('down', 380, 160)]],
  ['edit-down-group-handle-view', S({ selected: ['sq', 'sq2'] }), [ed('down', 380, 160, { zoom: 1 })]],
  ['edit-down-custom-chrome', S({ selected: ['sq'] }), [ed('down', 140, 50, { chrome: { handleTol: 2, knobTol: 3, knobOffset: 50 } })]],
  ['edit-down-inert', S(), [ed('down', 101, 130, { inert: new Set(['sq']) })]],
  ['edit-down-locked', S(), [ed('down', 100, 670)]],
  // ── edit: move ──
  ['edit-move-no-draft', S(), [ed('move', 10, 10)]],
  ['edit-up-no-draft', S(), [ed('up', 10, 10)]],
  ['edit-move-drag', S(), [ed('down', 101, 130), ed('move', 131, 170)]],
  ['edit-move-foreign-page', S(), [ed('down', 101, 130), ed('move', 131, 170, { page: pageRef(2) })]],
  ['edit-move-clamped', S(), [ed('down', 101, 130), ed('move', -500, 2000, { pageBox: PAGE })]],
  ['edit-move-shift-no-snap', S(), [ed('down', 101, 130), ed('move', 300, 131, { shift: true })]],
  ['edit-move-snap', S(), [ed('down', 101, 130), ed('move', 299, 131)]],
  ['edit-move-snap-off', S({ snap: { ...up.initialModel.snap, guides: false } }), [ed('down', 101, 130), ed('move', 299, 131)]],
  ['edit-move-snap-clamped', S(), [ed('down', 101, 130), ed('move', 518, 131, { pageBox: PAGE })]],
  ['edit-move-snap-page-edge', S(), [ed('down', 101, 130), ed('move', 3, 33, { pageBox: PAGE })]],
  ['edit-move-anchored-view-clamp', S({ selected: ['nozoom'] }), [ed('down', 575, 715), ed('move', 700, 900, { pageBox: PAGE, zoom: 2 })]],
  ['edit-handle-resize', S({ selected: ['sq'] }), [ed('down', 180, 160), ed('move', 220, 200), ed('move', 250, 1000, { pageBox: PAGE })]],
  ['edit-handle-foreign', S({ selected: ['sq'] }), [ed('down', 180, 160), ed('move', 220, 200, { page: pageRef(2) })]],
  ['edit-rotate-move', S({ selected: ['sq'] }), [ed('down', 140, 76), ed('move', 200, 90), ed('move', 200, 90, { shift: true })]],
  ['edit-group-resize-move', S({ selected: ['sq', 'sq2'] }), [ed('down', 380, 160), ed('move', 420, 200)]],
  ['edit-group-resize-iso', S({ selected: ['sq', 'pl'] }), [ed('down', 100, 100), ed('move', 50, 50)]],
  ['edit-marquee-draft-move', S(), [mq('down', 0, 0), ed('move', 10, 10), ed('up', 10, 10)]],
  // ── edit: up ──
  ['edit-move-commit', S(), editDrag(v(101, 130), v(151, 180))],
  ['edit-move-commit-baked', S(), [ed('down', 451, 520), ed('move', 471, 540), ed('up', 471, 540)]],
  ['edit-move-click', S(), editDrag(v(101, 130), v(101.005, 130))],
  ['edit-move-group-commit', S({ selected: ['sq', 'sq2', 'stamp'] }), editDrag(v(101, 130), v(111, 140))],
  ['edit-handle-commit', S({ selected: ['sq'] }), editDrag(v(180, 160), v(220, 200))],
  ['edit-handle-noop', S({ selected: ['sq'] }), editDrag(v(180, 160), v(180, 160))],
  ['edit-handle-commit-stamp', S({ selected: ['stamp'] }), editDrag(v(530, 440), v(560, 470))],
  ['edit-handle-commit-stamp-move-only', S({ selected: ['stamp'] }), [ed('down', 530, 440), ed('move', 530.005, 440), ed('up', 530.005, 440)]],
  ['edit-handle-commit-widget', S({ selected: ['widget'] }), editDrag(v(100, 570), v(120, 590))],
  ['edit-handle-commit-view', S({ selected: ['nozoom'] }), editDrag(v(575, 715), v(590, 730), { zoom: 2 })],
  ['edit-handle-commit-view-rot', S({ selected: ['nozoom'] }), editDrag(v(560, 700), v(540, 690), { zoom: 2, displayRotation: 90 })],
  ['edit-handle-vertex', S({ selected: ['ln'] }), editDrag(v(400, 300), v(420, 330))],
  ['edit-handle-callout-tip', S({ selected: ['callout'] }), editDrag(v(100, 550), v(90, 500))],
  ['edit-rotate-commit', S({ selected: ['sq'] }), editDrag(v(140, 76), v(200, 90))],
  ['edit-rotate-commit-snapped', S({ selected: ['sq'] }), editDrag(v(140, 76), v(250, 128))],
  ['edit-rotate-commit-free', S({ selected: ['sq'] }), editDrag(v(140, 76), v(250, 128), { shift: true })],
  ['edit-rotate-noop', S({ selected: ['sq'] }), editDrag(v(140, 76), v(140, 76))],
  ['edit-rotate-snap-off', S({ selected: ['sq'], snap: { ...up.initialModel.snap, rotation: false } }), editDrag(v(140, 76), v(250, 128))],
  ['edit-rotate-group', S({ selected: ['sq', 'sq2', 'ln'] }), editDrag(v(240, 76), v(320, 90))],
  ['edit-rotate-stamp', S({ selected: ['stamp'] }), editDrag(v(490, 376), v(560, 400))],
  ['edit-rotate-view', S({ selected: ['nozoom'] }), editDrag(v(575, 676), v(620, 700), { zoom: 2 })],
  ['edit-rotate-vanished', S({ selected: ['sq'] }), [ed('down', 140, 76), ed('move', 200, 90), { t: 'remove', ids: ['sq'] }, ed('up', 200, 90)]],
  ['edit-group-commit', S({ selected: ['sq', 'sq2'] }), editDrag(v(380, 160), v(420, 200))],
  ['edit-group-noop', S({ selected: ['sq', 'sq2'] }), editDrag(v(380, 160), v(380, 160))],
  ['edit-group-commit-iso', S({ selected: ['sq', 'pl'] }), editDrag(v(100, 100), v(50, 60))],
  ['edit-group-commit-stamp', S({ selected: ['sq', 'stamp'] }), editDrag(v(530, 440), v(580, 480))],
  ['edit-move-then-other-page-up', S(), [ed('down', 101, 130), ed('move', 131, 170), ed('up', 0, 0, { page: pageRef(2) })]],
  // ── marquee ──
  ['marquee-select', S(), [mq('down', 90, 90), mq('move', 200, 200), mq('up', 200, 200)]],
  ['marquee-clamped', S(), [mq('down', -50, -50, { pageBox: PAGE }), mq('up', 150, 150, { pageBox: PAGE })]],
  ['marquee-shift-toggle', S({ selected: ['sq', 'ink'] }), [mq('down', 90, 90), mq('up', 200, 200, { shift: true })]],
  ['marquee-inert', S(), [mq('down', 90, 90), mq('up', 200, 200, { inert: new Set(['sq']) })]],
  ['marquee-foreign', S(), [mq('down', 90, 90), mq('move', 200, 200, { page: pageRef(2) }), mq('up', 200, 200, { page: pageRef(2) })]],
  ['marquee-no-draft', S(), [mq('move', 200, 200), mq('up', 200, 200)]],
  ['marquee-groups', GROUP, [mq('down', 240, 90), mq('up', 300, 150)]],
  ['marquee-view', S(), [mq('down', 540, 690), mq('up', 570, 710, { zoom: 2 })]],
  ['marquee-empty', S({ selected: ['sq'] }), [mq('down', 590, 10), mq('up', 595, 15)]],
  // ── create: shapes ──
  ['create-square', {}, drag('square', v(100, 100), v(200, 160))],
  ['create-square-reversed', {}, drag('square', v(200, 160), v(100, 100))],
  ['create-circle', {}, drag('circle', v(0, 0), v(50, 50))],
  ['create-square-cloudy', { defaults: { square: { border: cloudy(1) } } }, drag('square', v(100, 100), v(200, 160))],
  ['create-square-click', {}, drag('square', v(100, 100), v(101, 101))],
  ['create-square-click-policy', {}, drag('square', v(100, 100), v(101, 101), {}, { clickCreate: { width: 50, height: 30 } })],
  ['create-square-click-segment-policy', {}, drag('square', v(100, 100), v(101, 101), {}, { clickCreate: { length: 30 } })],
  ['create-square-click-policy-page', {}, drag('square', v(595, 795), v(595, 795), { pageBox: PAGE }, { clickCreate: { width: 50, height: 30 } })],
  ['create-square-thin-drag', {}, drag('square', v(100, 100), v(103, 100))],
  ['create-square-upright-90', {}, drag('square', v(100, 100), v(200, 160), { upright: true, displayRotation: 90 })],
  ['create-square-upright-180', {}, drag('square', v(100, 100), v(200, 160), { upright: true, displayRotation: 180 })],
  ['create-square-upright-0', {}, drag('square', v(100, 100), v(200, 160), { upright: true, displayRotation: 0 })],
  ['create-square-rotation-no-upright', {}, drag('square', v(100, 100), v(200, 160), { displayRotation: 90 })],
  ['create-square-flags', {}, drag('square', v(100, 100), v(200, 160), {}, { flags: { noZoom: true, print: false } })],
  ['create-square-preset', { defaults: { cloud: { border: cloudy(2), color: '#0000ff' } } }, drag('square', v(100, 100), v(200, 160), {}, { preset: 'cloud' })],
  ['create-square-clamped', {}, drag('square', v(100, 100), v(900, 900), { pageBox: PAGE })],
  ['create-square-foreign-move', {}, [cr('square', 'down', 100, 100), cr('square', 'move', 200, 200, { page: pageRef(2) }), cr('square', 'up', 200, 200, { page: pageRef(2) })]],
  ['create-square-selects-and-seq', S({ selected: ['sq'], seq: 7 }), drag('square', v(10, 10), v(60, 60))],
  ['create-redact', {}, drag('redact', v(10, 10), v(110, 60))],
  ['create-link', {}, drag('link', v(10, 10), v(110, 60))],
  ['create-link-preset', { defaults: { 'docs-link': { link: URI as never } } }, drag('link', v(10, 10), v(110, 60), {}, { preset: 'docs-link' })],
  ['create-stamp-ignored', {}, drag('stamp', v(10, 10), v(110, 60))],
  ['create-unknown-ignored', {}, drag('mystery', v(10, 10), v(110, 60))],
  ['create-move-without-draft', {}, [cr('square', 'move', 10, 10), cr('square', 'up', 10, 10)]],
  // ── create: free text ──
  ['create-free-text-drag', {}, drag('free-text', v(100, 100), v(300, 160))],
  ['create-free-text-click', {}, drag('free-text', v(100, 100), v(100, 100))],
  ['create-free-text-click-false', {}, drag('free-text', v(100, 100), v(100, 100), {}, { clickCreate: false })],
  ['create-free-text-click-policy', {}, drag('free-text', v(100, 100), v(100, 100), {}, { clickCreate: { width: 90, height: 30, anchor: 'center' } })],
  ['create-free-text-click-segment-policy', {}, drag('free-text', v(100, 100), v(100, 100), {}, { clickCreate: { length: 90 } })],
  ['create-free-text-click-upright-90', {}, drag('free-text', v(100, 100), v(100, 100), { upright: true, displayRotation: 90 })],
  ['create-free-text-click-upright-270-page', {}, drag('free-text', v(5, 5), v(5, 5), { upright: true, displayRotation: 270, pageBox: PAGE })],
  ['create-free-text-drag-upright-270', {}, drag('free-text', v(100, 100), v(300, 160), { upright: true, displayRotation: 270 })],
  ['create-free-text-drag-upright-180', {}, drag('free-text', v(100, 100), v(300, 160), { upright: true, displayRotation: 180 })],
  ['create-free-text-fonts', { defaults: { 'free-text': { fontSize: 22, fontColor: '#ff0000' } } }, drag('free-text', v(100, 100), v(300, 160))],
  // ── create: lines ──
  ['create-line', { defaults: { line: { lineEndings: { end: 'open-arrow' } } } }, drag('line', v(10, 10), v(90, 40))],
  ['create-line-short', {}, drag('line', v(10, 10), v(11, 11))],
  ['create-line-click-policy', {}, drag('line', v(10, 10), v(10, 10), {}, { clickCreate: { length: 100, angleDeg: 90 } })],
  ['create-line-click-box-policy', {}, drag('line', v(10, 10), v(10, 10), {}, { clickCreate: { width: 10, height: 10 } })],
  ['create-line-click-page', {}, drag('line', v(590, 10), v(590, 10), { pageBox: PAGE }, { clickCreate: { length: 100 } })],
  ['create-line-exactly-min', {}, drag('line', v(0, 0), v(3, 0))],
  // ── create: poly ──
  ['create-polygon', {}, [cr('polygon', 'down', 10, 10), cr('polygon', 'up', 10, 10), cr('polygon', 'down', 80, 10), cr('polygon', 'move', 60, 60), cr('polygon', 'down', 40, 70), cr('polygon', 'up', 40, 70), cr('polygon', 'down', 40, 70, { finish: true })]],
  ['create-polygon-too-few', {}, [cr('polygon', 'down', 10, 10), cr('polygon', 'down', 80, 10), cr('polygon', 'down', 80, 10, { finish: true })]],
  ['create-polyline', { defaults: { polyline: { lineEndings: { start: 'circle' } } } }, [cr('polyline', 'down', 20, 20), cr('polyline', 'down', 90, 45), { t: 'finishCreationDraft' }]],
  ['create-polyline-too-few', {}, [cr('polyline', 'down', 20, 20), { t: 'finishCreationDraft' }]],
  ['create-poly-restart-subtype', {}, [cr('polygon', 'down', 10, 10), cr('polyline', 'down', 20, 20), cr('polyline', 'down', 30, 40), { t: 'finishCreationDraft' }]],
  ['create-poly-restart-page', {}, [cr('polygon', 'down', 10, 10), cr('polygon', 'down', 20, 20, { page: pageRef(2) }), cr('polygon', 'move', 30, 30, { page: pageRef(2) })]],
  ['create-poly-flags', {}, [cr('polygon', 'down', 10, 10, {}, { flags: { locked: true } }), cr('polygon', 'down', 90, 10), cr('polygon', 'down', 40, 90), { t: 'finishCreationDraft' }]],
  ['create-poly-finish-without-draft', {}, [cr('polygon', 'down', 10, 10, { finish: true }), { t: 'finishCreationDraft' }]],
  ['create-poly-up-ignored', {}, [cr('polygon', 'down', 10, 10), cr('polygon', 'up', 50, 50)]],
  ['create-poly-clamped', {}, [cr('polygon', 'down', -10, 900, { pageBox: PAGE })]],
  // ── create: ink ──
  ['create-ink', {}, drag('ink', v(10, 10), v(60, 40))],
  ['create-ink-multi-move', {}, [cr('ink', 'down', 10, 10), cr('ink', 'move', 20, 30), cr('ink', 'move', 50, 10), cr('ink', 'up', 50, 10)]],
  ['create-ink-tiny', {}, drag('ink', v(10, 10), v(11, 11))],
  ['create-ink-dot', {}, [cr('ink', 'down', 10, 10), cr('ink', 'up', 10, 10)]],
  ['create-ink-deferred', {}, [cr('ink', 'down', 10, 10, {}, { deferInkCommit: true }), cr('ink', 'move', 60, 10), cr('ink', 'up', 60, 10, {}, { deferInkCommit: true }), cr('ink', 'down', 10, 30), cr('ink', 'move', 60, 32), cr('ink', 'up', 60, 32, {}, { deferInkCommit: true }), { t: 'finishInkDraft' }]],
  ['create-ink-deferred-other-preset', {}, [cr('ink', 'down', 10, 10, {}, { deferInkCommit: true, preset: 'a' }), cr('ink', 'move', 60, 10), cr('ink', 'up', 60, 10, {}, { deferInkCommit: true }), cr('ink', 'down', 10, 30, {}, { preset: 'b' })]],
  ['create-ink-straighten', {}, [cr('ink', 'down', 0, 0), cr('ink', 'move', 50, 2), cr('ink', 'move', 100, 1), cr('ink', 'up', 100, 1, {}, { straightenInk: { deviationThreshold: 0.1, axisSnapDegrees: 10 } })]],
  ['create-ink-intent', { defaults: { 'ink-highlighter': { strokeWidth: 12, blendMode: 'multiply' } } }, drag('ink', v(0, 0), v(100, 3), {}, { preset: 'ink-highlighter', intent: 'ink-highlight', flags: { print: false } })],
  ['create-ink-finish-without-draft', {}, [{ t: 'finishInkDraft' }]],
  ['create-ink-finish-dots', {}, [cr('ink', 'down', 10, 10, {}, { deferInkCommit: true }), cr('ink', 'up', 10, 10, {}, { deferInkCommit: true }), { t: 'finishInkDraft' }]],
  // ── create: callout ──
  ['create-callout-click-box', {}, [cr('free-text-callout', 'down', 10, 10), cr('free-text-callout', 'move', 40, 60), cr('free-text-callout', 'up', 40, 60), cr('free-text-callout', 'down', 60, 70), cr('free-text-callout', 'move', 90, 80), cr('free-text-callout', 'down', 100, 80), cr('free-text-callout', 'up', 100, 80)]],
  ['create-callout-drag-box', {}, [cr('free-text-callout', 'down', 10, 10), cr('free-text-callout', 'down', 60, 70), cr('free-text-callout', 'down', 100, 80), cr('free-text-callout', 'move', 250, 140), cr('free-text-callout', 'up', 250, 140)]],
  ['create-callout-up-early', {}, [cr('free-text-callout', 'down', 10, 10), cr('free-text-callout', 'up', 10, 10), cr('free-text-callout', 'down', 60, 70), cr('free-text-callout', 'up', 60, 70)]],
  ['create-callout-upright-90', { defaults: { 'free-text-callout': { lineEndings: { end: 'closed-arrow' } } } }, [cr('free-text-callout', 'down', 10, 10, { upright: true, displayRotation: 90 }), cr('free-text-callout', 'down', 60, 70), cr('free-text-callout', 'down', 100, 80), cr('free-text-callout', 'move', 250, 140), cr('free-text-callout', 'up', 250, 140)]],
  ['create-callout-upright-180-click', {}, [cr('free-text-callout', 'down', 10, 10, { upright: true, displayRotation: 180 }, { flags: { noRotate: true } }), cr('free-text-callout', 'down', 60, 70), cr('free-text-callout', 'down', 100, 80), cr('free-text-callout', 'up', 100, 80)]],
  ['create-callout-restart-page', {}, [cr('free-text-callout', 'down', 10, 10), cr('free-text-callout', 'down', 60, 70, { page: pageRef(2) })]],
  ['create-callout-move-no-draft', {}, [cr('free-text-callout', 'move', 10, 10), cr('free-text-callout', 'up', 10, 10)]],
  ['create-callout-preset', { defaults: { 'my-callout': { color: '#00ff00' } } }, [cr('free-text-callout', 'down', 10, 10, {}, { preset: 'my-callout' }), cr('free-text-callout', 'down', 60, 70), cr('free-text-callout', 'down', 100, 80), cr('free-text-callout', 'up', 100, 80)]],
  // ── carets, markup, replace-text, preview ──
  ['create-caret', {}, [{ t: 'createCaret', page: pageRef(PON), anchor: { glyphQuad: tq(10, 10, 8, 12), advance: 1 } }]],
  ['create-caret-rot-flags', {}, [{ t: 'createCaret', page: pageRef(PON), anchor: { glyphQuad: tqRot(10, 10, 8, 12, 30), advance: -1 }, flags: { locked: true } }]],
  ['create-caret-nan', {}, [{ t: 'createCaret', page: pageRef(PON), anchor: { glyphQuad: tq(NaN, 10, 8, 12), advance: 1 } }]],
  ['create-markup', S({ preview: { subtype: 'highlight', preset: 'highlight', byPage: {} } }), [{ t: 'createMarkup', subtype: 'highlight', page: pageRef(PON), quads: [tq(10, 10, 100, 12), tq(0, 0, 0, 12), tq(10, 30, 50, 12)] }]],
  ['create-markup-preset-flags', { defaults: { squiggle2: { color: '#123123' } } }, [{ t: 'createMarkup', subtype: 'squiggly', page: pageRef(PON), quads: [tq(10, 10, 100, 12)], preset: 'squiggle2', flags: { print: false } }]],
  ['create-markup-degenerate', {}, [{ t: 'createMarkup', subtype: 'underline', page: pageRef(PON), quads: [tq(0, 0, 0, 12), tq(0, 0, 10, 0)] }]],
  ['create-replace-text', S({ seq: 3 }), [{ t: 'createReplaceText', page: pageRef(PON), quads: [tq(10, 10, 100, 12)], anchor: { glyphQuad: tq(100, 10, 10, 12), advance: 1 } }]],
  ['create-replace-text-preset', { defaults: { rt: { color: '#0000ff' } } }, [{ t: 'createReplaceText', page: pageRef(PON), quads: [tqRot(10, 10, 100, 12, 90)], anchor: { glyphQuad: tqRot(10, 10, 10, 12, 90), advance: -1 }, preset: 'rt' }]],
  ['create-replace-text-degenerate', {}, [{ t: 'createReplaceText', page: pageRef(PON), quads: [], anchor: { glyphQuad: tq(100, 10, 10, 12), advance: 1 } }]],
  ['markup-preview', {}, [{ t: 'setMarkupPreview', subtype: 'highlight', quadsByPage: { 1: [tq(10, 10, 100, 12), tq(0, 0, 0, 1)], 2: [tq(0, 0, 0, 1)], 3: [tq(0, 0, 5, 5)] } }, { t: 'clearMarkupPreview' }, { t: 'clearMarkupPreview' }]],
  ['markup-preview-keys', {}, [{ t: 'setMarkupPreview', subtype: 'underline', preset: 'u', quadsByPage: { b: [tq(0, 0, 5, 5)], 10: [tq(1, 1, 5, 5)], '01': [tq(2, 2, 5, 5)], 1: [tq(3, 3, 5, 5)], '-1': [tq(4, 4, 5, 5)] } as never }]],
  // ── selection ──
  ['select', S(), [{ t: 'select', ids: ['sq', 'hidden', 'ghost', 'hl'] }]],
  ['select-add', S({ selected: ['sq2', 'sq'] }), [{ t: 'select', ids: ['sq', 'ln'], add: true }]],
  ['select-none-valid', S({ selected: ['sq'] }), [{ t: 'select', ids: ['ghost'] }]],
  ['select-group', GROUP, [{ t: 'select', ids: ['m1'] }]],
  ['deselect-all', S({ selected: ['sq', 'ln'] }), [{ t: 'deselect' }, { t: 'deselect' }]],
  ['deselect-ids', S({ selected: ['sq', 'ln'] }), [{ t: 'deselect', ids: ['ln', 'ghost'] }, { t: 'deselect', ids: ['ghost'] }]],
  ['hover', S(), [{ t: 'hover', id: 'sq' }, { t: 'hover', id: 'sq' }, { t: 'hover', id: null }]],
  // ── props, flags, defaults, snap ──
  ['set-props-empty-selection', S(), [{ t: 'setProps', patch: { color: '#000000' } }]],
  ['set-props-mixed', S({ selected: ['sq', 'ln', 'ft', 'hl', 'stamp', 'locked', 'ghost'] }), [{ t: 'setProps', patch: { color: '#000000', lineEndings: { end: 'slash' }, fontSize: 30 } }]],
  ['set-props-key-order', S({ selected: ['sq'] }), [{ t: 'setProps', patch: { strokeWidth: 4, color: '#000000', opacity: 0.5, interiorColor: null } }]],
  ['set-props-unknown-key', S({ selected: ['sq'] }), [{ t: 'setProps', patch: { bogus: 1, color: '#010101' } as never }]],
  ['set-props-nothing-applies', S({ selected: ['sq'] }), [{ t: 'setProps', patch: { fontSize: 30 } }]],
  ['set-props-widget-stays-baked', S({ selected: ['widget'] }), [{ t: 'setProps', patch: { fontSize: 30, color: '#ff0000' } }]],
  ['set-props-link-parent', S({ selected: ['sq', 'ink', 'caret', 'locked'] }), [{ t: 'setProps', patch: { link: URI as never } }]],
  ['set-props-link-parent-clear', S({ selected: ['sq'] }), [{ t: 'setProps', patch: { link: null } }]],
  ['set-props-link-and-color', S({ selected: ['sq'] }), [{ t: 'setProps', patch: { link: URI as never, color: '#0f0f0f' } }]],
  ['set-props-link-kind', S({ selected: ['link'] }), [{ t: 'setProps', patch: { link: { kind: 'goto', destination: { kind: 'fit', page: pageRef(4) } } as never } }]],
  ['set-props-link-kind-null', start([annot('lnk', 'link', gRect(rc(0, 0, 10, 10)), { link: null })], { selected: ['lnk'] }), [{ t: 'setProps', patch: { link: null } }]],
  ['set-props-link-kind-unset-to-null', start([annot('lnk', 'link', gRect(rc(0, 0, 10, 10)))], { selected: ['lnk'] }), [{ t: 'setProps', patch: { link: null } }]],
  ['set-props-icon', S({ selected: ['note'] }), [{ t: 'setProps', patch: { icon: 'key' } }, { t: 'setProps', patch: { icon: 'key' } }]],
  ['set-props-endings-only', S({ selected: ['ln'] }), [{ t: 'setProps', patch: { lineEndings: {} } }]],
  ['set-props-border', S({ selected: ['sq', 'ci'] }), [{ t: 'setProps', patch: { border: DASHED } }, { t: 'setProps', patch: { border: SOLID } }]],
  ['set-flags-selection', S({ selected: ['sq', 'locked', 'ghost'] }), [{ t: 'setFlags', patch: { locked: true } }]],
  ['set-flags-ids', S(), [{ t: 'setFlags', patch: { hidden: true }, ids: ['sq', 'ghost'] }, { t: 'setFlags', patch: { hidden: true }, ids: ['sq'] }]],
  ['set-flags-empty', S(), [{ t: 'setFlags', patch: { hidden: true } }, { t: 'setFlags', patch: { hidden: true }, ids: [] }]],
  ['set-flags-uncommitted', GROUP, [{ t: 'setFlags', patch: { noView: true }, ids: ['tmp:1', 'p'] }]],
  ['set-flags-repeat-id', S(), [{ t: 'setFlags', patch: { locked: true }, ids: ['sq', 'sq'] }]],
  ['set-defaults', {}, [{ t: 'setDefaults', subtype: 'line', patch: { lineEndings: { end: 'open-arrow' }, color: '#00ff00' } }, { t: 'setDefaults', subtype: 'line', patch: { lineEndings: { start: 'circle' } } }, { t: 'setDefaults', subtype: 'line', patch: { color: '#0000ff' } }]],
  ['set-defaults-null', {}, [{ t: 'setDefaults', subtype: 'square', patch: { interiorColor: null, link: null } }]],
  ['set-defaults-keeps-fill', {}, [{ t: 'setDefaults', subtype: 'square', patch: { interiorColor: '#ff0000', link: { kind: 'uri', uri: 'x' } as never } }, { t: 'setDefaults', subtype: 'square', patch: { color: '#000000' } }]],
  ['set-snap', {}, [{ t: 'setSnap', patch: { guides: false, rotationAngles: [45] } }, { t: 'setSnap', patch: {} }]],
  // ── rotation verbs ──
  ['rotate90-single', S({ selected: ['sq'] }), [{ t: 'rotate90' }, { t: 'rotate90' }]],
  ['rotate90-vertex', S({ selected: ['pl'] }), [{ t: 'rotate90' }]],
  ['rotate90-group', S({ selected: ['sq', 'ln', 'stamp', 'hl', 'locked'] }), [{ t: 'rotate90' }]],
  ['rotate90-nothing', S({ selected: ['hl', 'note'] }), [{ t: 'rotate90' }]],
  ['rotate90-stamp', S({ selected: ['stamp'] }), [{ t: 'rotate90' }]],
  ['reset-rotation', S({ selected: ['sq', 'pl', 'locked'] }), [{ t: 'rotate90' }, { t: 'resetRotation' }, { t: 'resetRotation' }]],
  ['reset-rotation-vertex', S({ selected: ['pl'] }), [{ t: 'resetRotation' }]],
  // ── delete, cancel ──
  ['delete', S({ selected: ['sq', 'locked', 'ghost', 'ln'] }), [{ t: 'delete' }, { t: 'delete' }]],
  ['delete-uncommitted', GROUP, [{ t: 'select', ids: ['tmp:1'] }, { t: 'delete' }]],
  ['delete-authority', start([sq('a', rc(0, 0, 10, 10), { authority: { update: true, delete: false } }), sq('b', rc(0, 0, 10, 10), { authority: { update: false, delete: true } })], { selected: ['a', 'b'] }), [{ t: 'delete' }]],
  ['cancel', S(), [cr('square', 'down', 1, 1), { t: 'cancel' }, { t: 'cancel' }]],
  // ── store maintenance ──
  ['loaded', S(), [{ t: 'loaded', annots: [sq('sq', rc(0, 0, 1, 1)), sq('new1', rc(5, 5, 10, 10)), sq('new2', rc(6, 6, 10, 10))] }]],
  ['hydrated-reaps', S({ selected: ['sq', 'ln'], hovered: 'ln', editing: 'ln' }), [{ t: 'hydrated', annots: ANNOTS.filter((a) => a.id !== 'ln' && a.id !== 'sq2').map((a) => (a.id === 'sq' ? { ...a, style: style({ color: '#000000' }) } : a)) }]],
  ['hydrated-bump', GROUP, [{ t: 'hydrated', annots: [sq('p', rc(0, 0, 5, 5)), sq('fresh', rc(1, 1, 5, 5))], bumpAp: true }]],
  ['hydrated-keeps-dragged', S(), [ed('down', 101, 130), ed('move', 131, 170), { t: 'hydrated', annots: [] }]],
  ['hydrated-nothing-reaped', S(), [{ t: 'hydrated', annots: ANNOTS }]],
  ['created', GROUP, [{ t: 'beginTextEdit', id: 'tmp:1' }, { t: 'created', tempId: 'tmp:1', id: 'c1', ref: { kind: 'nm', page: pageRef(PON), nm: 'abc' } }]],
  // The Created ack lands mid-gesture: the draft must follow the temp→durable id swap.
  ['created-mid-move', {}, [...drag('square', v(100, 100), v(200, 160)), ed('down', 150, 130), { t: 'created', tempId: 'tmp:1', id: 'c1', ref: { kind: 'nm', page: pageRef(PON), nm: 'abc' } }, ed('move', 170, 150), ed('up', 170, 150)]],
  ['created-mid-handle', {}, [...drag('square', v(100, 100), v(200, 160)), ed('down', 200, 160), { t: 'created', tempId: 'tmp:1', id: 'c1', ref: { kind: 'nm', page: pageRef(PON), nm: 'abc' } }, ed('move', 220, 180), ed('up', 220, 180)]],
  // More messages that land mid-gesture: the ack under a rotate, the ack then
  // its upsert echo (the durable id stays locked to the drag), a failed
  // create or a remove taking the dragged annotation away, and an ack for an
  // earlier annotation while a polygon is still being drawn.
  ['created-mid-rotate', {}, [...drag('square', v(100, 100), v(200, 160)), ed('down', 150, 76), { t: 'created', tempId: 'tmp:1', id: 'c1', ref: { kind: 'nm', page: pageRef(PON), nm: 'abc' } }, ed('move', 200, 90), ed('up', 200, 90)]],
  ['created-then-echo-mid-move', {}, [...drag('square', v(100, 100), v(200, 160)), ed('down', 150, 130), { t: 'created', tempId: 'tmp:1', id: 'c1', ref: { kind: 'nm', page: pageRef(PON), nm: 'abc' } }, { t: 'upsert', annots: [sq('c1', rc(0, 0, 5, 5))] }, ed('move', 170, 150), ed('up', 170, 150)]],
  ['create-failed-mid-move', {}, [...drag('square', v(100, 100), v(200, 160)), ed('down', 150, 130), { t: 'createFailed', tempId: 'tmp:1' }, ed('move', 170, 150), ed('up', 170, 150)]],
  ['create-failed-mid-handle', {}, [...drag('square', v(100, 100), v(200, 160)), ed('down', 200, 160), { t: 'createFailed', tempId: 'tmp:1' }, ed('move', 220, 180), ed('up', 220, 180)]],
  ['removed-mid-move', S({ selected: ['sq'] }), [ed('down', 140, 130), { t: 'remove', ids: ['sq'] }, ed('move', 170, 170), ed('up', 170, 170)]],
  ['removed-mid-handle', S({ selected: ['sq'] }), [ed('down', 180, 160), { t: 'remove', ids: ['sq'] }, ed('move', 200, 180), ed('up', 200, 180)]],
  ['created-mid-poly-draft', {}, [...drag('square', v(100, 100), v(200, 160)), cr('polygon', 'down', 300, 300), { t: 'created', tempId: 'tmp:1', id: 'c1', ref: { kind: 'nm', page: pageRef(PON), nm: 'abc' } }, cr('polygon', 'move', 350, 300), cr('polygon', 'down', 350, 300), cr('polygon', 'down', 330, 350), cr('polygon', 'down', 330, 350, { finish: true })]],
  ['created-unknown', GROUP, [{ t: 'created', tempId: 'tmp:9', id: 'c1', ref: { kind: 'nm', page: pageRef(PON), nm: 'abc' } }]],
  ['created-index-ref', GROUP, [{ t: 'created', tempId: 'tmp:2', id: 'c2', ref: { kind: 'index', page: pageRef(PON), index: 3, revision: { docSessionId: 's', page: pageRef(PON), generation: 2 } } }]],
  ['create-failed', GROUP, [{ t: 'createFailed', tempId: 'tmp:2' }]],
  ['upsert', S(), [{ t: 'upsert', annots: [sq('sq', rc(0, 0, 1, 1)), sq('brand-new', rc(0, 0, 3, 3))] }, { t: 'upsert', annots: [sq('sq', rc(0, 0, 2, 2))], bumpAp: true }, { t: 'upsert', annots: [sq('sq', rc(0, 0, 2, 2))], bumpAp: true }]],
  ['upsert-while-dragging', S({ selected: ['sq'] }), [ed('down', 180, 160), { t: 'upsert', annots: [sq('sq', rc(0, 0, 1, 1)), sq('sq2', rc(0, 0, 1, 1))] }]],
  ['upsert-while-rotating', S({ selected: ['sq', 'sq2'] }), [ed('down', 240, 76), { t: 'upsert', annots: [sq('sq', rc(0, 0, 1, 1))] }]],
  ['upsert-while-group', S({ selected: ['sq', 'sq2'] }), [ed('down', 380, 160), { t: 'upsert', annots: [sq('sq', rc(0, 0, 1, 1))] }]],
  ['upsert-while-marquee', S(), [mq('down', 0, 0), { t: 'upsert', annots: [sq('sq', rc(0, 0, 1, 1))] }]],
  ['bump-ap', S(), [{ t: 'bumpAp', ids: ['widget', 'ghost', 'widget'] }, { t: 'bumpAp', ids: ['ghost'] }]],
  ['remove', S({ selected: ['sq', 'ln'], hovered: 'sq', editing: 'ft' }), [{ t: 'remove', ids: ['sq', 'ft'] }, { t: 'remove', ids: [] }]],
  ['remove-keeps-hover', S({ hovered: 'ln' }), [cr('square', 'down', 1, 1), { t: 'remove', ids: ['sq'] }]],
  // ── text editing ──
  ['text-edit', S(), [{ t: 'beginTextEdit', id: 'ft' }, { t: 'setText', id: 'ft', text: 'changed' }, { t: 'endTextEdit' }, { t: 'endTextEdit' }]],
  ['text-edit-no-data', S(), [{ t: 'beginTextEdit', id: 'callout' }, { t: 'setText', id: 'callout', text: 'x' }, { t: 'setText', id: 'ghost', text: 'x' }]],
  ['text-edit-locked-contents', S(), [{ t: 'beginTextEdit', id: 'lockedc' }, { t: 'beginTextEdit', id: 'ghost' }, { t: 'beginTextEdit', id: 'locked' }]],
  ['text-edit-vector-already', S(), [{ t: 'setText', id: 'sq', text: 'y' }]],
  ['text-edit-baked', S(), [{ t: 'setText', id: 'baked', text: 'y' }]],
  ['text-edit-drops-draft', S(), [cr('square', 'down', 1, 1), { t: 'beginTextEdit', id: 'ft' }]],
  // ── boundaries the mutation run asked for ──
  ['edit-group-resize-one-axis', S({ selected: ['sq', 'sq2'] }), editDrag(v(380, 130), v(420, 130))],
  ['create-polygon-clears-selection', S({ selected: ['sq'] }), [cr('polygon', 'down', 10, 10)]],
  ['create-callout-clears-selection', S({ selected: ['sq'] }), [cr('free-text-callout', 'down', 10, 10)]],
  ['create-callout-rotation-without-upright', {}, [cr('free-text-callout', 'down', 10, 10, { displayRotation: 90 })]],
  ['create-callout-upright-at-zero', {}, [cr('free-text-callout', 'down', 10, 10, { upright: true, displayRotation: 0 })]],
  ['create-free-text-thin-drag', {}, drag('free-text', v(100, 100), v(200, 101))],
  ['create-free-text-drag-exactly-min', {}, drag('free-text', v(100, 100), v(103, 100))],
  ['create-line-short-offset', {}, drag('line', v(100, 100), v(104, 100))],
  ['create-ink-exactly-min', {}, drag('ink', v(10, 10), v(13, 10))],
  ['create-ink-far-dots', {}, [cr('ink', 'down', 10, 10, {}, { deferInkCommit: true }), cr('ink', 'up', 10, 10, {}, { deferInkCommit: true }), cr('ink', 'down', 90, 90, {}, { deferInkCommit: true }), cr('ink', 'up', 90, 90, {}, { deferInkCommit: true }), { t: 'finishInkDraft' }]],
  ['draft-create-rect-upright-nan', { draft: { g: 'create-rect', subtype: 'square', preset: 'square', page: pageRef(PON), from: v(0, 0), to: v(50, 30), ellipse: false, displayRotation: NaN as never, upright: true } }, [cr('square', 'up', 50, 30)]],
  ['create-markup-degenerate-offset', {}, [{ t: 'createMarkup', subtype: 'underline', page: pageRef(PON), quads: [tq(50, 50, 0, 12), tq(50, 50, 10, 0), tq(50, 60, 0, 0)] }]],
  ['create-replace-text-clears-preview', S({ preview: { subtype: 'highlight', preset: 'highlight', byPage: {} } }), [cr('square', 'down', 1, 1), { t: 'createReplaceText', page: pageRef(PON), quads: [tq(10, 10, 100, 12)], anchor: { glyphQuad: tq(100, 10, 10, 12), advance: 1 } }]],
  ['hydrated-empty-hover', S({ hovered: '', editing: '' }), [{ t: 'hydrated', annots: [] }]],
  ['remove-keeps-editing', S({ editing: 'ft' }), [{ t: 'remove', ids: ['sq'] }]],
  ['hydrated-keeps-surviving-hover', S({ hovered: 'sq', editing: 'ft' }), [{ t: 'hydrated', annots: ANNOTS.filter((a) => a.id !== 'ln') }]],
  ['edit-move-exactly-0.01', start([sq('edge', rc(0, 0, 10, 10))]), [ed('down', 0, 5), ed('move', 0.01, 5), ed('up', 0.01, 5)]],
  ['create-free-text-click-height-exactly-min', {}, drag('free-text', v(100, 100), v(101, 103))],
  // The collinear skew away from the origin, where both edge vectors must be
  // measured from the upper start for the cross product to come out zero.
  ['create-markup-collinear-skew-offset', {}, [{ t: 'createMarkup', subtype: 'highlight', page: pageRef(PON), quads: [{ upperStart: v(10, 10), upperEnd: v(50, 50), lowerStart: v(20, 20), lowerEnd: v(60, 60) }] }]],
  ['create-markup-collinear-skew', {},[{ t: 'createMarkup', subtype: 'highlight', page: pageRef(PON), quads: [{ upperStart: v(0, 0), upperEnd: v(10, 10), lowerStart: v(5, 5), lowerEnd: v(15, 15) }, { upperStart: v(0, 0), upperEnd: v(10, 0), lowerStart: v(0, 0), lowerEnd: v(10, 0) }] }]],
  // ── baked kinds flip to vector on an owned edit ──
  ['baked-handle-flips', S({ selected: ['baked'] }), editDrag(v(510, 540), v(530, 560))],
  ['baked-rotate90-flips', S({ selected: ['baked'] }), [{ t: 'rotate90' }]],
  ['baked-props-flip', S({ selected: ['baked'] }), [{ t: 'setProps', patch: { color: '#000000' } }]],
  ['baked-weird-opaque', start([annot('stamp-ink', 'stamp', gInk([[v(0, 0), v(50, 50)]]), { source: 'baked', apBox: rc(0, 0, 50, 50) })], { selected: ['stamp-ink'] }), [{ t: 'rotate90' }]],
  // ── drafts handed in by the caller (missing ids, no preset) ──
  ['draft-move-missing-member', S({ selected: ['sq'], draft: { g: 'move', ids: ['sq', 'ghost'], start: v(0, 0), delta: v(0, 0), guides: [] } }), [ed('move', 10, 10, { pageBox: PAGE, shift: true })]],
  ['draft-move-all-missing', S({ draft: { g: 'move', ids: ['ghost'], start: v(0, 0), delta: v(0, 0), guides: [] } }), [ed('move', 10, 10, { pageBox: PAGE, shift: true }), ed('move', 10, 10, { pageBox: PAGE, page: pageRef(7), shift: true })]],
  ['draft-move-empty-ids', S({ draft: { g: 'move', ids: [], start: v(0, 0), delta: v(0, 0), guides: [] } }), [ed('move', 10, 10, { page: pageRef(7) })]],
  ['draft-group-missing-member', S({ selected: ['sq', 'sq2'], draft: { g: 'group', op: 'resize', ids: ['sq', 'ghost', 'sq2'], handle: 'se', anchor: v(100, 100), base: rc(100, 100, 280, 60), cur: rc(100, 100, 280, 60) } }), [ed('move', 420, 200), ed('up', 420, 200)]],
  ['draft-group-ghost-first', S({ draft: { g: 'group', op: 'resize', ids: ['ghost', 'sq'], handle: 'se', anchor: v(100, 100), base: rc(100, 100, 80, 60), cur: rc(100, 100, 80, 60) } }), [ed('move', 420, 200), ed('up', 420, 200)]],
  ['draft-rotate-missing-member', S({ draft: { g: 'rotate', ids: ['sq', 'ghost'], pivot: v(140, 130), start: v(140, 76), cur: v(140, 76) } }), [ed('move', 200, 90), ed('up', 200, 90)]],
  ['draft-handle-foreign-annot', S({ draft: { g: 'handle', id: 'ghost', handle: 'se', base: gRect(rc(0, 0, 10, 10)), cur: gRect(rc(0, 0, 10, 10)) } }), [ed('move', 20, 20, { page: pageRef(5) })]],
  ['draft-create-rect-no-preset', { defaults: { square: { color: '#00ff00' } }, draft: { g: 'create-rect', subtype: 'square', page: pageRef(PON), from: v(0, 0), to: v(50, 50), ellipse: false } }, [cr('square', 'up', 50, 50)]],
  ['draft-create-line-no-preset', { draft: { g: 'create-line', subtype: 'line', page: pageRef(PON), from: v(0, 0), to: v(50, 50) } }, [cr('line', 'up', 50, 50)]],
  ['draft-create-ink-no-preset', { draft: { g: 'create-ink', subtype: 'ink', page: pageRef(PON), strokes: [[v(0, 0), v(50, 50)]] } }, [{ t: 'finishInkDraft' }]],
  ['draft-create-poly-no-preset', { draft: { g: 'create-poly', subtype: 'polyline', page: pageRef(PON), points: [v(0, 0), v(50, 50)], cur: v(50, 50), closed: false } }, [{ t: 'finishCreationDraft' }]],
  ['draft-create-callout-no-preset', { draft: { g: 'create-callout', subtype: 'free-text-callout', page: pageRef(PON), step: 'box', tip: v(0, 0), cur: v(50, 50), boxFrom: v(50, 50) } }, [cr('free-text-callout', 'up', 50, 50)]],
  ['draft-create-ink-no-strokes', { draft: { g: 'create-ink', subtype: 'ink', page: pageRef(PON), preset: 'ink', strokes: [] } }, [cr('ink', 'up', 1, 1, {}, { straightenInk: { deviationThreshold: 0.1, axisSnapDegrees: 10 } })]],
  // ── reconcile keeps relationships across the id swap ──
  ['created-relationships', start([
    annot('tmp:1', 'caret', gCaret(rc(10, 10, 5, 5)), { ref: null }),
    annot('r-irt', 'text', gRect(rc(0, 0, 5, 5)), { irt: 'tmp:1' }),
    annot('r-group', 'strikeout', gQuads([tq(0, 0, 5, 5)]), { group: 'tmp:1', irt: 'other' }),
    annot('r-both', 'strikeout', gQuads([tq(0, 0, 5, 5)]), { group: 'tmp:1', irt: 'tmp:1' }),
  ], { selected: ['r-irt', 'tmp:1'] }), [{ t: 'created', tempId: 'tmp:1', id: 'final', ref: { kind: 'objectNumber', page: pageRef(PON), annotObjectNumber: 9 } }]],
  ['set-props-link-null-and-color', S({ selected: ['sq'] }), [{ t: 'setProps', patch: { color: '#0a0a0a', link: null } }]],
  ['marquee-shift-toggle-adds', S({ selected: ['sq'] }), [mq('down', 0, 0), mq('up', 600, 800, { shift: true })]],
  ['create-line-flags', {}, drag('line', v(10, 10), v(90, 40), {}, { flags: { noRotate: true } })],
];

const updateCases: FunctionCase[] = [
  c('MIN_DRAG', 'value'),
  c('initialStyle', 'value'),
  c('initialModel', 'value'),
  c('run', 'single', start([sq('a')]), [ed('down', 101, 130), ed('move', 120, 140), ed('up', 120, 140)]),
];
// Runs that share a page (the same `byId`/`order`) go out as one `runs` case
// per family (the name's first word), so a busy page is written once.
{
  const groups = new Map<string, { page: Partial<Model>; runs: { name: string; over: Partial<Model>; msgs: Msg[] }[] }>();
  for (const [name, s, msgs] of RUNS) {
    const { byId, order, ...over } = s;
    const page = { byId: byId ?? {}, order: order ?? [] };
    const key = `${name.split('-')[0]}|${JSON.stringify(page)}`;
    const group = groups.get(key) ?? { page, runs: [] as { name: string; over: Partial<Model>; msgs: Msg[] }[] };
    group.runs.push({ name, over, msgs });
    groups.set(key, group);
  }
  let n = 0;
  for (const [key, { page, runs }] of groups) {
    const family = key.split('|')[0];
    for (let i = 0; i < runs.length; i += 12) {
      const chunk = runs.slice(i, i + 12);
      updateCases.push(c('runs', `${family}-${n++}-${chunk[0].name}`, page, chunk));
    }
  }
}

// Direct calls.
const rotDraft = (pivot: Vec, s: Vec, cur: Vec, ids: Id[] = ['sq'], over: object = {}) => ({ g: 'rotate', ids, pivot, start: s, cur, ...over });
const RD_START = S();
const RD: [string, unknown][] = [];
for (const [label, d] of [
  ['raw', rotDraft(v(140, 130), v(140, 76), v(200, 90))],
  ['snaps-to-90', rotDraft(v(140, 130), v(140, 76), v(250, 128))],
  ['snap-threshold-edge', rotDraft(v(0, 0), v(0, -10), v(10 * Math.sin((94 * Math.PI) / 180), -10 * Math.cos((94 * Math.PI) / 180)))],
  ['free', rotDraft(v(140, 130), v(140, 76), v(250, 128), ['sq'], { free: true })],
  ['group', rotDraft(v(0, 0), v(0, -10), v(10, -10), ['sq', 'sq2'])],
  ['missing-member', rotDraft(v(0, 0), v(0, -10), v(10, -10), ['ghost'])],
  ['rotated-base', rotDraft(v(0, 0), v(0, -10), v(10, -10), ['pl'])],
  ['view-projected', rotDraft(v(0, 0), v(0, -10), v(10, -10), ['nozoom'], { view: { zoom: 2, rotation: 90 } })],
  ['wraps-negative', rotDraft(v(0, 0), v(10, 0), v(10, 1), ['pl'])],
  ['same-point', rotDraft(v(0, 0), v(0, 0), v(0, 0))],
] as const) {
  RD.push([label, d]);
}
updateCases.push(
  c('rotateDraftDelta', 'raw', RD_START, RD[0][1]),
  c('rotateDraftDeltas', 'page', RD_START, RD.map(([, d]) => d)),
  c('rotateDraftDelta', 'custom-angles', S({ snap: { ...up.initialModel.snap, rotationAngles: [45, -10], rotationThreshold: 10 } }), rotDraft(v(0, 0), v(0, -10), v(10, -10))),
  c('rotateDraftDelta', 'snap-off', S({ snap: { ...up.initialModel.snap, rotation: false } }), rotDraft(v(0, 0), v(0, -10), v(10, -10))),
);
const calloutDraft = (over: object = {}) => ({ g: 'create-callout', subtype: 'free-text-callout', page: pageRef(PON), step: 'box', tip: v(10, 10), knee: v(50, 60), cur: v(100, 80), ...over });
for (const [label, d] of [
  ['hover', calloutDraft()],
  ['pressed', calloutDraft({ boxFrom: v(100, 80) })],
  ['pressed-no-to', calloutDraft({ boxFrom: v(100, 80), boxTo: undefined })],
  ['dragged', calloutDraft({ boxFrom: v(100, 80), boxTo: v(300, 140) })],
  ['dragged-tiny', calloutDraft({ boxFrom: v(100, 80), boxTo: v(101, 81) })],
  ['dragged-reversed', calloutDraft({ boxFrom: v(300, 140), boxTo: v(100, 80) })],
  ['dragged-thin', calloutDraft({ boxFrom: v(100, 80), boxTo: v(300, 81) })],
  ['upright-nan', calloutDraft({ upright: true, displayRotation: NaN })],
  ['upright-zero', calloutDraft({ upright: true, displayRotation: 0 })],
  ...[0, 90, 180, 270, 45].flatMap((r) => [
    [`upright-${r}-hover`, calloutDraft({ upright: true, displayRotation: r })],
    [`upright-${r}-dragged`, calloutDraft({ upright: true, displayRotation: r, boxFrom: v(100, 80), boxTo: v(300, 140) })],
    [`upright-${r}-pressed`, calloutDraft({ upright: true, displayRotation: r, boxFrom: v(100, 80) })],
  ]),
  ['rotation-without-upright', calloutDraft({ displayRotation: 90 })],
  ['page-inside', calloutDraft({ pageBox: rc(0, 0, 600, 800) })],
  ['page-right-edge', calloutDraft({ cur: v(500, 80), pageBox: rc(0, 0, 600, 800) })],
  ['page-bottom-edge', calloutDraft({ cur: v(100, 790), pageBox: rc(0, 0, 600, 800) })],
  ['page-upright-90', calloutDraft({ cur: v(590, 790), upright: true, displayRotation: 90, pageBox: rc(0, 0, 600, 800) })],
  ['page-dragged-ignores', calloutDraft({ boxFrom: v(500, 700), boxTo: v(700, 900), pageBox: rc(0, 0, 600, 800) })],
] as const) {
  updateCases.push(c('calloutBox', label as string, d), c('calloutUprightRot', label as string, d));
}
const BOXES: unknown[][] = [];
for (const [, a, b, inert, view] of [
  ['box', v(90, 90), v(200, 200)],
  ['reversed', v(200, 200), v(90, 90)],
  ['everything', v(0, 0), v(600, 800)],
  ['inert', v(0, 0), v(600, 800), new Set(['sq', 'ln'])],
  ['view', v(540, 690), v(570, 710), undefined, { zoom: 2, rotation: 0 }],
  ['touching-edge', v(180, 160), v(190, 170)],
  ['point', v(140, 130), v(140, 130)],
] as const) {
  BOXES.push([PON, a, b, inert, view]);
}
BOXES.push([2, v(0, 0), v(600, 800)]);
updateCases.push(c('annotsInBox', 'box', S(), PON, v(90, 90), v(200, 200)));
updateCases.push(c('annotsInBoxes', 'page', S(), BOXES));
updateCases.push(c('annotsInBox', 'groups', GROUP, PON, v(0, 0), v(600, 800)));

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'core/annotation-update.json',
    describes: 'packages/core/annotation update.ts: message runs through update (identity, effects, draft and selection per step, final model), plus rotateDraftDelta, calloutBox, calloutUprightRot and annotsInBox.',
    functions: updateFns,
    cases: updateCases,
  },
];

export type { Annot };
