/** Current record conversion and field/shape writes, replacing the removed plugin repository. */
import {
  annotationKey,
  colorOf,
  rgbOf,
  annotationPatchBetween,
  type Annotation,
  type RichTextBody,
  type RichTextParagraph,
} from '@embedpdf/engine-core/runtime';
import {
  fromDTO,
  shapeOf,
  styleOf,
  withShape,
  withValues,
  kindOf,
  linkChildRects,
  writableTarget,
} from '../../../../packages/core/annotation/src/record';
import { kindFactsOf } from '../../../../packages/engine/core/src/annotation/kind-facts';
import { rotatedAabb } from '../../../../packages/core/annotation/src/rect';
import { draftOf } from '../../../../packages/core/annotation/src/update/changes';
import { familyOf } from '../../../../packages/core/annotation/src/shapes';
import { widgetAppearanceOf } from '../../../../packages/core/annotation/src/record/defaults';
import { widgetKindOf } from '../../../../packages/core/annotation/src/kinds/widget';
import { pagePointsBounds } from '@embedpdf/engine-core/runtime';
import type {
  ModelAnnotation,
  Shape,
  FieldValues,
} from '../../../../packages/core/annotation/src/types';
import {
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
  square,
  stamp,
  textNote,
  unsupported,
  widget,
  objRef,
  baseRef,
  pr,
  quad,
} from './annotation-current-fixtures';
import type { FunctionCase, Suite } from '../vectors';

const METRES = METRES_FORMAT();
const RICH_PARAS: RichTextParagraph[] = [
  { runs: [{ text: 'Hel' }, { text: 'lo', style: { weight: 700 } }] },
];
export const migration: { before: string; after: string | null; reason?: string }[] = [];
const cases: FunctionCase[] = [];
function replace(before: string, fn: string, ...args: unknown[]) {
  const name = fn === before.split('/')[0] ? before : `${fn}/${before}`;
  migration.push({ before, after: name });
  const invalidDrafts: Readonly<Record<string, string>> = {
    'toCreateDraft/callout-one-point':
      'Intentionally invalid current draft: a callout line needs two or three points.',
    'toCreateDraft/fresh/strikeout-rect-geom':
      'Intentionally invalid current draft: a strikeout needs quads, not a box.',
    'toCreateDraft/fresh/markup-no-geometry':
      'Intentionally invalid current draft: text markup needs at least one quad.',
  };
  const covers =
    fn === 'draftOf'
      ? invalidDrafts[before]
      : before.includes('text-no-icon')
        ? 'Explicit malformed imported read: the required current icon field is absent.'
        : undefined;
  cases.push({ name, fn, args, ...(covers ? { covers } : {}) });
}
const retire = (before: string, reason: string) => migration.push({ before, after: null, reason });
const rotatedBox = { x: 110, y: 110, width: 80, height: 80 };
const rotated = { rotation: 30, box: rotatedBox, rect: rotatedAabb(rotatedBox, 30) };

/** One current page-space DTO per kind and historical variant. */
export const DTOS: [string, Annotation][] = [
  ['square', square(1)],
  ['square-rotated', square(2, rotated)],
  ['square-rotation-only', square(3, { rotation: 90 })],
  ['square-cloudy', square(4, { cloudyIntensity: 2 })],
  [
    'square-dashed',
    square(5, { borderStyle: 'dashed', dashArray: [4, 2], interiorColor: '#0080ff' }),
  ],
  ['square-dashed-empty', square(6, { borderStyle: 'dashed', dashArray: [] })],
  [
    'square-offset-crop',
    square(7, {
      page: pr(2),
      box: { x: 50, y: 320, width: 100, height: 100 },
      rect: { x: 50, y: 320, width: 100, height: 100 },
    }),
  ],
  ['circle', circle(8, { rotation: 45, box: { x: 110, y: 110, width: 80, height: 80 } })],
  ['line', line(9)],
  ['line-rotated', line(10, { rotation: 15 })],
  ['line-no-endings', line(11, { lineEndings: { start: 'none', end: 'none' } })],
  ['polygon', polygon(12)],
  ['polygon-cloudy', polygon(13, { cloudyIntensity: 1.5, rotation: 20 })],
  ['polyline', polyline(14)],
  ['ink', ink(15)],
  ['ink-highlight', ink(16, { intent: 'ink-highlight', rotation: 10 })],
  ['free-text', freeText(17)],
  ['free-text-rotated', freeText(18, { ...rotated, fontColor: BLUE_DTO() })],
  ['free-text-null-contents', freeText(19, { contents: null, interiorColor: '#ffff00' })],
  ['callout', callout(20)],
  [
    'callout-two-points',
    callout(21, {
      calloutLine: [
        { x: 60, y: 290 },
        { x: 150, y: 200 },
      ],
      lineEnding: 'none',
    }),
  ],
  [
    'callout-rotated',
    callout(22, { rotation: 90, box: { x: 150, y: 100, width: 150, height: 150 } }),
  ],
  ['callout-rotation-no-box', callout(23, { rotation: 90 })],
  ['callout-one-point', Object.assign(callout(24), { calloutLine: [{ x: 60, y: 290 }] })],
  ['highlight', markup('highlight', 25)],
  ['underline', markup('underline', 26)],
  ['squiggly', markup('squiggly', 27)],
  ['strikeout', markup('strikeout', 28)],
  [
    'strikeout-text-edit',
    markup('strikeout', 29, {
      intent: 'strikeout-text-edit',
      reply: { to: objRef(30), type: 'group' },
    }),
  ],
  ['caret', caret(30, { intent: 'replace', contents: 'new words' })],
  ['caret-rotated', caret(31, { rotation: 90, box: { x: 86, y: 46, width: 8, height: 8 } })],
  ['redact-area', redact(32)],
  ['redact-no-fill', redact(51, { interiorColor: null })],
  [
    'redact-text',
    redact(33, {
      quadPoints: markup('highlight', 0).quadPoints,
      overlayText: 'REDACTED',
      repeat: true,
    }),
  ],
  ['text', textNote(34)],
  ['text-reply', textNote(35, { reply: { to: objRef(34), type: 'reply' }, contents: 'a reply' })],
  ['file-attachment', fileAttachment(36)],
  ['stamp', stamp(37, rotated)],
  ['link', link(38)],
  ['link-attached', link(39, { reply: { to: objRef(1), type: 'group' } })],
  ['link-no-target', link(40, { target: null })],
  [
    'link-javascript',
    link(41, { target: { kind: 'javascript' }, rect: { x: 220, y: 180, width: 80, height: 40 } }),
  ],
  [
    'widget-text',
    widget(42, 'text', {
      fontFamily: 'courier',
      fontSize: 10,
      fontColor: '#0000ff',
      color: RED_DTO(),
    }),
  ],
  ['widget-combobox', widget(43, 'combobox')],
  ['widget-listbox', widget(44, 'listbox', { interiorColor: '#f0f0f0', borderStyle: 'dashed' })],
  ['widget-pushbutton', widget(45, 'pushbutton')],
  ['widget-checkbox', widget(46, 'checkbox')],
  ['widget-radio', widget(47, 'radio')],
  ['widget-signature', widget(48, 'signature')],
  ['unsupported', unsupported(49)],
  ['base-index-ref', square(50, { ref: baseRef(3), nm: 'abc' })],
  // The engine now supplies each kind's own box, including upright boxes.
  // These retain distinct boxes rather than a locally calculated /RD inset.
  ['highlight-multiply', markup('highlight', 53, { blendMode: 'multiply' })],
  [
    'callout-unrotated-box-no-rotation',
    callout(54, { box: { x: 160, y: 110, width: 130, height: 130 } }),
  ],
  [
    'callout-rotated-distinct-box',
    callout(55, { rotation: 90, box: { x: 160, y: 110, width: 130, height: 130 } }),
  ],
  [
    'free-text-unrotated-box-no-rotation',
    freeText(56, { box: { x: 110, y: 110, width: 80, height: 80 } }),
  ],
  // Explicitly hostile nonfinite rotations remain robustness inputs.
  [
    'square-rotation-nan',
    square(90, { rotation: Number.NaN, box: { x: 110, y: 110, width: 80, height: 80 } }),
  ],
  [
    'square-rotation-infinite',
    square(91, {
      rotation: Number.POSITIVE_INFINITY,
      box: { x: 110, y: 110, width: 80, height: 80 },
    }),
  ],
  ['caret-unrotated-box-no-rotation', caret(58, { box: { x: 86, y: 46, width: 8, height: 8 } })],
  // Only a strikeout reads its intent.
  [
    'highlight-with-intent',
    Object.assign(markup('highlight', 57), { intent: 'strikeout-text-edit' }),
  ],
  // Measurement dimensions: the line carries its leader and caption, the
  // shapes their caption, and every one derives /Contents from /Measure.
  [
    'line-dimension',
    line(59, {
      intent: 'line-dimension',
      measure: METRES,
      captionEnabled: true,
      captionPosition: 'inline',
      leader: { length: 12, extension: 5, offset: 0 },
      contents: '2 m',
      linePoints: { start: { x: 100, y: 100 }, end: { x: 244, y: 100 } },
    }),
  ],
  ['line-dimension-bare', line(60, { intent: 'line-dimension' })],
  [
    'line-dimension-foreign',
    line(61, {
      intent: 'line-dimension',
      measure: { subtype: 'geospatial' },
      captionEnabled: false,
      contents: null,
    }),
  ],
  [
    'polyline-dimension',
    polyline(62, {
      intent: 'polyline-dimension',
      measure: METRES,
      captionEnabled: true,
      contents: 'stale',
    }),
  ],
  [
    'polygon-dimension',
    polygon(63, {
      intent: 'polygon-dimension',
      measure: METRES,
      captionEnabled: true,
      captionCenter: { x: 150, y: 130 },
    }),
  ],
  ['polygon-dimension-bare', polygon(64, { intent: 'polygon-dimension', rotation: 30 })],
  // A rich body carries the formatting the /DA cannot.
  [
    'free-text-rich',
    freeText(65, {
      richText: { body: RICH_BODY(700, true, ['underline']), paragraphs: RICH_PARAS },
    }),
  ],
  [
    'free-text-rich-plain',
    freeText(66, { richText: { body: RICH_BODY(400, false, []), paragraphs: RICH_PARAS } }),
  ],
];

/** 72 pt to the metre. */
function METRES_FORMAT() {
  return {
    subtype: 'rectilinear' as const,
    ratio: '1 in = 1 m',
    x: [{ unit: 'm', conversion: 1 / 72 }],
    distance: [{ unit: 'm', conversion: 1 }],
    area: [{ unit: 'm²', conversion: 1 }],
  };
}
function RICH_BODY(
  weight: number,
  italic: boolean,
  decoration: RichTextBody['decoration'],
): RichTextBody {
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
  return '#3858e9';
}
function RED_DTO() {
  return '#ef4444';
}

/** An actual earlier drawing, constructed with shipping edits before recording both DTOs. */
const earlierDrawing = (annotation: Annotation): Annotation => {
  const shape = moved(annotation);
  const upright = 'rotation' in shape ? { ...shape, rotation: 0 } : shape;
  const fontFamily =
    'fontFamily' in annotation && annotation.fontFamily === 'courier' ? 'helvetica' : 'courier';
  const fontSize = 'fontSize' in annotation && annotation.fontSize === 9 ? 10 : 9;
  const fontColor =
    'fontColor' in annotation && annotation.fontColor === '#010203' ? '#123456' : '#010203';
  const textAlign =
    'textAlign' in annotation && annotation.textAlign === 'right' ? 'left' : 'right';
  const page = annotation.page.objectNumber;
  const parent =
    annotation.ref.kind === 'objectNumber' && annotation.ref.objectNumber === 2 ? 1 : 2;
  return withValues(withShape(fromDTO(annotation), upright), {
    color: '#010203',
    opacity: 0.8,
    strokeWidth: 1,
    borderStyle: 'solid',
    dashArray: null,
    cloudyIntensity: null,
    interiorColor: null,
    contents: 'earlier text',
    reply: { to: objRef(parent, page), type: 'reply' },
    groupId: 'earlier-group',
    lineEndings: { start: 'closed-arrow', end: 'square' },
    captionEnabled:
      'captionEnabled' in annotation && annotation.captionEnabled === true ? false : true,
    captionPosition:
      'captionPosition' in annotation && annotation.captionPosition === 'top' ? 'inline' : 'top',
    captionCenter: { x: 8, y: 9 },
    captionOffset: { along: 1, perpendicular: 2 },
    leader: { length: 3, extension: 1, offset: 2 },
    measure: {
      ...METRES,
      ratio: '1 in = 2 cm',
      x: [{ unit: 'cm', conversion: 2 / 72 }],
      distance: [{ unit: 'cm', conversion: 1 }],
      area: [{ unit: 'cm²', conversion: 1 }],
    },
    fontFamily,
    fontSize,
    fontColor,
    textAlign,
    richText: {
      body: {
        ...RICH_BODY(400, false, []),
        family: fontFamily === 'courier' ? 'Courier' : 'Helvetica',
        size: fontSize,
        color: fontColor,
        align: textAlign,
      },
      paragraphs: [{ runs: [{ text: 'earlier text' }] }],
    },
    ...(annotation.subtype === 'text'
      ? { icon: annotation.icon === 'key' ? 'comment' : 'key' }
      : {}),
    ...(annotation.subtype === 'file-attachment'
      ? { icon: annotation.icon === 'tag' ? 'paperclip' : 'tag' }
      : {}),
    // A read-only target can be carried unchanged, never authored by this diff.
    ...(annotation.subtype === 'link'
      ? {
          target:
            annotation.target && !writableTarget(annotation.target)
              ? annotation.target
              : { kind: 'uri', uri: 'https://earlier.example' },
        }
      : {}),
  }).annotation;
};
const dtoOf = (label: string): Annotation => {
  const found = DTOS.find(([name]) => name === label);
  if (!found) throw new Error(`missing current DTO ${label}`);
  return found[1];
};
const record = (label: string): ModelAnnotation => fromDTO(dtoOf(label));
const oldLabel = (label: string) => (label === 'base-index-ref' ? 'nm-ref' : label);
/** Data fields for the drawing's draft, prepared before recording the arguments. */
function dataOf(annotation: Annotation): FieldValues {
  const owners = kindFactsOf(annotation.subtype)!.owners;
  const fields = Object.fromEntries(
    Object.entries(annotation).filter(([name]) => owners[name] === 'data'),
  );
  return annotation.subtype === 'link'
    ? { ...fields, target: writableTarget(annotation.target) }
    : fields;
}
const moved = (annotation: Annotation): Shape => {
  const shape = shapeOf(annotation);
  return familyOf(shape).translate(shape, { x: 7, y: -3 });
};
for (const [label, dto] of DTOS) {
  const old = oldLabel(label);
  replace(`fromDTO/${old}`, 'fromDTO', dto);
  replace(`toPatch/${old}`, 'annotationPatchBetween', earlierDrawing(dto), dto);
  replace(`toScopedPatch/${old}/geometry`, 'withShape', fromDTO(dto), moved(dto));
  if (dto.subtype === 'unsupported')
    retire(
      `toCreateDraft/${old}`,
      'Unsupported annotations cannot be created (write/crud.ts:72-79); no replacement create exists.',
    );
  else replace(`toCreateDraft/${old}`, 'draftOf', kindOf(dto).name, dataOf(dto), shapeOf(dto));
}
retire(
  'fromDTO/vector-source',
  'd29344e88: fromDTO takes one engine record and always uses baked; no caller-selected source. fromDTO/square covers this record.',
);
retire(
  'fromDTO/default-source',
  'd29344e88 core record/index.ts:98-116 already has one argument and baked source; same record covered by fromDTO/square.',
);
replace('fromDTO/no-ref', 'fromDTO', (({ ref: _ref, ...rest }) => rest)(square(1)));
replace('fromDTO/null-ref', 'fromDTO', { ...square(1), ref: null });

const cloud = { cloudyIntensity: 2 };
const dash = { borderStyle: 'dashed', dashArray: [5, 3] } as const;
const STYLED: [string, Annotation][] = [
  ['square', square(1)],
  ['square-cloudy', square(1, cloud)],
  [
    'square-dashed',
    square(1, { ...dash, dashArray: [...dash.dashArray], interiorColor: '#00ff00' }),
  ],
  ['circle-cloudy', circle(8, { ...cloud, strokeWidth: 4 })],
  ['line', dtoOf('line')],
  ['line-no-endings', dtoOf('line-no-endings')],
  ['polygon-cloudy', polygon(12, cloud)],
  ['polygon-dashed', polygon(12, { ...dash, dashArray: [...dash.dashArray] })],
  ['polyline', dtoOf('polyline')],
  ['ink', dtoOf('ink')],
  ['free-text', dtoOf('free-text')],
  [
    'free-text-no-text',
    freeText(17, { richText: { body: RICH_BODY(400, false, []), paragraphs: [] } }),
  ],
  ['callout', dtoOf('callout')],
  ['highlight', dtoOf('highlight')],
  ['redact-text', dtoOf('redact-text')],
  ['text', dtoOf('text')],
  // Explicit malformed imported read: the current read schema requires an icon.
  ['text-no-icon', textNote(34, { icon: undefined })],
  ['link', dtoOf('link')],
  ['link-null', dtoOf('link-no-target')],
  ['link-readonly', dtoOf('link-javascript')],
  ['widget-text', dtoOf('widget-text')],
  ['stamp', dtoOf('stamp')],
  ['square-empty-interior', square(1, { interiorColor: '' })],
  ['free-text-rich', dtoOf('free-text-rich')],
  [
    'free-text-bold-draft',
    freeText(17, {
      richText: { body: RICH_BODY(700, false, ['underline']), paragraphs: RICH_PARAS },
    }),
  ],
  ['line-dimension', dtoOf('line-dimension')],
  ['polygon-dimension', dtoOf('polygon-dimension')],
];
/** Current engine fields are recorded in full, including atomic border and rich body changes. */
const PROP_SCOPES: [string, FieldValues][] = [
  ['color', { color: '#123456' }],
  ['stroke-width', { strokeWidth: 5 }],
  ['border', { borderStyle: 'dashed', dashArray: [6, 2], cloudyIntensity: null }],
  ['line-endings', { lineEndings: { start: 'circle', end: 'closed-arrow' } }],
  ['interior', { interiorColor: '#00ff00', opacity: 0.4, blendMode: 'multiply' }],
  ['font', { fontFamily: 'courier', fontSize: 18, fontColor: '#0000ff', textAlign: 'center' }],
  ['format', { richText: { body: { weight: 700, italic: true, decoration: ['underline'] } } }],
  ['icon', { icon: 'key' }],
  ['link', { target: { kind: 'uri', uri: 'https://docs.example' } }],
  ['unknown', { bogus: 1 }],
  ['empty', {}],
];
for (const label of [
  'line-dimension',
  'line-dimension-bare',
  'polyline-dimension',
  'polygon-dimension',
  'square',
]) {
  replace(`toScopedPatch/${label}/caption`, 'withValues', record(label), {
    captionEnabled: true,
    captionPosition: 'top',
    captionCenter: { x: 170, y: 140 },
  });
  replace(`toScopedPatch/${label}/leader`, 'withValues', record(label), {
    leader: { length: 18, extension: 7, offset: 2 },
  });
}
for (const [label, annotation] of STYLED) {
  for (const [scope, values] of PROP_SCOPES) {
    const stated =
      scope === 'format' && annotation.subtype === 'free-text'
        ? {
            richText: {
              ...annotation.richText,
              body: {
                ...annotation.richText.body,
                weight: 700,
                italic: true,
                decoration: ['underline'],
              },
            },
          }
        : values;
    replace(`toScopedPatch/${label}/${scope}`, 'withValues', fromDTO(annotation), stated);
  }
  replace(
    `toPatch/styled/${label}`,
    'annotationPatchBetween',
    earlierDrawing(annotation),
    annotation,
  );
}
for (const label of ['square-cloudy', 'polygon-cloudy']) {
  const annotation = STYLED.find(([name]) => name === label)![1];
  replace(
    `toCreateDraft/styled/${label}`,
    'draftOf',
    kindOf(annotation).name,
    dataOf(annotation),
    shapeOf(annotation),
  );
}

// Fresh shapes use current discriminants and full engine fields. Wrong-family cases stay explicit.
const BOX = { x: 10, y: 10, width: 100, height: 30 };
const SHAPES = {
  ink: {
    kind: 'ink',
    inkList: [
      [
        { x: 1, y: 2 },
        { x: 3, y: 4 },
      ],
    ],
    rotation: 0,
  },
  caret: { kind: 'caret', box: { x: 10, y: 10, width: 6, height: 8 }, rotation: 0 },
  quads: { kind: 'quads', quadPoints: [quad(10, 10, 50, 12)] },
  box: { kind: 'box', box: BOX, rotation: 0, ellipse: false },
  text: { kind: 'text-box', box: BOX, rotation: 0, calloutLine: null, lineEnding: 'none' },
  line: { kind: 'line', linePoints: { start: { x: 1, y: 1 }, end: { x: 3, y: 4 } }, rotation: 0 },
} satisfies Record<string, Shape>;
const freshDrafts: [string, string, Shape, FieldValues?][] = [
  ['ink-highlight', 'ink', SHAPES.ink, { intent: 'ink-highlight' }],
  ['caret', 'caret', SHAPES.caret],
  ['caret-replace', 'caret', { ...SHAPES.caret, rotation: 90 }, { intent: 'replace' }],
  ['strikeout-edit', 'strikeout', SHAPES.quads, { intent: 'strikeout-text-edit' }],
  ['strikeout-rect-geom', 'strikeout', SHAPES.box],
  [
    'redact-quads',
    'redact',
    { kind: 'quads', quadPoints: [quad(10, 10, 50, 12), quad(10, 30, 20, 12)] },
  ],
  ['free-text', 'free-text', SHAPES.text],
  ['link', 'link', SHAPES.box, { target: null }],
  ['widget', 'widget-text', SHAPES.box],
  ['markup-no-geometry', 'highlight', { kind: 'quads', quadPoints: [] }],
];
for (const [label, kind, shape, fields = {}] of freshDrafts)
  replace(`toCreateDraft/fresh/${label}`, 'draftOf', kind, {}, shape, fields);
replace(
  'toPatch/fresh/unsupported',
  'annotationPatchBetween',
  earlierDrawing(unsupported(500)),
  unsupported(500),
);
for (const [label, dto, shape] of [
  ['unsupported-geometry', unsupported(500), SHAPES.box],
  ['highlight-geometry', markup('highlight', 501), SHAPES.quads],
  ['square-no-rect-geom', line(502), SHAPES.box],
  ['poly-on-ink-geom', polygon(503), SHAPES.line],
  ['ink-on-line-geom', ink(504), SHAPES.line],
  ['caret-on-rect-geom', caret(505), SHAPES.box],
  ['free-text-on-rect-geom', freeText(506), SHAPES.box],
  ['redact-quads-geometry', redact(507), SHAPES.quads],
  ['text-on-line-geom', textNote(508), SHAPES.line],
] as const)
  replace(`toScopedPatch/fresh/${label}`, 'withShape', fromDTO(dto), shape);
replace('toScopedPatch/fresh/line-endings-on-rect', 'withValues', fromDTO(line(509)), {
  lineEndings: { start: 'circle', end: 'none' },
});
replace(
  'toScopedPatch/fresh/square-props-on-line',
  'withValues',
  fromDTO(square(510, cloud)),
  cloud,
);

replace('annotationKey/object-number', 'annotationKey', objRef(12));
replace('annotationKey/nm', 'annotationKey', baseRef(7));
replace('annotationKey/index', 'annotationKey', baseRef(7, 3));
replace('colorToCss/plain', 'colorOf', 255, 16, 0);
replace('colorToCss/clamped', 'colorOf', 300, -5, 12.6);
for (const [label, css] of [
  ['six', '#aabbcc'],
  ['six-bare-upper', 'AABBCC'],
  ['three', '#abc'],
  ['three-padded', '  #ABC\n'],
  ['named', 'red'],
  ['four', '#abcd'],
])
  replace(`cssToColor/${label}`, 'rgbOf', css);
for (const label of ['flat', 'rotated'])
  replace(
    `boxGeomFields/${label}`,
    'draftOf',
    'square',
    {},
    { ...SHAPES.box, rotation: label === 'flat' ? 0 : 30 },
  );
for (const [label, rotation] of [
  ['flat', 0],
  ['rotated', 90],
  ['rotation-without-box', 45],
  ['full-turn', 360],
] as const)
  replace(
    `boxGeomFromDTO/${label}`,
    'shapeOf',
    square(600, { rotation, box: { x: 0, y: 0, width: 600, height: 800 } }),
  );
// Rectangle differences no longer reach public records. Their historical raw PDF insetting has no plugin replacement.
for (const label of ['inset', 'none'])
  retire(
    `insetPdfRectByRD/${label}`,
    '2bdc74fbf removes /RD repository inset; current record/shape.ts reads the engine-owned box directly. Explicit text/callout box variants cover surviving placement.',
  );
for (const [label, rotation] of [
  ['set', 30],
  ['zero', 0],
  ['absent', null],
] as const)
  replace(`rotFromDTO/${label}`, 'shapeOf', square(601, { rotation }));
for (const [label, fields] of [
  ['cloudy', { cloudyIntensity: 1, borderStyle: 'dashed' }],
  ['dashed', { borderStyle: 'dashed', dashArray: [2, 1] as number[] }],
  ['dashed-default', { borderStyle: 'dashed' }],
  ['solid', { borderStyle: 'solid', cloudyIntensity: null }],
] as const)
  replace(
    `borderFromDTO/${label}`,
    'styleOf',
    square(602, {
      ...fields,
      ...('dashArray' in fields ? { dashArray: [...fields.dashArray] } : {}),
    }),
  );
for (const [label, target] of [
  ['goto', { kind: 'goto', destination: { kind: 'fit', page: pr(1) } }],
  ['uri', { kind: 'uri', uri: 'https://x' }],
  ['named', { kind: 'named', name: 'NextPage' }],
  ['null', null],
  ['undefined', undefined],
])
  replace(`writableTarget/${label}`, 'writableTarget', target);
replace('widgetKindOf/text', 'widgetKindOf', 'text');
replace('widgetKindOf/unknown', 'widgetKindOf', 'signature');
for (const [label, defaults] of [
  [
    'everything',
    {
      color: '#ff0000',
      interiorColor: '#00ff00',
      strokeWidth: 2,
      borderStyle: 'dashed',
      dashArray: [2, 2],
      fontFamily: 'courier',
      fontSize: 11,
      fontColor: '#0000ff',
      textAlign: 'center',
    },
  ],
  ['clears', { interiorColor: null, borderStyle: 'solid' }],
  ['empty-interior', { interiorColor: '' }],
  ['none', {}],
])
  replace(`widgetAppearanceFromProps/${label}`, 'widgetAppearanceOf', defaults);
for (const [label, dto] of [
  ['cloudy', square(1, cloud)],
  ['plain', square(1)],
  ['not-a-rect', line(9)],
] as const)
  replace(`cloudyExtras/${label}`, 'draftOf', kindOf(dto).name, dataOf(dto), shapeOf(dto));
for (const [label, dto] of [
  ['callout', callout(20)],
  ['rotated', dtoOf('callout-rotated')],
  ['plain', freeText(17)],
] as const)
  replace(`calloutFields/${label}`, 'draftOf', kindOf(dto).name, dataOf(dto), shapeOf(dto));
for (const [label, dto] of [
  ['quads', markup('highlight', 25)],
  ['not-quads', square(1)],
] as const)
  replace(`quadPointsFor/${label}`, 'shapeOf', dto);
replace(
  'pdfBoundsOfQuads/two',
  'pagePointsBounds',
  [quad(1, 2, 4, 7), quad(-3, -10, 3, 8)].flatMap((q) => [
    q.upperLeft,
    q.upperRight,
    q.lowerLeft,
    q.lowerRight,
  ]),
);
replace('pdfBoundsOfQuads/none', 'pagePointsBounds', []);
for (const label of ['square-rotated', 'callout', 'highlight', 'line', 'ink', 'redact-text']) {
  const dto = dtoOf(label);
  replace(`linkChildRects/${label}`, 'linkChildRects', shapeOf(dto), styleOf(dto));
}
for (const label of [
  'square',
  'polygon-cloudy',
  'ink',
  'free-text',
  'highlight',
  'caret',
  'redact-area',
  'redact-no-fill',
  'text',
  'widget-text',
  'widget-checkbox',
  'unsupported',
])
  replace(`styleFromDTO/${label}`, 'styleOf', dtoOf(label));
export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'plugins/annotation-repository.json',
    describes:
      'Current annotation record, shape, style and draft boundary; historical removed repository targets mapped to shipping functions.',
    functions: {
      fromDTO: fromDTO as never,
      annotationPatchBetween: annotationPatchBetween as never,
      withShape: withShape as never,
      withValues: withValues as never,
      draftOf: draftOf as never,
      annotationKey: annotationKey as never,
      colorOf: colorOf as never,
      rgbOf: rgbOf as never,
      shapeOf: shapeOf as never,
      styleOf: styleOf as never,
      writableTarget: writableTarget as never,
      widgetKindOf: widgetKindOf as never,
      widgetAppearanceOf: widgetAppearanceOf as never,
      pagePointsBounds: pagePointsBounds as never,
      linkChildRects: linkChildRects as never,
    },
    cases,
  },
];
