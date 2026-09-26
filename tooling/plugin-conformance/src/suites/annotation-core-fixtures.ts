/**
 * Shared builders for the `packages/core/annotation` suites
 * (`annotation-core-*.ts`). This module writes no vector file of its own; the
 * generator requires every module here to export `suites`, so it exports an
 * empty list.
 */
import { DRAWN_FLAGS, NO_ANNOTATION_FLAGS } from '../../../../packages/core/annotation/src/flags';
import { initialModel, initialStyle } from '../../../../packages/core/annotation/src/update';
import type {
  Annot,
  Border,
  Callout,
  Geom,
  LineEnding,
  LineEndings,
  Model,
  Rect,
  Style,
  TextQuad,
  Vec,
} from '../../../../packages/core/annotation/src/types';
import type { AnnotationFlags } from '../../../../packages/core/annotation/src/flags';

import type { FunctionCase, Suite } from '../vectors';

export const suites: Suite[] = [];

export type Fn = (...args: never[]) => unknown;

/** A case whose name is `<fn>/<label>`. */
export const c = (fn: string, label: string, ...args: unknown[]): FunctionCase => ({
  name: `${fn}/${label}`,
  fn,
  args,
});

export const v = (x: number, y: number): Vec => ({ x, y });
export const rc = (x: number, y: number, width: number, height: number): Rect => ({
  x,
  y,
  width,
  height,
});
export const tq = (x: number, y: number, width: number, height: number): TextQuad => ({
  upperStart: v(x, y),
  upperEnd: v(x + width, y),
  lowerStart: v(x, y + height),
  lowerEnd: v(x + width, y + height),
});
/** A text quad turned `deg` clockwise about its upper-start corner. */
export const tqRot = (x: number, y: number, width: number, height: number, deg: number): TextQuad => {
  const r = (deg * Math.PI) / 180;
  const cos = Math.cos(r);
  const sin = Math.sin(r);
  const at = (dx: number, dy: number) => v(x + dx * cos - dy * sin, y + dx * sin + dy * cos);
  return {
    upperStart: at(0, 0),
    upperEnd: at(width, 0),
    lowerStart: at(0, height),
    lowerEnd: at(width, height),
  };
};

export const ENDINGS: LineEnding[] = [
  'none',
  'square',
  'circle',
  'diamond',
  'open-arrow',
  'closed-arrow',
  'butt',
  'r-open-arrow',
  'r-closed-arrow',
  'slash',
];
export const ends = (start: LineEnding, end: LineEnding): LineEndings => ({ start, end });

export const SOLID: Border = { kind: 'solid' };
export const DASHED: Border = { kind: 'dashed', dash: [3, 2] };
export const cloudy = (intensity: number): Border => ({ kind: 'cloudy', intensity });

// ── geoms ─────────────────────────────────────────────────────────────────────

export const gRect = (r: Rect, ellipse = false, rot?: number): Geom =>
  rot === undefined ? { t: 'rect', rect: r, ellipse } : { t: 'rect', rect: r, ellipse, rot };
export const gLine = (a: Vec, b: Vec, e?: LineEndings, rot?: number): Geom => {
  const g: Geom = { t: 'line', a, b };
  if (e) g.ends = e;
  if (rot !== undefined) g.rot = rot;
  return g;
};
export const gPoly = (points: Vec[], closed: boolean, e?: LineEndings, rot?: number): Geom => {
  const g: Geom = { t: 'poly', points, closed };
  if (e) g.ends = e;
  if (rot !== undefined) g.rot = rot;
  return g;
};
export const gQuads = (quads: TextQuad[]): Geom => ({ t: 'quads', quads });
export const gCaret = (r: Rect, rot?: number): Geom =>
  rot === undefined ? { t: 'caret', rect: r } : { t: 'caret', rect: r, rot };
export const gInk = (strokes: Vec[][], rot?: number): Geom =>
  rot === undefined ? { t: 'ink', strokes } : { t: 'ink', strokes, rot };
export const gText = (r: Rect, callout?: Callout, rot?: number): Geom => {
  const g: Geom = { t: 'text', rect: r };
  if (callout) g.callout = callout;
  if (rot !== undefined) g.rot = rot;
  return g;
};

const TRI = [v(10, 10), v(90, 20), v(40, 80)];
const ZIG = [v(0, 0), v(40, 30), v(80, 0), v(120, 30)];

/** One of every geometry shape the union allows, rotated and degenerate ones
 *  included: the input set the per-geom functions are crossed with. */
export const GEOMS: [string, Geom][] = [
  ['rect', gRect(rc(100, 100, 80, 60))],
  ['rect-rot0', gRect(rc(100, 100, 80, 60), false, 0)],
  ['rect-rot30', gRect(rc(100, 100, 80, 60), false, 30)],
  ['rect-rot90', gRect(rc(100, 100, 80, 60), false, 90)],
  ['rect-rot180', gRect(rc(100, 100, 80, 60), false, 180)],
  ['rect-rot270', gRect(rc(100, 100, 80, 60), false, 270)],
  ['rect-zero', gRect(rc(50, 50, 0, 0))],
  ['rect-inverted', gRect(rc(50, 50, -20, -10))],
  ['rect-thin', gRect(rc(0, 0, 200, 3))],
  ['ellipse', gRect(rc(10, 20, 100, 50), true)],
  ['ellipse-rot45', gRect(rc(10, 20, 100, 50), true, 45)],
  ['ellipse-zero', gRect(rc(10, 20, 0, 50), true)],
  ['line', gLine(v(10, 10), v(90, 40))],
  ['line-arrows', gLine(v(10, 10), v(90, 40), ends('open-arrow', 'closed-arrow'))],
  ['line-circle-square', gLine(v(0, 0), v(0, 100), ends('circle', 'square'))],
  ['line-diamond-slash', gLine(v(100, 0), v(0, 0), ends('diamond', 'slash'))],
  ['line-butt-rarrows', gLine(v(5, 5), v(60, 80), ends('butt', 'r-open-arrow'))],
  ['line-rclosed-none', gLine(v(5, 5), v(60, 80), ends('r-closed-arrow', 'none'))],
  ['line-zero', gLine(v(30, 30), v(30, 30), ends('closed-arrow', 'circle'))],
  ['line-rot25', gLine(v(10, 10), v(90, 40), ends('none', 'open-arrow'), 25)],
  ['poly-closed', gPoly(TRI, true)],
  ['poly-closed-rot60', gPoly(TRI, true, undefined, 60)],
  ['poly-open', gPoly(ZIG, false, ends('open-arrow', 'closed-arrow'))],
  ['poly-open-noends', gPoly(ZIG, false)],
  ['poly-open-rot15', gPoly(ZIG, false, ends('square', 'diamond'), 15)],
  ['poly-single', gPoly([v(5, 5)], false, ends('circle', 'circle'))],
  ['poly-empty', gPoly([], true)],
  ['poly-two-closed', gPoly([v(0, 0), v(10, 10)], true)],
  ['poly-collinear', gPoly([v(0, 0), v(50, 0), v(100, 0)], false, ends('butt', 'butt'))],
  ['poly-hairpin', gPoly([v(0, 0), v(100, 0), v(0, 0.0000000001)], false)],
  ['poly-spike', gPoly([v(0, 0), v(100, 2), v(0, 4)], false)],
  ['poly-repeat', gPoly([v(0, 0), v(0, 0), v(50, 50)], false, ends('closed-arrow', 'none'))],
  ['quads', gQuads([tq(10, 10, 100, 12)])],
  ['quads-two', gQuads([tq(10, 10, 100, 12), tq(10, 30, 60, 12)])],
  ['quads-rot30', gQuads([tqRot(40, 40, 80, 10, 30)])],
  ['quads-empty', gQuads([])],
  ['caret', gCaret(rc(20, 20, 6, 6))],
  ['caret-rot45', gCaret(rc(20, 20, 6, 6), 45)],
  ['ink', gInk([[v(0, 0), v(10, 5), v(20, 0)]])],
  ['ink-two', gInk([[v(0, 0), v(10, 5)], [v(30, 30), v(40, 50), v(45, 20)]])],
  ['ink-rot20', gInk([[v(0, 0), v(10, 5), v(20, 0)]], 20)],
  ['ink-empty', gInk([])],
  ['ink-dot', gInk([[v(7, 7)]])],
  ['text', gText(rc(50, 50, 120, 40))],
  ['text-rot90', gText(rc(50, 50, 120, 40), undefined, 90)],
  ['text-rot33', gText(rc(50, 50, 120, 40), undefined, 33)],
  ['callout', gText(rc(100, 100, 120, 40), { tip: v(20, 20), ending: 'open-arrow' })],
  [
    'callout-knee',
    gText(rc(100, 100, 120, 40), { tip: v(20, 20), knee: v(60, 30), ending: 'closed-arrow' }),
  ],
  [
    'callout-rot90',
    gText(rc(100, 100, 120, 40), { tip: v(20, 200), knee: v(60, 180), ending: 'circle' }, 90),
  ],
  ['callout-below', gText(rc(100, 100, 120, 40), { tip: v(160, 300), ending: 'square' })],
  ['callout-none', gText(rc(100, 100, 120, 40), { tip: v(300, 110), ending: 'none' })],
  ['callout-inside', gText(rc(100, 100, 120, 40), { tip: v(160, 120), ending: 'open-arrow' })],
];

export const geom = (name: string): Geom => {
  const found = GEOMS.find(([n]) => n === name);
  if (!found) throw new Error(`no geom ${name}`);
  return found[1];
};

// ── annots and models ─────────────────────────────────────────────────────────

export const PON = 1;
/** The page address for an object number. */
export const pageRef = (pon: number) => ({ kind: 'objectNumber' as const, pageObjectNumber: pon });
export const PAGE_REF = pageRef(PON);

export const flags = (over: Partial<AnnotationFlags> = {}): AnnotationFlags => ({
  ...NO_ANNOTATION_FLAGS,
  ...over,
});

export const style = (over: Partial<Style> = {}): Style => ({ ...initialStyle, ...over });

/** A stable object number per id, so a ref never depends on build order. */
const objectNumber = (id: string): number =>
  [...id].reduce((n, ch) => (n * 31 + ch.charCodeAt(0)) % 100000, 7);

/** A committed annotation (it has a ref) with the drawn defaults. */
export const annot = (id: string, subtype: string, g: Geom, over: Partial<Annot> = {}): Annot => ({
  id,
  ref: { kind: 'objectNumber', page: PAGE_REF, annotObjectNumber: objectNumber(id) },
  page: PAGE_REF,
  subtype,
  geom: g,
  style: initialStyle,
  flags: DRAWN_FLAGS,
  source: 'vector',
  ...over,
});

export const sq = (id: string, r: Rect = rc(100, 100, 80, 60), over: Partial<Annot> = {}): Annot =>
  annot(id, 'square', gRect(r), over);

/** A model start: fields of `Model` layered over `initialModel` by the suite
 *  wrappers, with `byId`/`order` built from `annots`. */
export const start = (annots: Annot[], over: Partial<Model> = {}): Partial<Model> => ({
  byId: Object.fromEntries(annots.map((a) => [a.id, a])),
  order: annots.map((a) => a.id),
  ...over,
});

/** The full model a start describes. */
export const modelOf = (s: Partial<Model> | undefined): Model => ({ ...initialModel, ...s });
