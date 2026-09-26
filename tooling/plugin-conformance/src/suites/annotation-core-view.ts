/**
 * `packages/core/annotation` view.ts: the render list, the selection chrome
 * and the selection anchors. Replayed by
 * `crates/plugins/annotation-core/tests/conformance.rs`.
 *
 * The interesting states are mid-gesture, so each model here is DRIVEN: a
 * start run through `update` messages at generation time, and the resulting
 * model (draft included) is the case argument. `viewAll` evaluates every
 * page selector on one model and view, so a gesture state is one case.
 * `selectionAnchor` takes three lookups; its wrapper takes them as tables
 * keyed by page object number (a missing key is `undefined`).
 */
import * as vw from '../../../../packages/core/annotation/src/view';
import { update } from '../../../../packages/core/annotation/src/update';
import type { ViewEnv } from '../../../../packages/core/annotation/src/anchor';
import type { Annot, Model, Msg, Rect, Vec } from '../../../../packages/core/annotation/src/types';

import type { FunctionCase, Suite } from '../vectors';
import {
  PON,
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
  v,
  type Fn,
  pageRef,
} from './annotation-core-fixtures';

type Table<T> = Record<string, T>;

export const viewFns: Record<string, Fn> = {
  pageItems: (m: Partial<Model>, pon: number, view?: ViewEnv) => vw.pageItems(modelOf(m), pageRef(pon), view),
  chrome: (m: Partial<Model>, pon: number, pageBox?: Rect, knobOffset?: number, view?: ViewEnv) =>
    vw.chrome(modelOf(m), pageRef(pon), pageBox, knobOffset, view),
  selectedItems: (m: Partial<Model>, view?: ViewEnv) => vw.selectedItems(modelOf(m), view),
  textBoxes: (m: Partial<Model>, pon: number, view?: ViewEnv) => vw.textBoxes(modelOf(m), pageRef(pon), view),
  selectionBoundsOnPage: (m: Partial<Model>, pon: number, view?: ViewEnv) =>
    vw.selectionBoundsOnPage(modelOf(m), pageRef(pon), view),
  selectionKnob: (m: Partial<Model>, pon: number, pageBox?: Rect, knobOffset?: number, view?: ViewEnv) =>
    vw.selectionKnob(modelOf(m), pageRef(pon), pageBox, knobOffset, view),
  creationDraftAnchor: (m: Partial<Model>) => vw.creationDraftAnchor(modelOf(m)),
  selectionAnchor: (
    m: Partial<Model>,
    pageBoxes?: Table<Rect>,
    knobOffsets?: Table<number>,
    views?: Table<ViewEnv>,
  ) =>
    vw.selectionAnchor(
      modelOf(m),
      pageBoxes && ((page) => pageBoxes[String(page.pageObjectNumber)]),
      knobOffsets && ((page) => knobOffsets[String(page.pageObjectNumber)]),
      views && ((page) => views[String(page.pageObjectNumber)]),
    ),
  /** One page, many states: each call layers `over` on the page and runs
   *  `viewAll` with its own page, box, offset and view. */
  viewMany: (
    page: Partial<Model>,
    calls: { name: string; over: Partial<Model>; pon: number; pageBox?: Rect; knobOffset?: number; view?: ViewEnv }[],
  ) =>
    calls.map(({ name, over, pon, pageBox, knobOffset, view }) => ({
      name,
      ...(viewFns.viewAll as (...a: unknown[]) => object)({ ...page, ...over }, pon, pageBox, knobOffset, view),
    })),
  selectionAnchors: (
    page: Partial<Model>,
    calls: { name: string; over: Partial<Model>; pageBoxes?: Table<Rect>; knobOffsets?: Table<number>; views?: Table<ViewEnv> }[],
  ) =>
    calls.map(({ name, over, pageBoxes, knobOffsets, views }) => ({
      name,
      anchor: (viewFns.selectionAnchor as (...a: unknown[]) => unknown)({ ...page, ...over }, pageBoxes, knobOffsets, views),
    })),
  viewAll: (m: Partial<Model>, pon: number, pageBox?: Rect, knobOffset?: number, view?: ViewEnv) => {
    const model = modelOf(m);
    return {
      pageItems: vw.pageItems(model, pageRef(pon), view),
      chrome: vw.chrome(model, pageRef(pon), pageBox, knobOffset, view),
      selectedItems: vw.selectedItems(model, view),
      textBoxes: vw.textBoxes(model, pageRef(pon), view),
      selectionBoundsOnPage: vw.selectionBoundsOnPage(model, pageRef(pon), view),
      selectionKnob: vw.selectionKnob(model, pageRef(pon), pageBox, knobOffset, view),
      creationDraftAnchor: vw.creationDraftAnchor(model),
    };
  },
};

/** Run messages from a start; the model reached (a full `Model`). */
export const drive = (s: Partial<Model>, msgs: Msg[]): Model =>
  msgs.reduce<Model>((m, msg) => update(m, msg)[0], modelOf(s));

const PAGE = rc(0, 0, 600, 800);
const ptr = (phase: 'down' | 'move' | 'up', x: number, y: number, over: object = {}): Msg => ({
  t: 'editPointer',
  phase,
  in: { page: pageRef(PON), point: v(x, y), shift: false, ...over },
});
const create = (subtype: string, phase: 'down' | 'move' | 'up', x: number, y: number, over: object = {}, msgOver: object = {}): Msg =>
  ({
    t: 'createPointer',
    phase,
    subtype,
    in: { page: pageRef(PON), point: v(x, y), shift: false, ...over },
    ...msgOver,
  }) as Msg;

const TEXT = { fontFamily: 'helvetica', fontSize: 12, fontColor: '#000000', textAlign: 'left' as const };
const VIEW_ANNOTS: Annot[] = [
  annot('hl', 'highlight', gQuads([tq(10, 400, 200, 14)]), { style: style({ blendMode: 'multiply' }) }),
  sq('sq', rc(100, 100, 80, 60)),
  sq('baked', rc(300, 100, 80, 60), { source: 'baked', apBox: rc(298, 98, 84, 64) }),
  sq('baked-rot', rc(300, 300, 80, 60), { source: 'baked', apBox: rc(300, 300, 80, 60), apRot: 30, geom: gRect(rc(300, 300, 80, 60), false, 30) }),
  annot('ci', 'circle', gRect(rc(100, 250, 80, 60), true), { style: style({ border: cloudy(1) }) }),
  annot('ln', 'line', gLine(v(300, 250), v(400, 300), ends('none', 'closed-arrow'))),
  annot('pl', 'polyline', gPoly([v(300, 500), v(350, 560), v(400, 500)], false, ends('open-arrow', 'none'), 15)),
  annot('ink', 'ink', gInk([[v(450, 100), v(500, 150), v(550, 100)]])),
  annot('ft', 'free-text', gText(rc(450, 250, 100, 40)), { text: TEXT }),
  annot('ft-baked', 'free-text', gText(rc(450, 320, 100, 40), undefined, 90), { text: TEXT, source: 'baked', apBox: rc(480, 290, 40, 100) }),
  annot('callout', 'free-text', gText(rc(200, 600, 100, 40), { tip: v(100, 550), knee: v(150, 560), ending: 'open-arrow' }), { text: TEXT, source: 'baked', apBox: rc(95, 545, 210, 100) }),
  annot('stamp', 'stamp', gRect(rc(450, 400, 80, 40), false, 30), { source: 'baked', apBox: rc(450, 400, 80, 40), apRot: 30 }),
  annot('note', 'text', gRect(rc(20, 20, 20, 20)), { source: 'baked', apBox: rc(20, 20, 20, 20) }),
  annot('caret', 'caret', gCaret(rc(250, 400, 6, 6), 20)),
  annot('link', 'link', gRect(rc(20, 700, 100, 20))),
  annot('redact', 'redact', gRect(rc(300, 650, 100, 50)), { label: { text: 'X', repeat: true }, text: TEXT }),
  sq('locked', rc(100, 650, 60, 40), { flags: flags({ locked: true }) }),
  sq('hidden', rc(200, 650, 60, 40), { flags: flags({ hidden: true }) }),
  sq('noview', rc(200, 720, 60, 40), { flags: flags({ noView: true, toggleNoView: true }) }),
  annot('hidden-ft', 'free-text', gText(rc(0, 0, 50, 20)), { flags: flags({ hidden: true }), source: 'vector' }),
  annot('reply', 'text', gRect(rc(100, 100, 20, 20)), { irt: 'sq' }),
  annot('nozoom', 'square', gLine(v(0, 0), v(0, 0)), { geom: gRect(rc(560, 700, 30, 30)), flags: flags({ noZoom: true, noRotate: true }), source: 'baked', apBox: rc(560, 700, 30, 30), apRot: 90 }),
  annot('nozoom-ln', 'line', gLine(v(500, 700), v(550, 720)), { flags: flags({ noZoom: true }) }),
  sq('other-page', rc(100, 100, 80, 60), { page: pageRef(2) }),
];
const S = (over: Partial<Model> = {}) => start(VIEW_ANNOTS, over);

/** Named states: a start plus the messages that drive it. */
const STATES: [string, Partial<Model>][] = [
  ['idle', S()],
  ['hover-redact', S({ hovered: 'redact' })],
  ['editing-ft', S({ editing: 'ft', selected: ['ft'] })],
  ['editing-callout', S({ editing: 'callout', selected: ['callout'] })],
  ['editing-baked-square', S({ editing: 'baked', selected: ['baked'] })],
  ['sel-sq', S({ selected: ['sq'] })],
  ['sel-baked-rot', S({ selected: ['baked-rot'] })],
  ['sel-pl', S({ selected: ['pl'] })],
  ['sel-caret', S({ selected: ['caret'] })],
  ['sel-hl', S({ selected: ['hl'] })],
  ['sel-locked', S({ selected: ['locked'] })],
  ['sel-callout', S({ selected: ['callout'] })],
  ['sel-note', S({ selected: ['note'] })],
  ['sel-ft-baked', S({ selected: ['ft-baked'] })],
  ['sel-noview', S({ selected: ['noview'] })],
  ['sel-hidden', S({ selected: ['hidden'] })],
  ['sel-nozoom', S({ selected: ['nozoom'] })],
  ['sel-group', S({ selected: ['sq', 'ln', 'ink'] })],
  ['sel-group-with-note', S({ selected: ['sq', 'note'] })],
  ['sel-group-with-hl', S({ selected: ['sq', 'hl'] })],
  ['sel-cross-page', S({ selected: ['other-page', 'sq'] })],
  ['sel-other-page', S({ selected: ['other-page'] })],
  ['sel-missing', S({ selected: ['ghost', 'sq'] })],
  ['sel-group-plus-other-page', S({ selected: ['sq', 'ln', 'other-page', 'ghost'] })],
  ['draft-rotate-unrotatable', S({ selected: ['hl'], draft: { g: 'rotate', ids: ['hl'], pivot: v(100, 400), start: v(100, 380), cur: v(120, 380) } })],
  ['draft-handle-text-to-rect', S({ selected: ['ft'], draft: { g: 'handle', id: 'ft', handle: 'se', base: gText(rc(450, 250, 100, 40)), cur: gRect(rc(450, 250, 120, 50)) } })],
  ['draft-create-rect-no-preset', S({ defaults: { square: { color: '#00ff00' } }, draft: { g: 'create-rect', subtype: 'square', page: pageRef(PON), from: v(0, 0), to: v(50, 50), ellipse: false } })],
  ['draft-create-callout-no-preset', S({ draft: { g: 'create-callout', subtype: 'free-text-callout', page: pageRef(PON), step: 'box', tip: v(0, 0), cur: v(50, 50), boxFrom: v(50, 50) } })],
  ['weird-opaque', start([annot('stamp-ink', 'stamp', gInk([[v(0, 0), v(50, 50)]]), { source: 'baked', apBox: rc(0, 0, 50, 50) })], { selected: ['stamp-ink'] })],
  ['move-sq', drive(S(), [ptr('down', 140, 101), ptr('move', 170, 131, { pageBox: PAGE })])],
  ['move-baked', drive(S(), [ptr('down', 301, 130), ptr('move', 321, 150)])],
  ['move-group-guides', drive(S({ selected: ['sq', 'ln'] }), [ptr('down', 140, 130), ptr('move', 142, 132)])],
  ['move-snap-guides', drive(S({ selected: ['sq'] }), [ptr('down', 140, 130), ptr('move', 339, 131)])],
  ['move-stamp', drive(S({ selected: ['stamp'] }), [ptr('down', 490, 420), ptr('move', 400, 420)])],
  ['handle-sq', drive(S({ selected: ['sq'] }), [ptr('down', 180, 160), ptr('move', 220, 200)])],
  ['handle-baked', drive(S({ selected: ['baked'] }), [ptr('down', 380, 160), ptr('move', 400, 180)])],
  ['handle-stamp', drive(S({ selected: ['stamp'] }), [ptr('down', 490, 420), ptr('up', 490, 420), ptr('down', 0, 0)])],
  ['handle-callout-tip', drive(S({ selected: ['callout'] }), [ptr('down', 100, 550), ptr('move', 80, 500)])],
  ['rotate-sq', drive(S({ selected: ['sq'] }), [ptr('down', 140, 76), ptr('move', 200, 90)])],
  ['rotate-sq-snapped', drive(S({ selected: ['sq'] }), [ptr('down', 140, 76), ptr('move', 250, 128)])],
  ['rotate-sq-free', drive(S({ selected: ['sq'] }), [ptr('down', 140, 76), ptr('move', 250, 128, { shift: true })])],
  ['rotate-sq-page', drive(S({ selected: ['sq'] }), [ptr('down', 140, 76, { pageBox: PAGE }), ptr('move', 200, 90, { pageBox: PAGE })])],
  ['rotate-group', drive(S({ selected: ['sq', 'ln', 'ink'] }), [ptr('down', 325, 76), ptr('move', 400, 76)])],
  ['rotate-baked', drive(S({ selected: ['baked'] }), [ptr('down', 340, 76), ptr('move', 400, 100)])],
  ['group-resize', drive(S({ selected: ['sq', 'ln', 'ink'] }), [ptr('down', 550, 300), ptr('move', 600, 350)])],
  ['group-resize-rotated', drive(S({ selected: ['sq', 'pl'] }), [ptr('down', 406, 568), ptr('move', 500, 600)])],
  ['marquee', drive(S(), [{ t: 'marqueePointer', phase: 'down', in: { page: pageRef(PON), point: v(90, 90), shift: false } }, { t: 'marqueePointer', phase: 'move', in: { page: pageRef(PON), point: v(200, 200), shift: false } }])],
  ['create-square', drive(S(), [create('square', 'down', 10, 10), create('square', 'move', 60, 40)])],
  ['create-square-zero', drive(S(), [create('square', 'down', 10, 10)])],
  ['create-square-line-drag', drive(S(), [create('square', 'down', 10, 10), create('square', 'move', 60, 10)])],
  ['create-circle-cloudy', drive(S({ defaults: { circle: { border: cloudy(1) } } }), [create('circle', 'down', 10, 10), create('circle', 'move', 60, 40)])],
  ['create-line', drive(S({ defaults: { line: { lineEndings: { end: 'open-arrow' } } } }), [create('line', 'down', 10, 10), create('line', 'move', 60, 40)])],
  ['create-arrow-preset', drive(S({ defaults: { arrow: { lineEndings: { end: 'closed-arrow' }, color: '#00f' } } }), [create('line', 'down', 10, 10, {}, { preset: 'arrow' }), create('line', 'move', 60, 40)])],
  ['create-polygon', drive(S(), [create('polygon', 'down', 10, 10), create('polygon', 'down', 60, 10), create('polygon', 'move', 30, 50)])],
  ['create-polygon-hover-same', drive(S(), [create('polygon', 'down', 10, 10), create('polygon', 'down', 60, 10), create('polygon', 'move', 60, 10)])],
  ['create-polyline', drive(S(), [create('polyline', 'down', 10, 10), create('polyline', 'move', 30, 50)])],
  ['create-ink', drive(S(), [create('ink', 'down', 10, 10), create('ink', 'move', 30, 50), create('ink', 'move', 50, 20)])],
  ['create-callout-knee', drive(S(), [create('free-text-callout', 'down', 10, 10), create('free-text-callout', 'move', 60, 70)])],
  ['create-callout-box', drive(S(), [create('free-text-callout', 'down', 10, 10), create('free-text-callout', 'down', 60, 70), create('free-text-callout', 'move', 100, 80)])],
  ['create-callout-box-drag', drive(S(), [create('free-text-callout', 'down', 10, 10), create('free-text-callout', 'down', 60, 70), create('free-text-callout', 'down', 100, 80), create('free-text-callout', 'move', 200, 140)])],
  ['create-polygon-hover-same-y', drive(S(), [create('polygon', 'down', 10, 10), create('polygon', 'down', 60, 10), create('polygon', 'move', 80, 10)])],
  ['create-square-height-drag', drive(S(), [create('square', 'down', 10, 10), create('square', 'move', 10, 60)])],
  ['create-callout-other-page', drive(S(), [create('free-text-callout', 'down', 10, 10, { page: pageRef(2) }), create('free-text-callout', 'move', 60, 70, { page: pageRef(2) })])],
  ['create-callout-upright', drive(S({ defaults: { 'free-text-callout': { lineEndings: { end: 'circle' } } } }), [create('free-text-callout', 'down', 10, 10, { upright: true, displayRotation: 90 }), create('free-text-callout', 'down', 60, 70), create('free-text-callout', 'move', 100, 80)])],
  ['create-on-page2', drive(S(), [create('square', 'down', 10, 10, { page: pageRef(2) }), create('square', 'move', 60, 40, { page: pageRef(2) })])],
  ['markup-preview', drive(S(), [{ t: 'setMarkupPreview', subtype: 'highlight', quadsByPage: { [PON]: [tq(10, 10, 100, 12)], 2: [tq(0, 0, 5, 5)] } }])],
  ['markup-preview-empty-page', drive(S(), [{ t: 'setMarkupPreview', subtype: 'underline', preset: 'u2', quadsByPage: { 2: [tq(0, 0, 5, 5)] } }])],
];

const VIEWS: [string, ViewEnv | undefined][] = [
  ['noview', undefined],
  ['z2-r90', { zoom: 2, rotation: 90 }],
];

const viewCases: FunctionCase[] = [];
// States that share a page (the same `byId`/`order`) go out as one `viewMany`
// case per view, so the page is written once.
{
  const groups = new Map<string, { page: Partial<Model>; calls: unknown[] }>();
  for (const [name, m] of STATES) {
    for (const [vname, view] of VIEWS) {
      if (vname !== 'noview' && !/nozoom|idle|sel-sq|rotate-sq$|move-sq/.test(name)) continue;
      const { byId, order, ...over } = m;
      const page = { byId: byId ?? {}, order: order ?? [] };
      const key = `${vname}|${JSON.stringify(page)}`;
      const group = groups.get(key) ?? { page, calls: [] as unknown[] };
      group.calls.push({ name, over, pon: PON, view });
      groups.set(key, group);
    }
  }
  let n = 0;
  for (const [key, { page, calls }] of groups) {
    for (let i = 0; i < calls.length; i += 10) {
      viewCases.push(c('viewMany', `${key.split('|')[0]}-${n++}`, page, calls.slice(i, i + 10)));
    }
  }
}
viewCases.push(
  c('viewAll', 'sel-sq-page-knob40', S({ selected: ['sq'] }), PON, rc(0, 90, 600, 800), 40),
  c('viewAll', 'rotate-sq-page2', STATES.find(([n]) => n === 'rotate-sq')![1], 2),
  c('viewAll', 'move-sq-page2', STATES.find(([n]) => n === 'move-sq')![1], 2),
  c('viewAll', 'marquee-page2', STATES.find(([n]) => n === 'marquee')![1], 2),
  c('viewAll', 'other-page', S({ selected: ['other-page'] }), 2),
  c('viewAll', 'nozoom-z0.5', S({ selected: ['nozoom'] }), PON, undefined, undefined, { zoom: 0.5, rotation: 0 }),
  c('viewAll', 'nozoom-z3-r270', S({ selected: ['nozoom-ln'] }), PON, undefined, undefined, { zoom: 3, rotation: 270 }),
  c('pageItems', 'idle', S(), PON),
  c('chrome', 'sel-sq-default-offset', S({ selected: ['sq'] }), PON),
  c('selectedItems', 'group', S({ selected: ['sq', 'ghost', 'ln'] })),
  c('textBoxes', 'editing', S({ editing: 'ft' }), PON),
  c('selectionBoundsOnPage', 'none', S(), PON),
  c('selectionKnob', 'sel-sq', S({ selected: ['sq'] }), PON),
  c('creationDraftAnchor', 'none', S()),
  c('creationDraftAnchor', 'polygon', STATES.find(([n]) => n === 'create-polygon')![1]),
  c('creationDraftAnchor', 'polyline-one', STATES.find(([n]) => n === 'create-polyline')![1]),
  c('creationDraftAnchor', 'empty-points', { draft: { g: 'create-poly', subtype: 'polygon', page: pageRef(PON), points: [], cur: v(0, 0), closed: true } }),
);
viewCases.push(
  c(
    'selectionAnchors',
    'states',
    S(),
    STATES.filter(([n]) => /^sel-|rotate-sq$|idle/.test(n)).flatMap(([name, m]) => {
      const { byId: _b, order: _o, ...over } = m;
      return [
        { name: `${name}-bare`, over },
        { name: `${name}-tables`, over, pageBoxes: { [PON]: rc(0, 90, 600, 800) }, knobOffsets: { [PON]: 40 }, views: { [PON]: { zoom: 2, rotation: 90 } } },
      ];
    }),
  ),
);
viewCases.push(
  c('selectionAnchor', 'empty-tables', S({ selected: ['sq'] }), {}, {}, {}),
  c('selectionAnchor', 'unselectable-first', S({ selected: ['hidden', 'sq'] })),
  c('selectionAnchor', 'nothing-selectable', S({ selected: ['hidden'] })),
);

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'core/annotation-view.json',
    describes: 'packages/core/annotation view.ts: pageItems, chrome, selectedItems, textBoxes, selection bounds, knob and anchors, over idle, selected and mid-gesture models.',
    functions: viewFns,
    cases: viewCases,
  },
];

export type { Vec };
