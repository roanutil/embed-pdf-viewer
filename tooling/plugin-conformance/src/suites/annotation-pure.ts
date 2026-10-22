/**
 * `packages/plugin/annotation`: the pure modules beside the repository. The
 * tool registry (`tools.ts`), the free-text projection (`text-item.ts`), the
 * icon placement draft (`placement.ts`), the named state transitions and shared settings, and the comment-thread
 * composer the `comments` lens is built on (`engine-core`'s
 * `buildCommentThreads`, which the Rust crate transcribes). Function cases;
 * replayed by `crates/plugins/annotation/tests/conformance.rs`.
 */
import { buildCommentThreads } from '../../../../packages/engine/core/src/runtime';
import { sniffBinaryMetadata } from '../../../../packages/engine/core/src/resource/binaryMetadata';
import { buildTextItems } from '../../../../packages/plugin/annotation/src/text-item';
import { iconPlacement as iconPlacementDraft } from '../../../../packages/plugin/annotation/src/write/placement';
import * as annotationModel from '../../../../packages/plugin/annotation/src/model';
import {
  buildToolRegistry,
  toolTouchOf as isTouchDirect,
} from '../../../../packages/plugin/annotation/src/tools/definitions';
import { fromDTO as upstreamFromDTO } from '../../../../packages/core/annotation/src/record/index';
import {
  cssFontFamilyForFace,
  familyKey,
  fontForFace,
  rangeProps,
  runDeltaForFields as runDeltaForProps,
  runsInRange,
  stripBodyDefaults,
  textCommitPatch,
} from '../../../../packages/plugin/annotation/src/rich-text';
import {
  bodyFromTextStyle,
  faceForFont,
  richDocOf,
} from '../../../../packages/core/annotation/src/richtext';
import { cssFontFamilyForFont } from '../../../../packages/core/annotation/src/font-css';
import { previewBucket } from '../../../../packages/plugin/annotation/src/host-contract';
import { initialModel, defaultsFor } from '../../../../packages/core/annotation/src/update';
import { mergeSettings, type DeepPartial } from '../../../../packages/core/main/src/settings';
import { ANNOTATION_DEFAULTS } from '../../../../packages/plugin/annotation/src/contract';
import { AnnotationDraftSchema } from '../../../../packages/engine/core/src/annotation/kinds';
import type { Annotation, RichTextBody, RichTextParagraph } from '@embedpdf/engine-core/runtime';
import type {
  Model,
  ModelAnnotation,
  Session,
} from '../../../../packages/core/annotation/src/types';
import { initialSession } from '../../../../packages/core/annotation/src/update';

import {
  callout,
  freeText,
  objRef,
  baseRef,
  pr,
  square,
  textNote,
  markup,
  link,
  widget,
  unsupported,
} from './annotation-current-fixtures';
import type { FunctionCase, Suite } from '../vectors';

export const migration: { before: string; after: string | null; reason?: string }[] = [];
const renamed: Record<string, string> = {
  isTouchDirect: 'toolTouchOf',
  iconPlacementDraft: 'iconPlacement',
  runDeltaForProps: 'runDeltaForFields',
  geometryFromInput: 'parseAnnotationDraft',
  mergeChrome: 'mergeSettings',
};
function c(fn: string, label: string, ...args: unknown[]): FunctionCase {
  const before = `${fn}/${label}`;
  const current = renamed[fn] ?? fn;
  const after = `${current}/${label}`;
  migration.push({ before, after });
  return { name: after, fn: current, args };
}
function transition(label: string, fn: string, ...args: unknown[]): FunctionCase {
  const before = `annotationReducer/${label}`;
  const after = `${fn}/${label}`;
  migration.push({ before, after });
  return { name: after, fn, args };
}
const retire = (before: string, reason: string) => migration.push({ before, after: null, reason });
const fromDTO = upstreamFromDTO;
const initialAnnotationState = annotationModel.initialAnnotationState;
const DEFAULT_CHROME = ANNOTATION_DEFAULTS.chrome;

const cases: FunctionCase[] = [];

// ── tools ─────────────────────────────────────────────────────────────────────

cases.push(
  c('buildToolRegistry', 'built-ins'),
  c('buildToolRegistry', 'configure-built-in', [
    {
      id: 'ink',
      defaults: { strokeWidth: 9 },
      ink: { straighten: { deviationThreshold: 0.2, axisSnapDegrees: 10 } },
    },
    { id: 'line', defaults: { lineEndings: { start: 'circle' } } },
    { id: 'square', enables: ['annotation-draw'], meta: { label: 'Box' }, flags: { locked: true } },
    { id: 'note', flags: { print: false }, meta: { icon: 'n' } },
  ]),
  c('buildToolRegistry', 'presets', [
    {
      id: 'arrow',
      extends: 'line',
      defaults: { lineEndings: { end: 'open-arrow' } },
      cursor: 'cell',
    },
    {
      id: 'thick-arrow',
      extends: 'arrow',
      defaults: { strokeWidth: 12 },
      meta: { label: 'Thick' },
    },
    {
      id: 'pen',
      extends: 'ink-highlight',
      ink: { groupStrokesMs: 0 },
      upright: true,
      clickCreate: false,
      ghost: false,
    },
    {
      id: 'big-free',
      extends: 'free-text',
      clickCreate: { width: 300, height: 100 },
      ghost: { opacity: 0.4 },
    },
    { id: 'insert', extends: 'insert-text', preset: 'mine' },
  ]),
  c('buildToolRegistry', 'custom-kind', [
    { id: 'widget-sig', subtype: 'widget-signature', defaults: { opacity: 0.5, bogus: 1 } },
  ]),
  c('buildToolRegistry', 'bare-id', [{ id: 'mystery' }]),
  c('buildToolRegistry', 'own-flags', [
    { id: 'locked-box', subtype: 'square', flags: { locked: true } },
  ]),
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
  ).map(([kind, defaults]) =>
    c('buildToolRegistry', `bad-default-${kind}`, [{ id: `bad-${kind}`, subtype: kind, defaults }]),
  ),
  c('buildToolRegistry', 'bad-default-inherited', [
    { id: 'marker', extends: 'highlight', subtype: 'caret' },
  ]),
  c('buildToolRegistry', 'widget-kinds', [
    { id: 'field-text', subtype: 'widget-text', defaults: { fontSize: 10, borderStyle: 'solid' } },
    { id: 'field-choice', subtype: 'widget-choice', defaults: { textAlign: 'center' } },
    { id: 'field-check', subtype: 'widget-toggle', defaults: { interiorColor: null } },
    { id: 'stamped', subtype: 'stamp', defaults: {} },
    {
      id: 'docs-link',
      extends: 'link',
      defaults: { target: { kind: 'uri', uri: 'https://docs' } },
    },
    { id: 'pin', subtype: 'text', defaults: { icon: 'pin' } },
    { id: 'clip', subtype: 'file-attachment', defaults: { icon: 'tag' } },
    { id: 'poly', subtype: 'polyline', defaults: { lineEndings: { start: 'square' } } },
    { id: 'callout2', subtype: 'free-text-callout', defaults: { fontFamily: 'courier' } },
  ]),
  c('isTouchDirect', 'draw', new Set(['annotation-draw', 'annotation-edit'])),
  c('isTouchDirect', 'select', new Set(['text-select'])),
  c('isTouchDirect', 'place', new Set(['annotation-place', 'annotation-edit'])),
);

// ── text items ────────────────────────────────────────────────────────────────

// These are session-owned live text records; fromDTO alone deliberately reads baked.
const a = (dto: Annotation): ModelAnnotation => ({ ...fromDTO(dto), source: 'vector' });
const model = (annots: ModelAnnotation[], over: Partial<Model> = {}): Model => ({
  ...initialModel,
  byId: Object.fromEntries(annots.map((record) => [record.id, record])),
  order: annots.map((record) => record.id),
  ...over,
});
/** A rich body: the engine's resolved defaults with the three toggles set. */
const body = (
  weight: number,
  italic: boolean,
  decoration: RichTextBody['decoration'],
  over: Partial<RichTextBody> = {},
): RichTextBody => ({
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
const PARAS: RichTextParagraph[] = [
  { runs: [{ text: 'Hel' }, { text: 'lo', style: { weight: 700 } }], align: 'left', dir: 'ltr' },
  {
    runs: [{ text: 'world', style: { italic: true, decoration: ['underline'], color: '#0000FF' } }],
    align: 'center',
  },
  { runs: [{ text: '' }, { text: 'x', style: { family: 'Courier', size: 20 } }], dir: 'rtl' },
];
const richTexts = [
  a(freeText(11, { richText: { body: body(700, true, ['underline']), paragraphs: PARAS } })),
  a(freeText(12, { richText: { body: body(400, false, []), paragraphs: PARAS } })),
  a(
    freeText(13, {
      fontFamily: 'helvetica',
      fontSize: 9,
      fontColor: '#000000',
      textAlign: 'right',
      richText: {
        body: body(700, true, ['underline'], { size: 9, color: '#000000', align: 'right' }),
        paragraphs: [{ runs: [{ text: 'Hello' }] }],
      },
    }),
  ),
];
const texts = [
  a(freeText(1)),
  a(freeText(2, { fontFamily: 'courier', fontColor: '#0000ff', interiorColor: '#ffff00' })),
  a(
    freeText(3, {
      fontFamily: 'times-roman',
      rotation: 30,
      box: { x: 110, y: 110, width: 80, height: 80 },
      contents: null,
    }),
  ),
  a(callout(4, { fontFamily: 'MyFont', strokeWidth: 3 })),
  a(
    freeText(5, {
      fontFamily: 'symbol',
      contents: '',
      richText: {
        body: body(400, false, [], { family: 'Symbol' }),
        paragraphs: [{ runs: [{ text: '' }] }],
      },
    }),
  ),
  a(freeText(6, { fontFamily: 'zapf-dingbats', hidden: true })),
  a(freeText(7, { fontFamily: 'helvetica-bold', page: pr(2) })),
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

const geom = { rect: { x: 10, y: 20, width: 20, height: 20 } };
const noteDefaults = defaultsFor(
  { ...initialModel, defaults: { note: { icon: 'key', color: '#facc15', opacity: 0.5 } } },
  'note',
);
const bare = defaultsFor(initialModel, 'nothing');
const file = {
  data: new Uint8Array([1, 2, 3]),
  name: 'a.bin',
  mimeType: 'application/octet-stream',
};
cases.push(
  c(
    'iconPlacementDraft',
    'note',
    'text',
    geom,
    noteDefaults,
    { noZoom: true, noRotate: true },
    null,
  ),
  c('iconPlacementDraft', 'note-default-icon', 'text', geom, bare, undefined, null),
  c(
    'iconPlacementDraft',
    'attachment',
    'file-attachment',
    geom,
    noteDefaults,
    { print: false },
    file,
  ),
  c(
    'iconPlacementDraft',
    'attachment-default-icon',
    'file-attachment',
    geom,
    bare,
    undefined,
    file,
  ),
  c('iconPlacementDraft', 'attachment-no-file', 'file-attachment', geom, bare, undefined, null),
);

// ── current state and shared settings transitions ──────────────────────────
const state = initialAnnotationState();
cases.push(
  c('initialAnnotationState', 'default'),
  // Settings are shared by the registration now, rather than stored in AnnotationState.
  {
    name: 'mergeSettings/configured',
    fn: 'mergeSettings',
    args: [
      ANNOTATION_DEFAULTS,
      {
        snap: { alignment: false, rotationAngles: [0, 45] },
        chrome: {
          accent: '#e91e63',
          outline: { style: 'dashed' },
          rotationHandle: { stalk: false, stroke: '#000000' },
          guides: { enabled: false },
        },
      } satisfies DeepPartial<typeof ANNOTATION_DEFAULTS>,
    ],
  },
  {
    name: 'withSnap/current-settings',
    fn: 'withSnap',
    args: [state, { ...initialSession.snap, alignment: false, rotationAngles: [0, 45] }],
  },
  c('mergeChrome', 'deep', DEFAULT_CHROME, {
    accent: '#111111',
    outline: { width: 2, color: '#222222' },
    handles: { size: 10, stroke: '#333333' },
    rotationHandle: { offset: 40, fill: '#444444' },
    guides: { color: '#555555', rotationColor: '#666666', style: 'dashed' },
  }),
  c('mergeChrome', 'empty', DEFAULT_CHROME, {}),
  transition('set-model', 'withSession', state, {
    ...initialSession,
    objectNumbers: [100001, 100002, 100003, 100004, 100005],
  } satisfies Session),
  transition('set-chrome', 'mergeSettings', ANNOTATION_DEFAULTS, {
    chrome: { accent: '#000000', handles: { hitSize: 30 } },
  }),
  transition('set-ghost', 'setGhostAt', state, {
    toolId: 'stamp',
    page: pr(1),
    point: { x: 2.5, y: 4 },
  }),
  transition('set-text-selection', 'setTextSelection', state, { id: 'obj:1', start: 1, end: 3 }),
);
migration.push({ before: 'initialAnnotationState/configured', after: 'mergeSettings/configured' });
retire(
  'annotationReducer/set-hydration',
  '4fc1a32af: hydration is mirror resource state, not AnnotationState. Live loading/ready/error/refresh coverage belongs to b1b.',
);
retire(
  'annotationReducer/unknown-action',
  '4fc1a32af removes reduce/dispatch; there is no named transition for an unknown action. Identity-preserving real transitions are covered separately.',
);
retire(
  'annotationReducer/stamp-armed',
  '4fc1a32af removes STAMP_ARM_CHANGED and stampArmEpoch from model.ts; no stamp-arm epoch exists in AnnotationState. Live stamps arm/disarm coverage belongs to b1b.',
);

// ── rich text ─────────────────────────────────────────────────────────────────

/** Registered fonts, as the local engine's `fonts.list()` answers them. */
const FONTS = [
  { key: 'inter-regular', familyName: 'Inter', weight: 400, italic: false },
  { key: 'inter-bold', familyName: 'Inter', weight: 700, italic: false },
  { key: 'inter-italic', familyName: 'Inter', weight: 400, italic: true },
  { key: 'noto-sans-light', familyName: 'Noto Sans', weight: 300, italic: false },
];
const TEXT_STYLE = {
  fontFamily: 'helvetica',
  fontSize: 12,
  fontColor: '#ef4444',
  textAlign: 'left',
};
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
for (const font of ['helvetica-bold', 'courier', 'symbol', 'inter-bold'])
  cases.push(c('cssFontFamilyForFont', font, font));
cases.push(c('familyKey', 'punctuated', " Times-New_Roman 'PS' "));
cases.push(
  c('bodyFromTextStyle', 'plain', TEXT_STYLE),
  c('bodyFromTextStyle', 'toggles', {
    ...TEXT_STYLE,
    fontFamily: 'times-italic',
    bold: true,
    underline: true,
    textAlign: 'center',
  }),
  c('bodyFromTextStyle', 'registered', { ...TEXT_STYLE, fontFamily: 'inter-italic' }, FONTS),
  c('bodyFromTextStyle', 'unknown-font', { ...TEXT_STYLE, fontFamily: 'MyFont', italic: false }),
  // An integral size past i64 stays the number it is.
  c('bodyFromTextStyle', 'huge-size', { ...TEXT_STYLE, fontSize: 1e20 }),
  c('richDocOf', 'dto-rich', richTexts[0].annotation),
  c(
    'richDocOf',
    'dto-plain',
    Object.assign(freeText(14, { contents: 'one\rtwo' }), { richText: undefined }),
  ),
  c('richDocOf', 'no-text-style', square(15, { contents: 'plain fallback' })),
  c('richDocOf', 'not-free-text', square(16)),
  c('textCommitPatch', 'strips-defaults', richTexts[0], PARAS),
  c('textCommitPatch', 'plain', a(freeText(17)), [
    { runs: [{ text: 'hi' }], align: 'left', dir: 'ltr' },
  ]),
  c('stripBodyDefaults', 'mixed', PARAS, { align: 'center', dir: 'ltr' }),
  c('stripBodyDefaults', 'none', [{ runs: [{ text: 'a' }] }], { align: 'left', dir: 'ltr' }),
  c('runDeltaForProps', 'every-key', {
    fontFamily: 'times-bold',
    fontSize: 14,
    fontColor: '#abcdef',
    bold: false,
    italic: true,
    underline: true,
    color: '#000000',
    opacity: 0.5,
  }),
  c(
    'runDeltaForProps',
    'registered-font',
    { fontFamily: 'inter-italic', bold: true, underline: false },
    FONTS,
  ),
  c('runDeltaForProps', 'unknown-font', { fontFamily: 'MyFont', fontSize: undefined }),
  c('runsInRange', 'across', { paragraphs: PARAS }, { start: 2, end: 9 }),
  c('runsInRange', 'reversed', { paragraphs: PARAS }, { start: 9, end: 2 }),
  c('runsInRange', 'empty', { paragraphs: PARAS }, { start: 4, end: 4 }),
  c('runsInRange', 'to-the-end', { paragraphs: PARAS }, { start: 0, end: 12 }),
  // A range that starts on a run boundary skips the run before it; an empty
  // run inside a range contributes nothing.
  c('runsInRange', 'second-run', { paragraphs: PARAS }, { start: 3, end: 5 }),
  c(
    'runsInRange',
    'empty-run-inside',
    {
      paragraphs: [
        {
          runs: [
            { text: 'ab' },
            { text: '', style: { weight: 700 } },
            { text: 'cd', style: { italic: true } },
          ],
        },
      ],
    },
    { start: 0, end: 4 },
  ),
  c('rangeProps', 'mixed', { body: body(400, false, []), paragraphs: PARAS }, { start: 2, end: 9 }),
  c(
    'rangeProps',
    'one-run',
    { body: body(400, false, []), paragraphs: PARAS },
    { start: 0, end: 2 },
  ),
  c(
    'rangeProps',
    'caret',
    { body: body(700, true, ['underline']), paragraphs: PARAS },
    { start: 3, end: 3 },
  ),
  c(
    'rangeProps',
    'registered',
    { body: body(700, false, [], { family: 'Inter' }), paragraphs: PARAS },
    { start: 0, end: 1 },
    FONTS,
  ),
  // Explicit malformed foreign bodies with no size or color: the size reads back
  // undefined and drops out of `values`; the color throws.
  c(
    'rangeProps',
    'no-size',
    { body: { ...body(400, false, []), size: undefined }, paragraphs: PARAS },
    { start: 0, end: 2 },
  ),
  c(
    'rangeProps',
    'no-size-mixed',
    { body: { ...body(400, false, []), size: undefined }, paragraphs: PARAS },
    { start: 0, end: 12 },
  ),
  c(
    'rangeProps',
    'no-color',
    { body: { ...body(400, false, []), color: undefined }, paragraphs: PARAS },
    { start: 0, end: 2 },
  ),
  // The run that differs first stops the read before the one with no color.
  c(
    'rangeProps',
    'no-color-after-a-difference',
    {
      body: { ...body(400, false, []), color: undefined },
      paragraphs: [
        {
          runs: [
            { text: 'ab', style: { color: '#111111' } },
            { text: 'cd', style: { color: '#222222' } },
            { text: 'ef' },
          ],
        },
      ],
    },
    { start: 0, end: 6 },
  ),
);

// ── page-space create input ───────────────────────────────────────────────────

const R = { x: 10, y: 20, width: 30, height: 40 };
const Q = {
  upperLeft: { x: 1, y: 2 },
  upperRight: { x: 3, y: 2 },
  lowerLeft: { x: 1, y: 4 },
  lowerRight: { x: 3, y: 4 },
};
for (const [label, input] of [
  ['square', { subtype: 'square', box: R }],
  ['circle-rotated', { subtype: 'circle', box: R, rotation: 30 }],
  ['square-negative', { subtype: 'square', box: { ...R, width: -1 } }],
  ['square-nan', { subtype: 'square', box: { ...R, x: Number.NaN } }],
  ['square-zero-width', { subtype: 'square', box: { ...R, width: 0 } }],
  ['square-zero-height', { subtype: 'square', box: { ...R, height: 0 } }],
  ['square-negative-height', { subtype: 'square', box: { ...R, height: -1 } }],
  ['square-rotation-zero', { subtype: 'square', box: R, rotation: 0 }],
  ['square-rotation-nan', { subtype: 'square', box: R, rotation: Number.NaN }],
  ['line', { subtype: 'line', linePoints: { start: { x: 1, y: 2 }, end: { x: 3, y: 4 } } }],
  [
    'line-infinite',
    {
      subtype: 'line',
      linePoints: { start: { x: 1, y: Number.POSITIVE_INFINITY }, end: { x: 3, y: 4 } },
    },
  ],
  [
    'polygon',
    {
      subtype: 'polygon',
      vertices: [
        { x: 0, y: 0 },
        { x: 5, y: 0 },
        { x: 5, y: 5 },
      ],
    },
  ],
  [
    'polygon-short',
    {
      subtype: 'polygon',
      vertices: [
        { x: 0, y: 0 },
        { x: 5, y: 0 },
      ],
    },
  ],
  [
    'polyline',
    {
      subtype: 'polyline',
      vertices: [
        { x: 0, y: 0 },
        { x: 5, y: 0 },
      ],
    },
  ],
  [
    'polyline-nan',
    {
      subtype: 'polyline',
      vertices: [
        { x: 0, y: 0 },
        { x: Number.NaN, y: 0 },
      ],
    },
  ],
  [
    'ink',
    {
      subtype: 'ink',
      inkList: [
        [{ x: 0, y: 0 }],
        [
          { x: 1, y: 1 },
          { x: 2, y: 2 },
        ],
      ],
    },
  ],
  ['ink-empty', { subtype: 'ink', inkList: [[{ x: 0, y: 0 }]] }],
  [
    'ink-nan',
    {
      subtype: 'ink',
      inkList: [
        [
          { x: 0, y: 0 },
          { x: 1, y: Number.NaN },
        ],
      ],
    },
  ],
  ['highlight', { subtype: 'highlight', quadPoints: [Q] }],
  ['squiggly-empty', { subtype: 'squiggly', quadPoints: [] }],
  ['redact-quads', { subtype: 'redact', quadPoints: [Q] }],
  ['redact-area', { subtype: 'redact', rect: R }],
  ['redact-area-empty', { subtype: 'redact', rect: { ...R, height: 0 } }],
  ['redact-area-nan', { subtype: 'redact', rect: { ...R, x: Number.NaN } }],
  ['free-text', { subtype: 'free-text', box: R }],
  [
    'free-text-callout',
    {
      subtype: 'free-text',
      box: R,
      intent: 'free-text-callout',
      calloutLine: [
        { x: 0, y: 0 },
        { x: 5, y: 5 },
        { x: 10, y: 20 },
      ],
      lineEnding: 'open-arrow',
    },
  ],
  ['free-text-empty', { subtype: 'free-text', box: { ...R, width: 0 } }],
  ['stamp', { subtype: 'stamp', box: R }],
] as const) {
  cases.push(c('geometryFromInput', label, input));
}
for (const px of [0, 127.5, 129, 1000, 5000]) cases.push(c('previewBucket', `${px}`, px));
cases.push(c('previewBucket', 'capped', 3000, 2048));

// ── comment threads ───────────────────────────────────────────────────────────

const t = textNote;
const at = (d: string) => `2024-01-0${d}T00:00:00Z`;
const THREADS = [
  square(1, { contents: 'root', createdAt: at('1'), nm: 'root-nm' }),
  t(2, { reply: { to: objRef(1), type: 'reply' }, contents: 'first', createdAt: at('3') }),
  t(3, { reply: { to: objRef(1), type: 'reply' }, contents: 'by name', createdAt: at('2') }),
  t(4, { reply: { to: objRef(2), type: 'reply' }, contents: 'nested', modifiedAt: at('4') }),
  t(5, { reply: { to: objRef(1), type: 'reply' }, contents: 'undated' }),
  t(6, {
    reply: { to: objRef(1), type: 'reply' },
    state: 'accepted',
    stateModel: 'review',
    userId: 'u1',
    modifiedAt: at('5'),
  }),
  t(7, {
    reply: { to: objRef(6), type: 'reply' },
    state: 'rejected',
    stateModel: 'review',
    userId: 'u1',
    modifiedAt: at('6'),
  }),
  t(8, {
    reply: { to: objRef(1), type: 'reply' },
    state: 'completed',
    author: 'Bea',
    createdAt: at('4'),
  }),
  t(9, { reply: { to: objRef(1), type: 'reply' }, state: 'marked', userId: 'u2' }),
  t(10, {
    reply: { to: objRef(1), type: 'reply' },
    stateModel: 'marked',
    userId: 'u3',
    modifiedAt: at('7'),
  }),
  t(11, {
    reply: { to: objRef(1), type: 'reply' },
    state: 'marked',
    stateModel: 'marked',
    userId: 'u3',
    modifiedAt: at('6'),
  }),
  t(12, { reply: { to: objRef(1), type: 'reply' }, stateModel: 'review', author: '' }),
  t(13, { reply: { to: objRef(1), type: 'reply' }, state: 'custom-thing' }),
  t(14, { reply: { to: objRef(1), type: 'reply' }, stateModel: 'custom-model' }),
  t(15, {
    reply: { to: objRef(1), type: 'reply' },
    state: 'ok',
    stateModel: 'custom-model',
    userId: 'u4',
  }),
  markup('strikeout', 16, { reply: { to: objRef(1), type: 'group' } }),
  t(17, { reply: { to: objRef(99), type: 'reply' }, contents: 'orphan' }),
  t(18, { reply: { to: objRef(19), type: 'reply' }, contents: 'cycle a' }),
  t(19, { reply: { to: objRef(18), type: 'reply' }, contents: 'cycle b' }),
  t(20, { reply: { to: objRef(98), type: 'reply' }, state: 'accepted' }),
  link(21),
  widget(22, 'text'),
  unsupported(23),
  t(24, { state: '', stateModel: '', contents: 'empty states are no states' }),
  t(25, { reply: { to: objRef(1), type: 'reply' }, state: 'none', userId: 'u1' }),
  t(26, {
    reply: { to: objRef(1), type: 'reply' },
    state: 'unmarked',
    stateModel: 'marked',
    userId: 'u2',
    modifiedAt: at('1'),
  }),
];
const nmRef = (name: string) => baseRef(name === 'first' ? 0 : 1);
const ORDERING = [
  square(60, { ref: nmRef('first') }),
  square(61, { ref: nmRef('second') }),
  t(62, { reply: { to: nmRef('second'), type: 'reply' }, contents: 'to second' }),
  t(63, { reply: { to: nmRef('first'), type: 'reply' }, contents: 'to first' }),
  square(64, { contents: 'sorting root' }),
  t(90, {
    reply: { to: objRef(64), type: 'reply' },
    contents: 'late index, dated',
    createdAt: at('9'),
  }),
  t(70, { reply: { to: objRef(64), type: 'reply' }, contents: 'undated' }),
  t(85, {
    reply: { to: objRef(64), type: 'reply' },
    contents: 'same date, high index',
    createdAt: at('8'),
  }),
  t(86, {
    reply: { to: objRef(64), type: 'reply' },
    contents: 'same date, same index: input order',
    createdAt: at('8'),
    ref: baseRef(80),
  }),
  t(80, {
    reply: { to: objRef(64), type: 'reply' },
    contents: 'same date, low index',
    createdAt: at('8'),
  }),
  t(81, { reply: { to: objRef(64), type: 'reply' }, contents: 'undated, high index' }),
  square(65, { contents: 'review default root' }),
  t(66, { reply: { to: objRef(65), type: 'reply' }, stateModel: 'review', userId: 'u5' }),
];
// ── engine-core binaryMetadata ────────────────────────────────────────────────

const bin = (bytes: number[]) => ({ $bytes: Buffer.from(bytes).toString('base64') });
const PNG_SIG = [
  0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52,
];
const SOF = [0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x1e, 0x00, 0x28];
cases.push(
  c('sniffBinaryMetadata', 'png-wide', bin([...PNG_SIG, 0, 1, 17, 112, 0, 1, 0, 1])),
  c(
    'sniffBinaryMetadata',
    'png-trailing-bytes',
    bin([...PNG_SIG, 0, 0, 0, 100, 0, 0, 0, 50, 1, 2, 3, 4, 5, 6]),
  ),
  c('sniffBinaryMetadata', 'png-zero-width', bin([...PNG_SIG, 0, 0, 0, 0, 0, 0, 0, 50])),
  c('sniffBinaryMetadata', 'jpeg-sof-ends-the-buffer', bin([0xff, 0xd8, ...SOF])),
  c(
    'sniffBinaryMetadata',
    'jpeg-sof-truncated',
    bin([0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00]),
  ),
  c(
    'sniffBinaryMetadata',
    'jpeg-zero-width',
    bin([0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x1e, 0x00, 0x00]),
  ),
  c(
    'sniffBinaryMetadata',
    'jpeg-soi-after-a-segment',
    bin([0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0, 0, 0xff, 0xd8, ...SOF]),
  ),
  c(
    'sniffBinaryMetadata',
    'jpeg-long-segment',
    bin([0xff, 0xd8, 0xff, 0xe0, 0x01, 0x02, ...new Array(256).fill(0), ...SOF]),
  ),
  c(
    'sniffBinaryMetadata',
    'jpeg-second-segment',
    bin([
      0xff,
      0xd8,
      0xff,
      0xe0,
      0x00,
      0x04,
      0,
      0,
      0xff,
      0xe1,
      0x00,
      0x06,
      0xaa,
      0xbb,
      0xcc,
      0xdd,
      ...SOF,
    ]),
  ),
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
      'packages/plugin/annotation: the tool registry, the free-text projection, icon placement, the named state/settings transitions, the rich-text policy, page-space create input, the ghost preview bucket, and engine-core buildCommentThreads and sniffBinaryMetadata.',
    functions: {
      buildToolRegistry: buildToolRegistry as never,
      toolTouchOf: isTouchDirect as never,
      buildTextItems: buildTextItems as never,
      iconPlacement: iconPlacementDraft as never,
      initialAnnotationState: initialAnnotationState as never,
      mergeSettings: mergeSettings as never,
      withSnap: annotationModel.withSnap as never,
      withSession: annotationModel.withSession as never,
      setGhostAt: annotationModel.setGhostAt as never,
      setTextSelection: annotationModel.setTextSelection as never,
      buildCommentThreads: buildCommentThreads as never,
      sniffBinaryMetadata: ((b: Uint8Array) =>
        sniffBinaryMetadata(new Uint8Array(b).buffer)) as never,
      // A `FontLookup` is a function; the cases pass the list it answers.
      faceForFont: ((font: string, fonts?: never[]) =>
        faceForFont(font, fonts && (() => fonts))) as never,
      fontForFace: ((face: never, fonts?: never[]) =>
        fontForFace(face, fonts && (() => fonts))) as never,
      cssFontFamilyForFace: ((family: string, fonts?: never[]) =>
        cssFontFamilyForFace(family, fonts && (() => fonts))) as never,
      cssFontFamilyForFont: cssFontFamilyForFont as never,
      familyKey: familyKey as never,
      bodyFromTextStyle: ((t: never, fonts?: never[]) =>
        bodyFromTextStyle(t, fonts && (() => fonts))) as never,
      richDocOf: richDocOf as never,
      textCommitPatch: textCommitPatch as never,
      stripBodyDefaults: stripBodyDefaults as never,
      runDeltaForFields: ((p: never, fonts?: never[]) =>
        runDeltaForProps(p, fonts && (() => fonts))) as never,
      runsInRange: runsInRange as never,
      rangeProps: ((d: never, r: never, fonts?: never[]) =>
        rangeProps(d, r, fonts && (() => fonts))) as never,
      parseAnnotationDraft: ((draft: unknown) => AnnotationDraftSchema.parse(draft)) as never,
      previewBucket: previewBucket as never,
    },
    cases,
  },
];
