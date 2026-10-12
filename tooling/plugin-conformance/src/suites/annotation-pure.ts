/**
 * `packages/plugin/annotation`: the pure modules beside the repository. The
 * tool registry (`tools.ts`), the free-text projection (`text-item.ts`), the
 * icon placement draft (`placement.ts`), the reducer, and the comment-thread
 * composer the `comments` lens is built on (`engine-core`'s
 * `buildCommentThreads`, which the Rust crate transcribes). Function cases;
 * replayed by `crates/plugins/annotation/tests/conformance.rs`.
 */
import { buildCommentThreads } from '../../../../packages/engine/core/src/runtime';
import { sniffBinaryMetadata } from '../../../../packages/engine/core/src/resource/binaryMetadata';
import { buildTextItems } from '../../../../packages/plugin/annotation/src/text-item';
import { iconPlacementDraft } from '../../../../packages/plugin/annotation/src/write/placement';
import {
  annotationReducer,
  initialAnnotationState,
  mergeChrome,
  DEFAULT_CHROME,
} from '../../../../packages/plugin/annotation/src/model';
import { buildToolRegistry, isTouchDirect } from '../../../../packages/plugin/annotation/src/tools/definitions';
import { fromDTO } from '../../../../packages/plugin/annotation/src/repository';
import {
  bodyFromTextStyle,
  cssFontFamilyForFace,
  cssFontFamilyForFont,
  faceForFont,
  familyKey,
  fontForFace,
  rangeProps,
  richDocOf,
  runDeltaForProps,
  runsInRange,
  stripBodyDefaults,
  textCommitPatch,
} from '../../../../packages/plugin/annotation/src/rich-text';
import { geometryFromInput } from '../../../../packages/plugin/annotation/src/create-input';
import { previewBucket } from '../../../../packages/plugin/annotation/src/host-contract';
import { initialModel, defaultsFor } from '../../../../packages/core/annotation/src/update';

import { CROP, callout, freeText, objRef, pr, square, textNote, markup, link, widget, base } from './annotation-fixtures';
import type { FunctionCase, Suite } from '../vectors';

const c = (fn: string, label: string, ...args: unknown[]): FunctionCase => ({ name: `${fn}/${label}`, fn, args });

const cases: FunctionCase[] = [];

// ── tools ─────────────────────────────────────────────────────────────────────

cases.push(
  c('buildToolRegistry', 'built-ins'),
  c('buildToolRegistry', 'configure-built-in', [
    { id: 'ink', defaults: { strokeWidth: 9 }, ink: { straighten: { deviationThreshold: 0.2, axisSnapDegrees: 10 } } },
    { id: 'line', defaults: { lineEndings: { start: 'circle' } } },
    { id: 'square', enables: ['annotation-draw'], meta: { label: 'Box' }, flags: { locked: true } },
    { id: 'note', flags: { print: false }, meta: { icon: 'n' } },
  ]),
  c('buildToolRegistry', 'presets', [
    { id: 'arrow', extends: 'line', defaults: { lineEndings: { end: 'open-arrow' } }, cursor: 'cell' },
    { id: 'thick-arrow', extends: 'arrow', defaults: { strokeWidth: 12 }, meta: { label: 'Thick' } },
    { id: 'pen', extends: 'ink-highlight', ink: { groupStrokesMs: 0 }, upright: true, clickCreate: false, ghost: false },
    { id: 'big-free', extends: 'free-text', clickCreate: { width: 300, height: 100 }, ghost: { mode: 'footprint' } },
    { id: 'insert', extends: 'insert-text', preset: 'mine' },
  ]),
  c('buildToolRegistry', 'custom-kind', [{ id: 'widget-sig', subtype: 'widget-signature', defaults: { opacity: 0.5, bogus: 1 } }]),
  c('buildToolRegistry', 'bare-id', [{ id: 'mystery' }]),
  c('buildToolRegistry', 'own-flags', [{ id: 'locked-box', subtype: 'square', flags: { locked: true } }]),
  c('buildToolRegistry', 'self-extends', [{ id: 'loop', extends: 'loop', subtype: 'square' }]),
  c('buildToolRegistry', 'cycle', [
    { id: 'a', extends: 'b', subtype: 'square' },
    { id: 'b', extends: 'a', cursor: 'move' },
  ]),
  c('buildToolRegistry', 'extends-forward', [
    { id: 'first', extends: 'second' },
    { id: 'second', subtype: 'circle', enables: ['annotation-draw', 'annotation-draw'] },
  ]),
  c('buildToolRegistry', 'unknown-base', [{ id: 'orphan', extends: 'nope' }]),
  c('buildToolRegistry', 'bad-default', [{ id: 'square', defaults: { fontSize: 12 } }]),
  // Every known kind is strict about its defaults: one key it doesn't take each.
  ...(
    [
      ['line', { icon: 'comment' }],
      ['polyline', { icon: 'comment' }],
      ['ink', { icon: 'comment' }],
      ['free-text', { blendMode: 'multiply' }],
      ['free-text-callout', { blendMode: 'multiply' }],
      ['underline', { strokeWidth: 2 }],
      ['redact', { strokeWidth: 2 }],
      ['stamp', { opacity: 0.5 }],
      ['link', { color: '#000000' }],
      ['file-attachment', { strokeWidth: 2 }],
      ['widget-choice', { opacity: 0.5 }],
      ['widget-toggle', { fontSize: 10 }],
    ] as const
  ).map(([kind, defaults]) => c('buildToolRegistry', `bad-default-${kind}`, [{ id: `bad-${kind}`, subtype: kind, defaults }])),
  c('buildToolRegistry', 'bad-default-inherited', [{ id: 'marker', extends: 'highlight', subtype: 'caret' }]),
  c('buildToolRegistry', 'widget-kinds', [
    { id: 'field-text', subtype: 'widget-text', defaults: { fontSize: 10, border: { kind: 'solid' } } },
    { id: 'field-choice', subtype: 'widget-choice', defaults: { textAlign: 'center' } },
    { id: 'field-check', subtype: 'widget-toggle', defaults: { interiorColor: null } },
    { id: 'stamped', subtype: 'stamp', defaults: {} },
    { id: 'docs-link', extends: 'link', defaults: { link: { kind: 'uri', uri: 'https://docs' } } },
    { id: 'pin', subtype: 'text', defaults: { icon: 'pin' } },
    { id: 'clip', subtype: 'file-attachment', defaults: { icon: 'tag' } },
    { id: 'poly', subtype: 'polyline', defaults: { lineEndings: { start: 'square' } } },
    { id: 'callout2', subtype: 'free-text-callout', defaults: { fontFamily: 'courier' } },
  ]),
  c('isTouchDirect', 'draw', ['annotation-draw', 'annotation-edit']),
  c('isTouchDirect', 'select', ['text-select']),
  c('isTouchDirect', 'place', ['annotation-place', 'annotation-edit']),
);

// ── text items ────────────────────────────────────────────────────────────────

const a = (dto: Record<string, unknown>, over: Record<string, unknown> = {}) => ({ ...fromDTO(dto as never, CROP, 'vector'), ...over });
const model = (annots: Record<string, unknown>[], over: Record<string, unknown> = {}) => ({
  ...initialModel,
  byId: Object.fromEntries(annots.map((x) => [x.id as string, x])),
  order: annots.map((x) => x.id as string),
  ...over,
});
/** A rich body: the engine's resolved defaults with the three toggles set. */
const body = (weight: number, italic: boolean, decoration: string[], over: Record<string, unknown> = {}) => ({
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
  ...over,
});
const PARAS = [
  { runs: [{ text: 'Hel' }, { text: 'lo', style: { weight: 700 } }], align: 'left', dir: 'ltr' },
  { runs: [{ text: 'world', style: { italic: true, decoration: ['underline'], color: '#0000FF' } }], align: 'center' },
  { runs: [{ text: '' }, { text: 'x', style: { family: 'Courier', size: 20 } }], dir: 'rtl' },
];
const richTexts = [
  a(freeText(11, { richText: { body: body(700, true, ['underline']), paragraphs: PARAS } })),
  a(freeText(12, { richText: { body: body(400, false, []), paragraphs: PARAS } })),
  a(freeText(13), { text: { fontFamily: 'helvetica', fontSize: 9, fontColor: '#000000', textAlign: 'right', bold: true, italic: true, underline: true } }),
];
const texts = [
  a(freeText(1)),
  a(freeText(2, { fontFamily: 'courier', fontColor: { r: 0, g: 0, b: 255 }, interiorColor: { r: 255, g: 255, b: 0 } })),
  a(freeText(3, { fontFamily: 'times-roman', rotation: 30, unrotatedRect: { left: 110, bottom: 610, right: 190, top: 690 }, contents: null })),
  a(callout(4, { fontFamily: 'MyFont', strokeWidth: 3 })),
  a(freeText(5, { fontFamily: 'symbol' }), { text: undefined }),
  a(freeText(6, { fontFamily: 'zapf-dingbats', flags: { ...base(0, 'x').flags as object, hidden: true } })),
  a(freeText(7, { fontFamily: 'helvetica-bold', pageObjectNumber: 2 })),
  a(square(8)),
];
cases.push(
  c('buildTextItems', 'page', model(texts, { editing: 'obj:2', selected: ['obj:6'] }), pr(1)),
  c('buildTextItems', 'view', model(texts), pr(1), { zoom: 2, rotation: 90 }),
  c('buildTextItems', 'other-page', model(texts), pr(2)),
  c('buildTextItems', 'empty', model([]), pr(1)),
  c('buildTextItems', 'rich', model(richTexts), pr(1)),
);

// ── icon placement ────────────────────────────────────────────────────────────

const geom = { rect: { left: 10, bottom: 20, right: 30, top: 40 }, rotation: null, unrotatedRect: null };
const noteDefaults = defaultsFor({ ...initialModel, defaults: { note: { icon: 'key', color: '#facc15', opacity: 0.5 } } }, 'note');
const bare = defaultsFor(initialModel, 'nothing');
const file = { data: new Uint8Array([1, 2, 3]), name: 'a.bin', mimeType: 'application/octet-stream' };
cases.push(
  c('iconPlacementDraft', 'note', 'text', geom, noteDefaults, { noZoom: true, noRotate: true }, null),
  c('iconPlacementDraft', 'note-default-icon', 'text', geom, bare, undefined, null),
  c('iconPlacementDraft', 'attachment', 'file-attachment', geom, noteDefaults, { print: false }, file),
  c('iconPlacementDraft', 'attachment-default-icon', 'file-attachment', geom, bare, undefined, file),
  c('iconPlacementDraft', 'attachment-no-file', 'file-attachment', geom, bare, undefined, null),
);

// ── reducer ───────────────────────────────────────────────────────────────────

const state = initialAnnotationState();
cases.push(
  c('initialAnnotationState', 'default'),
  c('initialAnnotationState', 'configured', {
    snap: { guides: false, rotationAngles: [0, 45] },
    chrome: { accent: '#e91e63', outline: { style: 'dashed' }, knob: { stalk: false, stroke: '#000' }, guides: { enabled: false } },
  }),
  c('mergeChrome', 'deep', DEFAULT_CHROME, {
    accent: '#111111',
    outline: { width: 2, color: '#222222' },
    handles: { size: 10, stroke: '#333333' },
    knob: { offset: 40, fill: '#444444' },
    guides: { axisColor: '#555555', indicatorOpacity: 1, style: 'dashed' },
  }),
  c('mergeChrome', 'empty', DEFAULT_CHROME, {}),
  c('annotationReducer', 'set-model', state, { type: 'SET_MODEL', model: { ...initialModel, seq: 5 } }),
  c('annotationReducer', 'set-hydration', state, { type: 'SET_HYDRATION', hydration: { status: 'complete' } }),
  c('annotationReducer', 'set-chrome', state, { type: 'SET_CHROME', patch: { accent: '#000000', handles: { hitSize: 30 } } }),
  c('annotationReducer', 'set-ghost', state, {
    type: 'SET_TOOL_GHOST',
    ghost: { page: pr(1), box: { x: 1, y: 2, width: 3, height: 4 }, rot: 0, kind: 'image' },
  }),
  c('annotationReducer', 'set-text-selection', state, { type: 'SET_TEXT_SELECTION', selection: { id: 'obj:1', start: 1, end: 3 } }),
  c('annotationReducer', 'unknown-action', state, { type: 'NOPE' }),
  c('annotationReducer', 'stamp-armed', { ...state, stampArmEpoch: 3, toolGhost: { page: pr(1), box: { x: 0, y: 0, width: 1, height: 1 }, rot: 0, kind: 'image' } }, { type: 'STAMP_ARM_CHANGED' }),
);

// ── rich text ─────────────────────────────────────────────────────────────────

/** Registered fonts, as the local engine's `fonts.list()` answers them. */
const FONTS = [
  { key: 'inter-regular', familyName: 'Inter', weight: 400, italic: false },
  { key: 'inter-bold', familyName: 'Inter', weight: 700, italic: false },
  { key: 'inter-italic', familyName: 'Inter', weight: 400, italic: true },
  { key: 'noto-sans-light', familyName: 'Noto Sans', weight: 300, italic: false },
];
const TEXT_STYLE = { fontFamily: 'helvetica', fontSize: 12, fontColor: '#ef4444', textAlign: 'left' };
for (const font of ['helvetica', 'times-bold-italic', 'zapf-dingbats', 'inter-bold', 'MyFont']) {
  cases.push(c('faceForFont', font, font, FONTS));
}
cases.push(c('faceForFont', 'no-fonts', 'inter-bold'));
for (const [label, face] of [
  ['registered-exact', { family: 'Inter', weight: 700 }],
  ['registered-closest', { family: 'inter', weight: 600, italic: true }],
  ['registered-italic-penalty', { family: 'Noto_Sans', italic: true }],
  ['registered-tie', { family: 'Inter', weight: 550 }],
  ['standard-regular', { family: 'Arial' }],
  ['standard-bold-italic', { family: 'Times New Roman', weight: 700, italic: true }],
  ['standard-one-face', { family: 'Symbol', weight: 700 }],
  ['dingbats', { family: 'dingbats', italic: true }],
  ['unknown', { family: 'Comic Sans' }],
] as const) {
  cases.push(c('fontForFace', label, face, FONTS));
}
cases.push(c('fontForFace', 'no-fonts', { family: 'Inter', weight: 700 }));
for (const family of ['Helvetica', 'courier new', 'Inter', 'Unknown Face']) {
  cases.push(c('cssFontFamilyForFace', family, family, FONTS));
}
cases.push(c('cssFontFamilyForFace', 'no-fonts', 'Inter'));
for (const font of ['helvetica-bold', 'courier', 'symbol', 'inter-bold']) cases.push(c('cssFontFamilyForFont', font, font));
cases.push(c('familyKey', 'punctuated', " Times-New_Roman 'PS' "));
cases.push(
  c('bodyFromTextStyle', 'plain', TEXT_STYLE),
  c('bodyFromTextStyle', 'toggles', { ...TEXT_STYLE, fontFamily: 'times-italic', bold: true, underline: true, textAlign: 'center' }),
  c('bodyFromTextStyle', 'registered', { ...TEXT_STYLE, fontFamily: 'inter-italic' }, FONTS),
  c('bodyFromTextStyle', 'unknown-font', { ...TEXT_STYLE, fontFamily: 'MyFont', italic: false }),
  // An integral size past i64 stays the number it is.
  c('bodyFromTextStyle', 'huge-size', { ...TEXT_STYLE, fontSize: 1e20 }),
  c('richDocOf', 'dto-rich', richTexts[0]),
  c('richDocOf', 'dto-plain', a(freeText(14, { contents: 'one\rtwo' }))),
  c('richDocOf', 'no-text-style', { ...a(freeText(15)), text: undefined, data: undefined }),
  c('richDocOf', 'not-free-text', a(square(16))),
  c('textCommitPatch', 'strips-defaults', richTexts[0], PARAS),
  c('textCommitPatch', 'plain', a(freeText(17)), [{ runs: [{ text: 'hi' }], align: 'left', dir: 'ltr' }]),
  c('stripBodyDefaults', 'mixed', PARAS, { align: 'center', dir: 'ltr' }),
  c('stripBodyDefaults', 'none', [{ runs: [{ text: 'a' }] }], { align: 'left', dir: 'ltr' }),
  c('runDeltaForProps', 'every-key', { fontFamily: 'times-bold', fontSize: 14, fontColor: '#abcdef', bold: false, italic: true, underline: true, color: '#000000', opacity: 0.5 }),
  c('runDeltaForProps', 'registered-font', { fontFamily: 'inter-italic', bold: true, underline: false }, FONTS),
  c('runDeltaForProps', 'unknown-font', { fontFamily: 'MyFont', fontSize: undefined }),
  c('runsInRange', 'across', { paragraphs: PARAS }, { start: 2, end: 9 }),
  c('runsInRange', 'reversed', { paragraphs: PARAS }, { start: 9, end: 2 }),
  c('runsInRange', 'empty', { paragraphs: PARAS }, { start: 4, end: 4 }),
  c('runsInRange', 'to-the-end', { paragraphs: PARAS }, { start: 0, end: 12 }),
  // A range that starts on a run boundary skips the run before it; an empty
  // run inside a range contributes nothing.
  c('runsInRange', 'second-run', { paragraphs: PARAS }, { start: 3, end: 5 }),
  c('runsInRange', 'empty-run-inside', { paragraphs: [{ runs: [{ text: 'ab' }, { text: '', style: { weight: 700 } }, { text: 'cd', style: { italic: true } }] }] }, { start: 0, end: 4 }),
  c('rangeProps', 'mixed', { body: body(400, false, []), paragraphs: PARAS }, { start: 2, end: 9 }),
  c('rangeProps', 'one-run', { body: body(400, false, []), paragraphs: PARAS }, { start: 0, end: 2 }),
  c('rangeProps', 'caret', { body: body(700, true, ['underline']), paragraphs: PARAS }, { start: 3, end: 3 }),
  c('rangeProps', 'registered', { body: body(700, false, [], { family: 'Inter' }), paragraphs: PARAS }, { start: 0, end: 1 }, FONTS),
  // A body with no size or color (a foreign /RC): the size reads back
  // undefined and drops out of `values`; the color throws.
  c('rangeProps', 'no-size', { body: { ...body(400, false, []), size: undefined }, paragraphs: PARAS }, { start: 0, end: 2 }),
  c('rangeProps', 'no-size-mixed', { body: { ...body(400, false, []), size: undefined }, paragraphs: PARAS }, { start: 0, end: 12 }),
  c('rangeProps', 'no-color', { body: { ...body(400, false, []), color: undefined }, paragraphs: PARAS }, { start: 0, end: 2 }),
  // The run that differs first stops the read before the one with no color.
  c('rangeProps', 'no-color-after-a-difference', { body: { ...body(400, false, []), color: undefined }, paragraphs: [{ runs: [{ text: 'ab', style: { color: '#111111' } }, { text: 'cd', style: { color: '#222222' } }, { text: 'ef' }] }] }, { start: 0, end: 6 }),
);

// ── page-space create input ───────────────────────────────────────────────────

const R = { x: 10, y: 20, width: 30, height: 40 };
const Q = { upperStart: { x: 1, y: 2 }, upperEnd: { x: 3, y: 2 }, lowerStart: { x: 1, y: 4 }, lowerEnd: { x: 3, y: 4 } };
for (const [label, input] of [
  ['square', { subtype: 'square', bounds: R }],
  ['circle-rotated', { subtype: 'circle', bounds: R, rotation: 30 }],
  ['square-negative', { subtype: 'square', bounds: { ...R, width: -1 } }],
  ['square-nan', { subtype: 'square', bounds: { ...R, x: Number.NaN } }],
  ['square-zero-width', { subtype: 'square', bounds: { ...R, width: 0 } }],
  ['square-zero-height', { subtype: 'square', bounds: { ...R, height: 0 } }],
  ['square-negative-height', { subtype: 'square', bounds: { ...R, height: -1 } }],
  ['square-rotation-zero', { subtype: 'square', bounds: R, rotation: 0 }],
  ['square-rotation-nan', { subtype: 'square', bounds: R, rotation: Number.NaN }],
  ['line', { subtype: 'line', from: { x: 1, y: 2 }, to: { x: 3, y: 4 } }],
  ['line-infinite', { subtype: 'line', from: { x: 1, y: Number.POSITIVE_INFINITY }, to: { x: 3, y: 4 } }],
  ['polygon', { subtype: 'polygon', vertices: [{ x: 0, y: 0 }, { x: 5, y: 0 }, { x: 5, y: 5 }] }],
  ['polygon-short', { subtype: 'polygon', vertices: [{ x: 0, y: 0 }, { x: 5, y: 0 }] }],
  ['polyline', { subtype: 'polyline', vertices: [{ x: 0, y: 0 }, { x: 5, y: 0 }] }],
  ['polyline-nan', { subtype: 'polyline', vertices: [{ x: 0, y: 0 }, { x: Number.NaN, y: 0 }] }],
  ['ink', { subtype: 'ink', strokes: [[{ x: 0, y: 0 }], [{ x: 1, y: 1 }, { x: 2, y: 2 }]] }],
  ['ink-empty', { subtype: 'ink', strokes: [[{ x: 0, y: 0 }]] }],
  ['ink-nan', { subtype: 'ink', strokes: [[{ x: 0, y: 0 }, { x: 1, y: Number.NaN }]] }],
  ['highlight', { subtype: 'highlight', quads: [Q] }],
  ['squiggly-empty', { subtype: 'squiggly', quads: [] }],
  ['redact-quads', { subtype: 'redact', quads: [Q] }],
  ['redact-area', { subtype: 'redact', bounds: R }],
  ['redact-area-empty', { subtype: 'redact', bounds: { ...R, height: 0 } }],
  ['redact-area-nan', { subtype: 'redact', bounds: { ...R, x: Number.NaN } }],
  ['free-text', { subtype: 'free-text', bounds: R }],
  ['free-text-callout', { subtype: 'free-text', bounds: R, callout: { tip: { x: 0, y: 0 }, knee: { x: 5, y: 5 }, ending: 'open-arrow' } }],
  ['free-text-empty', { subtype: 'free-text', bounds: { ...R, width: 0 } }],
  ['stamp', { subtype: 'stamp', bounds: R }],
] as const) {
  cases.push(c('geometryFromInput', label, { page: pr(1), ...input }));
}
for (const px of [0, 127.5, 129, 1000, 5000]) cases.push(c('previewBucket', `${px}`, px));
cases.push(c('previewBucket', 'capped', 3000, 2048));

// ── comment threads ───────────────────────────────────────────────────────────

const t = (n: number, over: Record<string, unknown> = {}) => textNote(n, over);
const at = (d: string) => `2024-01-0${d}T00:00:00Z`;
const THREADS = [
  square(1, { contents: 'root', created: at('1'), nm: 'root-nm' }),
  t(2, { inReplyTo: objRef(1), replyType: 'reply', contents: 'first', created: at('3') }),
  t(3, { inReplyTo: { kind: 'nm', page: pr(1), nm: 'root-nm' }, contents: 'by name', created: at('2') }),
  t(4, { inReplyTo: objRef(2), contents: 'nested', modified: at('4') }),
  t(5, { inReplyTo: objRef(1), contents: 'undated' }),
  t(6, { inReplyTo: objRef(1), state: 'accepted', stateModel: 'review', userId: 'u1', modified: at('5') }),
  t(7, { inReplyTo: objRef(6), state: 'rejected', stateModel: 'review', userId: 'u1', modified: at('6') }),
  t(8, { inReplyTo: objRef(1), state: 'completed', author: 'Bea', created: at('4') }),
  t(9, { inReplyTo: objRef(1), state: 'marked', userId: 'u2' }),
  t(10, { inReplyTo: objRef(1), stateModel: 'marked', userId: 'u3', modified: at('7') }),
  t(11, { inReplyTo: objRef(1), state: 'marked', stateModel: 'marked', userId: 'u3', modified: at('6') }),
  t(12, { inReplyTo: objRef(1), stateModel: 'review', author: '' }),
  t(13, { inReplyTo: objRef(1), state: 'custom-thing' }),
  t(14, { inReplyTo: objRef(1), stateModel: 'custom-model' }),
  t(15, { inReplyTo: objRef(1), state: 'ok', stateModel: 'custom-model', userId: 'u4' }),
  markup('strikeout', 16, { inReplyTo: objRef(1), replyType: 'group' }),
  t(17, { inReplyTo: objRef(99), contents: 'orphan' }),
  t(18, { inReplyTo: objRef(19), contents: 'cycle a' }),
  t(19, { inReplyTo: objRef(18), contents: 'cycle b' }),
  t(20, { inReplyTo: objRef(98), state: 'accepted' }),
  link(21),
  widget(22, 'text'),
  base(23, 'unsupported'),
  t(24, { state: '', stateModel: '', contents: 'empty states are no states' }),
  t(25, { inReplyTo: objRef(1), state: 'none', userId: 'u1' }),
  t(26, { inReplyTo: objRef(1), state: 'unmarked', stateModel: 'marked', userId: 'u2', modified: at('1') }),
];
const nmRef = (nm: string) => ({ kind: 'nm', page: pr(1), nm });
const ORDERING = [
  square(60, { ref: nmRef('first') }),
  square(61, { ref: nmRef('second') }),
  t(62, { inReplyTo: nmRef('second'), contents: 'to second' }),
  t(63, { inReplyTo: nmRef('first'), contents: 'to first' }),
  square(64, { contents: 'sorting root' }),
  t(90, { inReplyTo: objRef(64), contents: 'late index, dated', created: at('9') }),
  t(70, { inReplyTo: objRef(64), contents: 'undated' }),
  t(85, { inReplyTo: objRef(64), contents: 'same date, high index', created: at('8') }),
  t(86, { inReplyTo: objRef(64), contents: 'same date, same index: input order', created: at('8'), index: 80 }),
  t(80, { inReplyTo: objRef(64), contents: 'same date, low index', created: at('8') }),
  t(81, { inReplyTo: objRef(64), contents: 'undated, high index' }),
  square(65, { contents: 'review default root' }),
  t(66, { inReplyTo: objRef(65), stateModel: 'review', userId: 'u5' }),
];
// ── engine-core binaryMetadata ────────────────────────────────────────────────

const bin = (bytes: number[]) => ({ $bytes: Buffer.from(bytes).toString('base64') });
const PNG_SIG = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52];
const SOF = [0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x1e, 0x00, 0x28];
cases.push(
  c('sniffBinaryMetadata', 'png-wide', bin([...PNG_SIG, 0, 1, 17, 112, 0, 1, 0, 1])),
  c('sniffBinaryMetadata', 'png-trailing-bytes', bin([...PNG_SIG, 0, 0, 0, 100, 0, 0, 0, 50, 1, 2, 3, 4, 5, 6])),
  c('sniffBinaryMetadata', 'png-zero-width', bin([...PNG_SIG, 0, 0, 0, 0, 0, 0, 0, 50])),
  c('sniffBinaryMetadata', 'jpeg-sof-ends-the-buffer', bin([0xff, 0xd8, ...SOF])),
  c('sniffBinaryMetadata', 'jpeg-sof-truncated', bin([0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00])),
  c('sniffBinaryMetadata', 'jpeg-zero-width', bin([0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x1e, 0x00, 0x00])),
  c('sniffBinaryMetadata', 'jpeg-soi-after-a-segment', bin([0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0, 0, 0xff, 0xd8, ...SOF])),
  c('sniffBinaryMetadata', 'jpeg-long-segment', bin([0xff, 0xd8, 0xff, 0xe0, 0x01, 0x02, ...new Array(256).fill(0), ...SOF])),
  c('sniffBinaryMetadata', 'jpeg-second-segment', bin([0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0, 0, 0xff, 0xe1, 0x00, 0x06, 0xaa, 0xbb, 0xcc, 0xdd, ...SOF])),
  c('sniffBinaryMetadata', 'pdf-four-bytes', bin([0x25, 0x50, 0x44, 0x46])),
  c('sniffBinaryMetadata', 'three-bytes', bin([0x25, 0x50, 0x44])),
);

cases.push(
  c('buildCommentThreads', 'nm-refs-and-ordering', ORDERING, {}),
  c('buildCommentThreads', 'everything', THREADS, {}),
  c('buildCommentThreads', 'mine', THREADS, { currentUserId: 'u1' }),
  c('buildCommentThreads', 'mine-absent', THREADS, { currentUserId: 'nobody' }),
  c('buildCommentThreads', 'empty', [], {}),
);

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'plugins/annotation-pure.json',
    describes:
      'packages/plugin/annotation: the tool registry, the free-text projection, icon placement, the reducer, the rich-text policy, page-space create input, the ghost preview bucket, and engine-core buildCommentThreads and sniffBinaryMetadata.',
    functions: {
      buildToolRegistry: buildToolRegistry as never,
      isTouchDirect: ((tags: string[]) => isTouchDirect(new Set(tags))) as never,
      buildTextItems: buildTextItems as never,
      iconPlacementDraft: iconPlacementDraft as never,
      initialAnnotationState: initialAnnotationState as never,
      mergeChrome: mergeChrome as never,
      annotationReducer: annotationReducer as never,
      buildCommentThreads: buildCommentThreads as never,
      sniffBinaryMetadata: ((b: Uint8Array) => sniffBinaryMetadata(new Uint8Array(b).buffer)) as never,
      // A `FontLookup` is a function; the cases pass the list it answers.
      faceForFont: ((font: string, fonts?: never[]) => faceForFont(font, fonts && (() => fonts))) as never,
      fontForFace: ((face: never, fonts?: never[]) => fontForFace(face, fonts && (() => fonts))) as never,
      cssFontFamilyForFace: ((family: string, fonts?: never[]) => cssFontFamilyForFace(family, fonts && (() => fonts))) as never,
      cssFontFamilyForFont: cssFontFamilyForFont as never,
      familyKey: familyKey as never,
      bodyFromTextStyle: ((t: never, fonts?: never[]) => bodyFromTextStyle(t, fonts && (() => fonts))) as never,
      richDocOf: richDocOf as never,
      textCommitPatch: textCommitPatch as never,
      stripBodyDefaults: stripBodyDefaults as never,
      runDeltaForProps: ((p: never, fonts?: never[]) => runDeltaForProps(p, fonts && (() => fonts))) as never,
      runsInRange: runsInRange as never,
      rangeProps: ((d: never, r: never, fonts?: never[]) => rangeProps(d, r, fonts && (() => fonts))) as never,
      geometryFromInput: geometryFromInput as never,
      previewBucket: previewBucket as never,
    },
    cases,
  },
];
