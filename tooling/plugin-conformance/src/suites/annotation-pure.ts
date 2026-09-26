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
import { iconPlacementDraft } from '../../../../packages/plugin/annotation/src/placement';
import {
  annotationReducer,
  initialAnnotationState,
  mergeChrome,
  DEFAULT_CHROME,
} from '../../../../packages/plugin/annotation/src/reducer';
import { buildToolRegistry, isTouchDirect } from '../../../../packages/plugin/annotation/src/tools';
import { fromDTO } from '../../../../packages/plugin/annotation/src/repository';
import { initialModel, defaultsFor } from '../../../../packages/core/annotation/src/update';

import { CROP, callout, freeText, objRef, square, textNote, markup, link, widget, base } from './annotation-fixtures';
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
  c('buildTextItems', 'page', model(texts, { editing: 'obj:2', selected: ['obj:6'] }), 1),
  c('buildTextItems', 'view', model(texts), 1, { zoom: 2, rotation: 90 }),
  c('buildTextItems', 'other-page', model(texts), 2),
  c('buildTextItems', 'empty', model([]), 1),
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
    ghost: { pon: 1, box: { x: 1, y: 2, width: 3, height: 4 }, rot: 0, kind: 'image' },
  }),
  c('annotationReducer', 'stamp-armed', { ...state, stampArmEpoch: 3, toolGhost: { pon: 1, box: { x: 0, y: 0, width: 1, height: 1 }, rot: 0, kind: 'image' } }, { type: 'STAMP_ARM_CHANGED' }),
);

// ── comment threads ───────────────────────────────────────────────────────────

const t = (n: number, over: Record<string, unknown> = {}) => textNote(n, over);
const at = (d: string) => `2024-01-0${d}T00:00:00Z`;
const THREADS = [
  square(1, { contents: 'root', created: at('1'), nm: 'root-nm' }),
  t(2, { inReplyTo: objRef(1), replyType: 'reply', contents: 'first', created: at('3') }),
  t(3, { inReplyTo: { kind: 'nm', pageObjectNumber: 1, nm: 'root-nm' }, contents: 'by name', created: at('2') }),
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
const nmRef = (nm: string) => ({ kind: 'nm', pageObjectNumber: 1, nm });
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
      'packages/plugin/annotation: the tool registry, the free-text projection, icon placement, the reducer, and engine-core buildCommentThreads and sniffBinaryMetadata.',
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
    },
    cases,
  },
];
