/**
 * `packages/plugin/annotation/src/repository`: the DTO <-> content-model
 * boundary, kind by kind. Function cases; replayed by
 * `crates/plugins/annotation/tests/conformance.rs`.
 *
 * Every derivation is pinned from both ends: `fromDTO` over one fixture per
 * kind (and the rotated, callout and widget variants), then `toPatch`,
 * `toScopedPatch` (geometry and props scopes) and `toCreateDraft` over the
 * annotations it produced, with styles varied to reach the cloudy, dashed and
 * endings couplings.
 */
import {
  boxGeomFields,
  cssToColor,
  colorToCss,
  fromDTO,
  linkChildRects,
  styleFromDTO,
  toCreateDraft,
  toPatch,
  toScopedPatch,
  widgetAppearanceFromProps,
  writableTarget,
} from '../../../../packages/plugin/annotation/src/repository';
import {
  borderFromDTO,
  boxGeomFromDTO,
  insetPdfRectByRD,
  rotFromDTO,
} from '../../../../packages/plugin/annotation/src/repository/seam';
import { cloudyExtras } from '../../../../packages/plugin/annotation/src/repository/kinds/shape';
import { calloutFields } from '../../../../packages/plugin/annotation/src/repository/kinds/freeText';
import { pdfBoundsOfQuads, quadPointsFor } from '../../../../packages/plugin/annotation/src/repository/kinds/quads';
import { widgetKindOf } from '../../../../packages/plugin/annotation/src/repository/kinds/misc';
import type { Annot, Border } from '../../../../packages/core/annotation/src/types';
import { annotationKey } from '../../../../packages/engine/core/src/runtime';

import {
  CROP,
  CROP2,
  callout,
  caret,
  circle,
  fileAttachment,
  freeText,
  ink,
  line,
  link,
  markup,
  polygon,
  polyline,
  redact,
  resetUuid,
  square,
  stamp,
  textNote,
  unsupported,
  widget,
  objRef,
  pr,
} from './annotation-fixtures';
import type { FunctionCase, Suite } from '../vectors';

type Dto = Record<string, unknown>;

const METRES = METRES_FORMAT();
const RICH_PARAS = [{ runs: [{ text: 'Hel' }, { text: 'lo', style: { weight: 700 } }] }];

const c = (fn: string, label: string, ...args: unknown[]): FunctionCase => ({ name: `${fn}/${label}`, fn, args });

const rotated = { rotation: 30, unrotatedRect: { left: 110, bottom: 610, right: 190, top: 690 } };

/** One DTO per kind and variant, with the crop it is read against. */
const DTOS: [string, Dto, typeof CROP][] = [
  ['square', square(1), CROP],
  ['square-rotated', square(2, rotated), CROP],
  ['square-rotation-only', square(3, { rotation: 90 }), CROP],
  ['square-cloudy', square(4, { cloudyIntensity: 2, rectDifferences: { left: 3, bottom: 3, right: 3, top: 3 } }), CROP],
  ['square-dashed', square(5, { borderStyle: 'dashed', dashArray: [4, 2], interiorColor: { r: 0, g: 128, b: 255 } }), CROP],
  ['square-dashed-empty', square(6, { borderStyle: 'dashed', dashArray: [] }), CROP],
  ['square-offset-crop', square(7, { pageObjectNumber: 2, rect: { left: 60, bottom: 100, right: 160, top: 200 } }), CROP2],
  ['circle', circle(8, { rotation: 45, unrotatedRect: { left: 110, bottom: 610, right: 190, top: 690 } }), CROP],
  ['line', line(9), CROP],
  ['line-rotated', line(10, { rotation: 15 }), CROP],
  ['line-no-endings', line(11, { lineEndings: undefined }), CROP],
  ['polygon', polygon(12), CROP],
  ['polygon-cloudy', polygon(13, { cloudyIntensity: 1.5, rotation: 20 }), CROP],
  ['polyline', polyline(14), CROP],
  ['ink', ink(15), CROP],
  ['ink-highlight', ink(16, { intent: 'ink-highlight', rotation: 10 }), CROP],
  ['free-text', freeText(17), CROP],
  ['free-text-rotated', freeText(18, { ...rotated, fontColor: BLUE_DTO() }), CROP],
  ['free-text-null-contents', freeText(19, { contents: null, interiorColor: { r: 255, g: 255, b: 0 } }), CROP],
  ['callout', callout(20), CROP],
  ['callout-two-points', callout(21, { calloutLine: [{ x: 60, y: 510 }, { x: 150, y: 600 }], lineEnding: undefined }), CROP],
  ['callout-rotated', callout(22, { rotation: 90, unrotatedRect: { left: 150, bottom: 550, right: 300, top: 700 } }), CROP],
  ['callout-rotation-no-box', callout(23, { rotation: 90 }), CROP],
  ['callout-one-point', callout(24, { calloutLine: [{ x: 60, y: 510 }] }), CROP],
  ['highlight', markup('highlight', 25), CROP],
  ['underline', markup('underline', 26), CROP],
  ['squiggly', markup('squiggly', 27), CROP],
  ['strikeout', markup('strikeout', 28), CROP],
  ['strikeout-text-edit', markup('strikeout', 29, { intent: 'strikeout-text-edit', inReplyTo: objRef(30), replyType: 'group' }), CROP],
  ['caret', caret(30, { intent: 'replace', contents: 'new words' }), CROP],
  ['caret-rotated', caret(31, { rotation: 90, unrotatedRect: { left: 86, bottom: 746, right: 94, top: 754 } }), CROP],
  ['redact-area', redact(32), CROP],
  ['redact-no-fill', redact(51, { interiorColor: null }), CROP],
  ['redact-text', redact(33, { quadPoints: markup('x', 0).quadPoints, overlayText: 'REDACTED', repeat: true }), CROP],
  ['text', textNote(34), CROP],
  ['text-reply', textNote(35, { inReplyTo: objRef(34), replyType: 'reply', contents: 'a reply' }), CROP],
  ['file-attachment', fileAttachment(36), CROP],
  ['stamp', stamp(37, rotated), CROP],
  ['link', link(38), CROP],
  ['link-attached', link(39, { inReplyTo: objRef(1), replyType: 'group' }), CROP],
  ['link-no-target', link(40, { target: null }), CROP],
  ['link-javascript', link(41, { target: { kind: 'javascript' } }), CROP],
  ['widget-text', widget(42, 'text', { fontFamily: 'courier', fontSize: 10, fontColor: { r: 0, g: 0, b: 255 }, color: RED_DTO() }), CROP],
  ['widget-combobox', widget(43, 'combobox'), CROP],
  ['widget-listbox', widget(44, 'listbox', { interiorColor: { r: 240, g: 240, b: 240 }, borderStyle: 'dashed' }), CROP],
  ['widget-pushbutton', widget(45, 'pushbutton'), CROP],
  ['widget-checkbox', widget(46, 'checkbox'), CROP],
  ['widget-radio', widget(47, 'radio'), CROP],
  ['widget-signature', widget(48, 'signature'), CROP],
  ['unsupported', unsupported(49), CROP],
  ['nm-ref', square(50, { ref: { kind: 'nm', page: pr(1), nm: 'abc' } }), CROP],
  // An unrotated box is read only with a rotation, and never mistaken for the
  // RD-inset rect (unlike `callout-rotated`, whose two boxes coincide).
  ['highlight-multiply', markup('highlight', 53, { blendMode: 'multiply' }), CROP],
  ['callout-unrotated-box-no-rotation', callout(54, { unrotatedRect: { left: 160, bottom: 560, right: 290, top: 690 } }), CROP],
  ['callout-rotated-distinct-box', callout(55, { rotation: 90, unrotatedRect: { left: 160, bottom: 560, right: 290, top: 690 } }), CROP],
  ['free-text-unrotated-box-no-rotation', freeText(56, { unrotatedRect: { left: 110, bottom: 610, right: 190, top: 690 } }), CROP],
  // A NaN rotation is falsy and an infinite one truthy, as JavaScript reads them.
  ['square-rotation-nan', square(90, { rotation: Number.NaN, unrotatedRect: { left: 110, bottom: 610, right: 190, top: 690 } }), CROP],
  ['square-rotation-infinite', square(91, { rotation: Number.POSITIVE_INFINITY, unrotatedRect: { left: 110, bottom: 610, right: 190, top: 690 } }), CROP],
  ['caret-unrotated-box-no-rotation', caret(58, { unrotatedRect: { left: 86, bottom: 746, right: 94, top: 754 } }), CROP],
  // Only a strikeout reads its intent.
  ['highlight-with-intent', markup('highlight', 57, { intent: 'strikeout-text-edit' }), CROP],
  // Measurement dimensions: the line carries its leader and caption, the
  // shapes their caption, and every one derives /Contents from /Measure.
  [
    'line-dimension',
    line(59, {
      intent: 'LineDimension',
      measure: METRES,
      caption: { enabled: true, position: 'inline' },
      leader: { length: 12, extension: 5, offset: 0 },
      contents: '2 m',
      linePoints: { start: { x: 100, y: 700 }, end: { x: 244, y: 700 } },
    }),
    CROP,
  ],
  ['line-dimension-bare', line(60, { intent: 'LineDimension' }), CROP],
  ['line-dimension-foreign', line(61, { intent: 'LineDimension', measure: { subtype: 'GEO' }, caption: { enabled: false }, contents: null }), CROP],
  ['polyline-dimension', polyline(62, { intent: 'PolyLineDimension', measure: METRES, caption: { enabled: true }, contents: 'stale' }), CROP],
  ['polygon-dimension', polygon(63, { intent: 'PolygonDimension', measure: METRES, caption: { enabled: true, center: { x: 150, y: 670 } } }), CROP],
  ['polygon-dimension-bare', polygon(64, { intent: 'PolygonDimension', rotation: 30 }), CROP],
  // A rich body carries the formatting the /DA cannot.
  ['free-text-rich', freeText(65, { richText: { body: RICH_BODY(700, true, ['underline']), paragraphs: RICH_PARAS } }), CROP],
  ['free-text-rich-plain', freeText(66, { richText: { body: RICH_BODY(400, false, []), paragraphs: RICH_PARAS } }), CROP],
];

/** 72 pt to the metre. */
function METRES_FORMAT() {
  return {
    subtype: 'RL',
    ratio: '1 in = 1 m',
    x: [{ unit: 'm', conversion: 1 / 72 }],
    distance: [{ unit: 'm', conversion: 1 }],
    area: [{ unit: 'm²', conversion: 1 }],
  };
}
function RICH_BODY(weight: number, italic: boolean, decoration: string[]) {
  return {
    family: 'Helvetica',
    weight,
    italic,
    size: 12,
    color: '#EF4444',
    decoration,
    script: 'normal',
    letterSpacing: 0,
    horizontalScale: 1,
    align: 'left',
    dir: 'ltr',
  };
}

function BLUE_DTO() {
  return { r: 56, g: 88, b: 233 };
}
function RED_DTO() {
  return { r: 239, g: 68, b: 68 };
}

const annotOf = (label: string, source: 'baked' | 'vector' = 'vector'): Annot => {
  const found = DTOS.find(([l]) => l === label);
  if (!found) throw new Error(`no dto ${label}`);
  return fromDTO(found[1] as never, found[2], source);
};
const cropOf = (label: string) => DTOS.find(([l]) => l === label)![2];

/** Style overrides that reach the shape and stroke couplings. */
const CLOUDY: Border = { kind: 'cloudy', intensity: 2 };
const DASHED: Border = { kind: 'dashed', dash: [5, 3] };
const restyle = (a: Annot, style: Partial<Annot['style']>): Annot => ({ ...a, style: { ...a.style, ...style } });

const PROP_SCOPES: [string, string[]][] = [
  ['color', ['color']],
  ['stroke-width', ['strokeWidth']],
  ['border', ['border']],
  ['line-endings', ['lineEndings']],
  ['interior', ['interiorColor', 'opacity', 'blendMode']],
  ['font', ['fontFamily', 'fontSize', 'fontColor', 'textAlign']],
  ['format', ['bold', 'italic', 'underline']],
  ['icon', ['icon']],
  ['link', ['link']],
  ['unknown', ['bogus']],
  ['empty', []],
];

const cases: FunctionCase[] = [];

for (const [label, dto, crop] of DTOS) {
  cases.push(c('fromDTO', label, dto, crop, 'baked'));
}
cases.push(c('fromDTO', 'vector-source', square(1), CROP, 'vector'));
cases.push(c('fromDTO', 'default-source', square(1), CROP));
// annotationKey(dto.ref) throws on a record without its address.
cases.push(c('fromDTO', 'no-ref', (({ ref: _ref, ...rest }) => rest)(square(1)), CROP, 'baked'));
cases.push(c('fromDTO', 'null-ref', { ...square(1), ref: null }, CROP, 'baked'));

for (const [label] of DTOS) {
  const a = annotOf(label);
  const crop = cropOf(label);
  cases.push(c('toPatch', label, a, crop));
  cases.push(c('toScopedPatch', `${label}/geometry`, a, { kind: 'geometry' }, crop));
  cases.push(c('toCreateDraft', label, a, crop));
}

// Style variants through the prop scopes.
const STYLED: [string, Annot, typeof CROP][] = [
  ['square', annotOf('square'), CROP],
  ['square-cloudy', restyle(annotOf('square'), { border: CLOUDY }), CROP],
  ['square-dashed', restyle(annotOf('square'), { border: DASHED, interiorColor: '#00ff00' }), CROP],
  ['circle-cloudy', restyle(annotOf('circle'), { border: CLOUDY, strokeWidth: 4 }), CROP],
  ['line', annotOf('line'), CROP],
  ['line-no-endings', annotOf('line-no-endings'), CROP],
  ['polygon-cloudy', restyle(annotOf('polygon'), { border: CLOUDY }), CROP],
  ['polygon-dashed', restyle(annotOf('polygon'), { border: DASHED }), CROP],
  ['polyline', annotOf('polyline'), CROP],
  ['ink', annotOf('ink'), CROP],
  ['free-text', annotOf('free-text'), CROP],
  ['free-text-no-text', { ...annotOf('free-text'), text: undefined } as Annot, CROP],
  ['callout', annotOf('callout'), CROP],
  ['highlight', annotOf('highlight'), CROP],
  ['redact-text', annotOf('redact-text'), CROP],
  ['text', annotOf('text'), CROP],
  ['text-no-icon', { ...annotOf('text'), icon: undefined } as Annot, CROP],
  ['link', annotOf('link'), CROP],
  ['link-null', annotOf('link-no-target'), CROP],
  ['link-readonly', annotOf('link-javascript'), CROP],
  ['widget-text', annotOf('widget-text'), CROP],
  ['stamp', annotOf('stamp'), CROP],
  ['square-empty-interior', restyle(annotOf('square'), { interiorColor: '' }), CROP],
  ['free-text-rich', annotOf('free-text-rich'), CROP],
  ['free-text-bold-draft', { ...annotOf('free-text'), text: { ...annotOf('free-text').text!, bold: true, underline: true }, data: undefined } as Annot, CROP],
  ['line-dimension', annotOf('line-dimension'), CROP],
  ['polygon-dimension', annotOf('polygon-dimension'), CROP],
];
// The measurement scopes: a caption for every dimension, a leader for the line only.
for (const label of ['line-dimension', 'line-dimension-bare', 'polyline-dimension', 'polygon-dimension', 'square']) {
  cases.push(c('toScopedPatch', `${label}/caption`, annotOf(label), { kind: 'caption' }, CROP));
  cases.push(c('toScopedPatch', `${label}/leader`, annotOf(label), { kind: 'leader' }, CROP));
}
for (const [label, a, crop] of STYLED) {
  for (const [scope, keys] of PROP_SCOPES) {
    cases.push(c('toScopedPatch', `${label}/${scope}`, a, { kind: 'props', keys }, crop));
  }
}
for (const [label, a, crop] of STYLED) cases.push(c('toPatch', `styled/${label}`, a, crop));
cases.push(c('toCreateDraft', 'styled/square-cloudy', STYLED[1][1], CROP));
cases.push(c('toCreateDraft', 'styled/polygon-cloudy', STYLED[6][1], CROP));

// Drafts from annotations that never had a DTO (a fresh draw).
const fresh = (subtype: string, geom: Annot['geom'], over: Partial<Annot> = {}): Annot => ({
  ...annotOf('square'),
  id: 'tmp:1',
  ref: null,
  subtype,
  geom,
  data: undefined,
  ...over,
});
const tq = (x: number, y: number, w: number, h: number) => ({
  upperStart: { x, y },
  upperEnd: { x: x + w, y },
  lowerStart: { x, y: y + h },
  lowerEnd: { x: x + w, y: y + h },
});
cases.push(
  c('toCreateDraft', 'fresh/ink-highlight', fresh('ink', { t: 'ink', strokes: [[{ x: 1, y: 2 }, { x: 3, y: 4 }]] }, { intent: 'ink-highlight' }), CROP),
  c('toCreateDraft', 'fresh/caret', fresh('caret', { t: 'caret', rect: { x: 10, y: 10, width: 6, height: 8 } }), CROP),
  c('toCreateDraft', 'fresh/caret-replace', fresh('caret', { t: 'caret', rect: { x: 10, y: 10, width: 6, height: 8 }, rot: 90 }, { intent: 'replace' }), CROP),
  c('toCreateDraft', 'fresh/strikeout-edit', fresh('strikeout', { t: 'quads', quads: [tq(10, 10, 50, 12)] }, { intent: 'strikeout-text-edit' }), CROP),
  c('toCreateDraft', 'fresh/strikeout-rect-geom', fresh('strikeout', { t: 'rect', rect: { x: 1, y: 1, width: 1, height: 1 }, ellipse: false }), CROP),
  c('toCreateDraft', 'fresh/redact-quads', fresh('redact', { t: 'quads', quads: [tq(10, 10, 50, 12), tq(10, 30, 20, 12)] }), CROP),
  c('toCreateDraft', 'fresh/free-text', fresh('free-text', { t: 'text', rect: { x: 10, y: 10, width: 100, height: 30 } }), CROP),
  c('toCreateDraft', 'fresh/link', fresh('link', { t: 'rect', rect: { x: 10, y: 10, width: 100, height: 30 }, ellipse: false }), CROP),
  c('toCreateDraft', 'fresh/widget', fresh('widget-text', { t: 'rect', rect: { x: 10, y: 10, width: 100, height: 30 }, ellipse: false }), CROP),
  c('toCreateDraft', 'fresh/markup-no-geometry', fresh('highlight', { t: 'quads', quads: [] }), CROP),
  c('toPatch', 'fresh/unsupported', fresh('unsupported', { t: 'rect', rect: { x: 1, y: 1, width: 1, height: 1 }, ellipse: false }), CROP),
  c('toScopedPatch', 'fresh/unsupported-geometry', fresh('unsupported', { t: 'rect', rect: { x: 1, y: 1, width: 1, height: 1 }, ellipse: false }), { kind: 'geometry' }, CROP),
  c('toScopedPatch', 'fresh/highlight-geometry', fresh('highlight', { t: 'quads', quads: [tq(1, 1, 5, 5)] }), { kind: 'geometry' }, CROP),
  c('toScopedPatch', 'fresh/square-no-rect-geom', fresh('line', { t: 'rect', rect: { x: 1, y: 1, width: 1, height: 1 }, ellipse: false }), { kind: 'geometry' }, CROP),
  c('toScopedPatch', 'fresh/poly-on-ink-geom', fresh('polygon', { t: 'line', a: { x: 1, y: 1 }, b: { x: 3, y: 4 } }), { kind: 'geometry' }, CROP),
  c('toScopedPatch', 'fresh/ink-on-line-geom', fresh('ink', { t: 'line', a: { x: 1, y: 1 }, b: { x: 3, y: 4 } }), { kind: 'geometry' }, CROP),
  c('toScopedPatch', 'fresh/caret-on-rect-geom', fresh('caret', { t: 'rect', rect: { x: 1, y: 1, width: 1, height: 1 }, ellipse: false }), { kind: 'geometry' }, CROP),
  c('toScopedPatch', 'fresh/free-text-on-rect-geom', fresh('free-text', { t: 'rect', rect: { x: 1, y: 1, width: 1, height: 1 }, ellipse: false }), { kind: 'geometry' }, CROP),
  c('toScopedPatch', 'fresh/redact-quads-geometry', fresh('redact', { t: 'quads', quads: [tq(1, 1, 5, 5)] }), { kind: 'geometry' }, CROP),
  c('toScopedPatch', 'fresh/text-on-line-geom', fresh('text', { t: 'line', a: { x: 1, y: 1 }, b: { x: 3, y: 4 } }), { kind: 'geometry' }, CROP),
  c('toScopedPatch', 'fresh/line-endings-on-rect', fresh('line', { t: 'rect', rect: { x: 1, y: 1, width: 1, height: 1 }, ellipse: false }), { kind: 'props', keys: ['lineEndings'] }, CROP),
  c('toScopedPatch', 'fresh/square-props-on-line', fresh('square', { t: 'line', a: { x: 1, y: 1 }, b: { x: 3, y: 4 } }, { style: { ...annotOf('square').style, border: CLOUDY } }), { kind: 'props', keys: ['border', 'strokeWidth'] }, CROP),
);

// The seam helpers.
cases.push(
  c('annotationKey', 'object-number', objRef(12)),
  c('annotationKey', 'nm', { kind: 'nm', page: pr(1), nm: 'x y' }),
  c('annotationKey', 'index', { kind: 'index', page: pr(3), index: 7, revision: { docSessionId: 's', page: pr(3), generation: 1 } }),
  c('colorToCss', 'plain', { r: 255, g: 16, b: 0 }),
  c('colorToCss', 'clamped', { r: 300, g: -5, b: 12.6 }),
  c('cssToColor', 'six', '#aabbcc'),
  c('cssToColor', 'six-bare-upper', 'AABBCC'),
  c('cssToColor', 'three', '#abc'),
  c('cssToColor', 'three-padded', '  #ABC\n'),
  c('cssToColor', 'named', 'red'),
  c('cssToColor', 'four', '#abcd'),
  c('boxGeomFields', 'flat', { x: 10, y: 20, width: 100, height: 50 }, 0, CROP),
  c('boxGeomFields', 'rotated', { x: 10, y: 20, width: 100, height: 50 }, 30, CROP2),
  c('boxGeomFromDTO', 'flat', { rect: CROP }, undefined, undefined, CROP, false),
  c('boxGeomFromDTO', 'rotated', { rect: CROP }, 90, { left: 1, bottom: 2, right: 3, top: 4 }, CROP, true),
  c('boxGeomFromDTO', 'rotation-without-box', { rect: CROP }, 45, undefined, CROP, false),
  c('boxGeomFromDTO', 'full-turn', { rect: CROP }, 360, { left: 1, bottom: 2, right: 3, top: 4 }, CROP, false),
  c('insetPdfRectByRD', 'inset', { left: 0, bottom: 0, right: 100, top: 50 }, { left: 1, bottom: 2, right: 3, top: 4 }),
  c('insetPdfRectByRD', 'none', { left: 0, bottom: 0, right: 100, top: 50 }, null),
  c('rotFromDTO', 'set', 30),
  c('rotFromDTO', 'zero', 0),
  c('rotFromDTO', 'absent'),
  c('borderFromDTO', 'cloudy', { cloudyIntensity: 1, borderStyle: 'dashed' }),
  c('borderFromDTO', 'dashed', { borderStyle: 'dashed', dashArray: [2, 1] }),
  c('borderFromDTO', 'dashed-default', { borderStyle: 'dashed' }),
  c('borderFromDTO', 'solid', { borderStyle: 'solid', cloudyIntensity: 0 }),
  c('writableTarget', 'goto', { kind: 'goto', destination: { kind: 'fit', page: pr(1) } }),
  c('writableTarget', 'uri', { kind: 'uri', uri: 'https://x' }),
  c('writableTarget', 'named', { kind: 'named', name: 'NextPage' }),
  c('writableTarget', 'null', null),
  c('writableTarget', 'undefined'),
  c('widgetKindOf', 'text', 'text'),
  c('widgetKindOf', 'unknown', 'signature'),
  c('widgetAppearanceFromProps', 'everything', {
    color: '#ff0000',
    interiorColor: '#00ff00',
    strokeWidth: 2,
    border: { kind: 'dashed', dash: [2, 2] },
    fontFamily: 'courier',
    fontSize: 11,
    fontColor: '#0000ff',
    textAlign: 'center',
  }),
  c('widgetAppearanceFromProps', 'clears', { interiorColor: null, border: { kind: 'solid' } }),
  c('widgetAppearanceFromProps', 'empty-interior', { interiorColor: '' }),
  c('widgetAppearanceFromProps', 'none', {}),
  c('cloudyExtras', 'cloudy', restyle(annotOf('square'), { border: CLOUDY })),
  c('cloudyExtras', 'plain', annotOf('square')),
  c('cloudyExtras', 'not-a-rect', annotOf('line')),
  c('calloutFields', 'callout', annotOf('callout'), CROP),
  c('calloutFields', 'rotated', annotOf('callout-rotated'), CROP),
  c('calloutFields', 'plain', annotOf('free-text'), CROP),
  c('quadPointsFor', 'quads', annotOf('highlight'), CROP),
  c('quadPointsFor', 'not-quads', annotOf('square'), CROP),
  c('pdfBoundsOfQuads', 'two', [
    { p1: { x: 1, y: 9 }, p2: { x: 5, y: 9 }, p3: { x: 1, y: 2 }, p4: { x: 5, y: 2 } },
    { p1: { x: -3, y: 20 }, p2: { x: 0, y: 20 }, p3: { x: -3, y: 12 }, p4: { x: 0, y: 12 } },
  ]),
  c('pdfBoundsOfQuads', 'none', []),
);
for (const label of ['square-rotated', 'callout', 'highlight', 'line', 'ink', 'redact-text']) {
  cases.push(c('linkChildRects', label, annotOf(label)));
}
for (const label of ['square', 'polygon-cloudy', 'ink', 'free-text', 'highlight', 'caret', 'redact-area', 'redact-no-fill', 'text', 'widget-text', 'widget-checkbox', 'unsupported']) {
  const found = DTOS.find(([l]) => l === label)!;
  cases.push(c('styleFromDTO', label, found[1]));
}

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'plugins/annotation-repository.json',
    describes: 'packages/plugin/annotation/src/repository: the DTO to content-model boundary, kind by kind.',
    functions: {
      fromDTO: fromDTO as never,
      toPatch: toPatch as never,
      toScopedPatch: toScopedPatch as never,
      toCreateDraft: ((a: Annot, crop: typeof CROP) => {
        resetUuid();
        return toCreateDraft(a, crop);
      }) as never,
      annotationKey: annotationKey as never,
      colorToCss: colorToCss as never,
      cssToColor: cssToColor as never,
      boxGeomFields: boxGeomFields as never,
      boxGeomFromDTO: boxGeomFromDTO as never,
      insetPdfRectByRD: insetPdfRectByRD as never,
      rotFromDTO: rotFromDTO as never,
      borderFromDTO: borderFromDTO as never,
      writableTarget: writableTarget as never,
      widgetKindOf: widgetKindOf as never,
      widgetAppearanceFromProps: widgetAppearanceFromProps as never,
      cloudyExtras: cloudyExtras as never,
      calloutFields: calloutFields as never,
      quadPointsFor: quadPointsFor as never,
      pdfBoundsOfQuads: pdfBoundsOfQuads as never,
      linkChildRects: linkChildRects as never,
      styleFromDTO: styleFromDTO as never,
    },
    cases,
  },
];
