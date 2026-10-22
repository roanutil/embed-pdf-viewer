/** Current upstream inputs shared by the annotation oracle suites. */
import { DRAWN_FLAGS, NO_ANNOTATION_FLAGS } from '../../../../packages/core/annotation/src/flags';
import { calloutShape } from '../../../../packages/core/annotation/src/shapes/text-box';
import { initialModel } from '../../../../packages/core/annotation/src/update';
import { annotationKey } from '@embedpdf/core';
import { rectFromPoints } from '../../../../packages/core/annotation/src/rect';
import {
  recordOf,
  modelWith,
  type RecordInput,
} from '../../../../packages/core/annotation/test/support';
import {
  bodyFromTextStyle,
  paragraphsFromPlainText,
} from '../../../../packages/core/annotation/src/richtext';
import { ANNOTATION_FIELD_NAMES } from '@embedpdf/engine-core/runtime';
import type { Annotation, AnnotationFlags, PageRef } from '@embedpdf/engine-core/runtime';
import type {
  LineEnding,
  LineEndings,
  Model,
  ModelAnnotation,
  Shape,
  Style,
  Message,
  Point,
  Quad,
  Rect,
} from '../../../../packages/core/annotation/src/types';
import type { FunctionCase, Suite } from '../vectors';
export const suites: Suite[] = [];
export type Fn = (...args: never[]) => unknown;
export type Vec = Point;
export type TextQuad = Quad;
export type Geom = Shape;
export type Annot = ModelAnnotation;
export type Msg = Message;
export type { Style };
export type Border = Pick<Style, 'borderStyle' | 'dashArray' | 'cloudyIntensity'>;
export interface Callout {
  tip: Point;
  knee?: Point;
  ending: LineEnding;
}
export const initialStyle: Style = {
  color: '#e5484d',
  interiorColor: null,
  strokeWidth: 2,
  opacity: 1,
  blendMode: 'normal',
  borderStyle: 'solid',
  dashArray: null,
  cloudyIntensity: null,
};
/** A case builder expands model/session inputs before recording them. Runtime wrappers do not translate old inputs. */
export const c = (fn: string, label: string, ...args: unknown[]): FunctionCase => {
  const modelFns = new Set([
    'groupCaps',
    'groupKeyOf',
    'groupMembers',
    'expandGroups',
    'linkChildrenOf',
    'linkOf',
    'groupProbe',
    'defaultsFor',
    'computeMoveSnap',
    'paintOrder',
    'isSelectable',
    'canMove',
    'selectability',
    'groupUnionBounds',
    'hitTest',
    'cursorAt',
    'hitCalls',
    'hitProbe',
    'run',
    'runs',
    'rotateDraftDelta',
    'rotateDraftDeltas',
    'annotsInBox',
    'annotsInBoxes',
    'pageItems',
    'chrome',
    'selectedItems',
    'textBoxes',
    'selectionBoundsOnPage',
    'selectionKnob',
    'creationDraftAnchor',
    'selectionAnchor',
    'selectionAnchors',
    'viewAll',
    'viewMany',
  ]);
  if (modelFns.has(fn)) args[0] = modelOf(args[0] as Partial<Model>);
  if (
    [
      'groupCaps',
      'groupKeyOf',
      'groupMembers',
      'expandGroups',
      'linkChildrenOf',
      'linkOf',
      'groupProbe',
      'isSelectable',
      'canMove',
      'selectability',
      'computeMoveSnap',
    ].includes(fn)
  )
    args[1] = idsOf(args[1]);
  if (fn === 'hitProbe') args[2] = idsOf(args[2]);
  if (fn === 'run') args[1] = (args[1] as Message[]).map(messageOf);
  if (fn === 'runs')
    args[1] = (args[1] as { name: string; over: Partial<Model>; msgs: Message[] }[]).map(
      ({ name, over, msgs }) => ({
        name,
        model: modelOf({ ...(args[0] as Model), ...over }),
        msgs: msgs.map(messageOf),
      }),
    );
  if (['viewMany', 'selectionAnchors'].includes(fn))
    args[1] = (args[1] as { name: string; over: Partial<Model>; pon?: number }[]).map(
      ({ over, pon, ...call }) => ({
        ...call,
        model: modelOf({ ...(args[0] as Model), ...over }),
        ...(pon !== undefined ? { page: pageRef(pon) } : {}),
      }),
    );
  if (fn === 'hitCalls')
    args[1] = (args[1] as { selected: string[]; pon: number }[]).map(
      ({ selected, pon, ...call }) => ({
        ...call,
        model: modelOf({ ...(args[0] as Model), selected }),
        page: pageRef(pon),
      }),
    );
  const pageArg = ['computeMoveSnap'].includes(fn)
    ? 2
    : [
          'paintOrder',
          'groupUnionBounds',
          'hitTest',
          'cursorAt',
          'hitProbe',
          'annotsInBox',
          'pageItems',
          'chrome',
          'textBoxes',
          'selectionBoundsOnPage',
          'selectionKnob',
          'viewAll',
        ].includes(fn)
      ? 1
      : -1;
  if (pageArg >= 0) args[pageArg] = pageRef(args[pageArg] as number);
  if (fn === 'annotsInBox') args.splice(2, 2, rectFromPoints(args[2] as Point, args[3] as Point));
  if (fn === 'annotsInBoxes')
    args[1] = (args[1] as [number, Point, Point, unknown?, unknown?][]).map(
      ([pon, a, b, ...rest]) => [pageRef(pon), rectFromPoints(a, b), ...rest],
    );
  if (fn === 'geomHitProbe') args[3] = { strokeWidth: args[3] };
  if (
    ['geomVisualBounds', 'selectionBounds', 'selectionQuad', 'obbFromGeom', 'geomScene'].includes(
      fn,
    ) &&
    typeof args[1] === 'number'
  )
    args.splice(1, 2, {
      strokeWidth: args[1],
      cloudyIntensity: (args[2] as Border | undefined)?.cloudyIntensity,
    });
  if (['rotateDraftDelta', 'rotateDraftDeltas'].includes(fn)) args[1] = sessionIds(args[1]);
  return { name: `${fn}/${label}`, fn, args };
};
export const v = (x: number, y: number): Point => ({ x, y });
export const rc = (x: number, y: number, width: number, height: number): Rect => ({
  x,
  y,
  width,
  height,
});
export const tq = (x: number, y: number, width: number, height: number): Quad => ({
  upperLeft: v(x, y),
  upperRight: v(x + width, y),
  lowerLeft: v(x, y + height),
  lowerRight: v(x + width, y + height),
});
export const tqRot = (x: number, y: number, width: number, height: number, deg: number): Quad => {
  const r = (deg * Math.PI) / 180;
  const at = (dx: number, dy: number) =>
    v(x + dx * Math.cos(r) - dy * Math.sin(r), y + dx * Math.sin(r) + dy * Math.cos(r));
  return {
    upperLeft: at(0, 0),
    upperRight: at(width, 0),
    lowerLeft: at(0, height),
    lowerRight: at(width, height),
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
export const SOLID: Border = { borderStyle: 'solid', dashArray: null, cloudyIntensity: null };
export const DASHED: Border = { borderStyle: 'dashed', dashArray: [3, 2], cloudyIntensity: null };
export const cloudy = (intensity: number): Border => ({
  borderStyle: 'solid',
  dashArray: null,
  cloudyIntensity: intensity,
});
export const gRect = (
  box: Rect,
  ellipse = false,
  rotation = 0,
): Extract<Shape, { kind: 'box' }> => ({ kind: 'box', box, ellipse, rotation });
export const gLine = (
  start: Point,
  end: Point,
  lineEndings?: LineEndings,
  rotation = 0,
): Extract<Shape, { kind: 'line' }> => ({
  kind: 'line',
  linePoints: { start, end },
  ...(lineEndings ? { lineEndings } : {}),
  rotation,
});
export const gPoly = (
  vertices: Point[],
  closed: boolean,
  lineEndings?: LineEndings,
  rotation = 0,
): Extract<Shape, { kind: 'poly' }> => ({
  kind: 'poly',
  vertices,
  closed,
  ...(lineEndings ? { lineEndings } : {}),
  rotation,
});
export const gQuads = (quads: Quad[]): Extract<Shape, { kind: 'quads' }> => ({
  kind: 'quads',
  quadPoints: quads,
});
export const gCaret = (box: Rect, rotation = 0): Extract<Shape, { kind: 'caret' }> => ({
  kind: 'caret',
  box,
  rotation,
});
export const gInk = (inkList: Point[][], rotation = 0): Extract<Shape, { kind: 'ink' }> => ({
  kind: 'ink',
  inkList,
  rotation,
});
export const gText = (
  box: Rect,
  callout?: Callout,
  rotation = 0,
): Extract<Shape, { kind: 'text-box' }> =>
  callout
    ? calloutShape(box, rotation, callout.tip, callout.knee, callout.ending)
    : { kind: 'text-box', box, rotation, calloutLine: null, lineEnding: null };
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
  [
    'ink-two',
    gInk([
      [v(0, 0), v(10, 5)],
      [v(30, 30), v(40, 50), v(45, 20)],
    ]),
  ],
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

// Builders state inputs in drawing terms, then use upstream's own record builder.
// Their returned values (not the builder descriptions) are recorded in cases.
export const PON = 1;
export const pageRef = (objectNumber: number): PageRef => ({ kind: 'objectNumber', objectNumber });
export const PAGE_REF = pageRef(PON);
export const flags = (over: Partial<AnnotationFlags> = {}): AnnotationFlags => ({
  ...NO_ANNOTATION_FLAGS,
  ...over,
});
export const style = (over: Partial<Style> = {}): Style => ({ ...initialStyle, ...over });
const fixtureNames = new Map<string, string>();
export const nameOf = (record: { id: string }): string => fixtureNames.get(record.id) ?? record.id;
const numberOf = (id: string): number =>
  [...id].reduce((n, ch) => (n * 31 + ch.charCodeAt(0)) % 100000, 7);
export const refOfName = (id: string, page = PAGE_REF) => ({
  kind: 'objectNumber' as const,
  page,
  objectNumber: numberOf(id),
});
export interface AnnotationOverrides extends Partial<Omit<RecordInput, 'annotation'>> {
  data?: Partial<Annotation>;
  annotation?: Partial<Annotation>;
  irt?: string;
  group?: string;
  contents?: string;
  label?: { text: string; repeat: boolean };
}
export const annot = (
  id: string,
  subtype: string,
  geometry: Shape,
  over: AnnotationOverrides = {},
): ModelAnnotation => {
  const page = over.page ?? PAGE_REF;
  const { data, annotation, irt, group, label, contents, ref, ...input } = over;
  const reply = group ?? irt;
  const supported =
    Object.hasOwn(ANNOTATION_FIELD_NAMES, subtype) ||
    subtype.startsWith('widget') ||
    subtype === 'free-text-callout';
  const actualSubtype = supported ? subtype : 'square';
  const record = recordOf({
    id: idOf(id, page),
    ref: refOfName(id, page),
    page,
    subtype: actualSubtype,
    geometry,
    style: initialStyle,
    flags: DRAWN_FLAGS,
    source: 'vector',
    ...input,
    ...(ref === null ? { unconfirmed: true as const } : {}),
    annotation: {
      ...(subtype === 'free-text' && input.text
        ? {
            richText: {
              body: bodyFromTextStyle(input.text),
              paragraphs: paragraphsFromPlainText(data?.contents ?? contents ?? ''),
            },
          }
        : {}),
      ...(!supported ? { subtype: 'unsupported', rawSubtypeCode: 0, rawSubtypeName: subtype } : {}),
      ...data,
      ...annotation,
      ...(contents !== undefined ? { contents } : {}),
      ...(label ? { overlayText: label.text, repeat: label.repeat } : {}),
      ...(reply ? { reply: { to: refOfName(reply, page), type: group ? 'group' : 'reply' } } : {}),
    } as Partial<Annotation>,
  });
  return ref === null ? { ...record, unconfirmed: true } : record;
};
export const sq = (
  id: string,
  r: Rect = rc(100, 100, 80, 60),
  over: AnnotationOverrides = {},
): ModelAnnotation => annot(id, 'square', gRect(r), over);
/** Named fixture ids use the same annotationKey as current engine records. */
export const idOf = (name: string, page = PAGE_REF): string => {
  if (/^(obj:|base:)/.test(name)) return name;
  const id = annotationKey(refOfName(name, page));
  fixtureNames.set(id, name);
  return id;
};
const idsOf = (value: unknown): unknown =>
  Array.isArray(value) ? value.map(idsOf) : typeof value === 'string' ? idOf(value) : value;
/** Build current session/message identity fields, preserving non-identity strings. */
export const sessionIds = (value: unknown): unknown => {
  if (value instanceof Set) return new Set([...value].map(idsOf));
  if (Array.isArray(value)) return value.map(sessionIds);
  if (!value || typeof value !== 'object' || 'annotation' in value) return value;
  return Object.fromEntries(
    Object.entries(value).map(([key, entry]) => [
      key,
      ['id', 'selected', 'ids', 'order', 'hovered', 'editing'].includes(key)
        ? idsOf(entry)
        : key === 'patches'
          ? Object.fromEntries(
              Object.entries(entry as object).map(([id, patch]) => [idOf(id), patch]),
            )
          : sessionIds(entry),
    ]),
  );
};
export const messageOf = (message: Message): Message => sessionIds(message) as Message;
const NUMBERS = Array.from({ length: 32 }, (_, i) => 1000001 + i);
export const start = (annots: ModelAnnotation[], over: Partial<Model> = {}): Model =>
  modelOf(modelWith(annots, over));
export const modelOf = (s: Partial<Model> | undefined): Model => ({
  ...initialModel,
  ...s,
  ...(sessionIds(s) as Partial<Model>),
  objectNumbers: s?.objectNumbers?.length ? s.objectNumbers : NUMBERS,
});

const retired = new WeakSet<Fn>();
/** Historical definition only: removed in the named upstream commit; excluded by currentSuites. */
export const historicalTarget = (name: string, commit: string): Fn => {
  const fn = Object.assign(() => undefined, { historical: { name, commit } });
  retired.add(fn);
  return fn;
};
/** Keep historical definitions in source, but never emit removal placeholders as oracle targets. */
export const currentSuites = (suites: Suite[]): Suite[] =>
  suites.map((suite) => {
    if (suite.kind !== 'function') return suite;
    const functions = Object.fromEntries(
      Object.entries(suite.functions).filter(([, fn]) => !retired.has(fn)),
    );
    return { ...suite, functions, cases: suite.cases.filter((c) => c.fn in functions) };
  });
