/** Complete engine records in page space, shared by the current annotation oracles. */
import {
  annotationOfDraft,
  type Annotation,
  type AnnotationDraft,
  type AnnotationRef,
  type PageRef,
  type PdfMeasure,
  type PageQuad,
} from '@embedpdf/engine-core/runtime';
import { annotationOfResolvedDraft } from '../../../../packages/engine/core/src/annotation/resolve/annotationOfDraft';
import type { Suite } from '../vectors';
export const suites: Suite[] = [];
export const pr = (objectNumber: number): PageRef => ({ kind: 'objectNumber', objectNumber });
export const objRef = (objectNumber: number, page = 1): AnnotationRef => ({
  kind: 'objectNumber',
  page: pr(page),
  objectNumber,
});
export const baseRef = (baseIndex: number, page = 1): AnnotationRef => ({
  kind: 'baseIndex',
  page: pr(page),
  baseIndex,
});
export const RED = '#ef4444';
export const BLUE = '#3858e9';
export const BOX = { x: 100, y: 100, width: 100, height: 100 };
export const METRES: PdfMeasure = {
  subtype: 'rectilinear',
  ratio: '1 in = 1 m',
  x: [{ unit: 'm', conversion: 1 / 72 }],
  distance: [{ unit: 'm', conversion: 1 }],
  area: [{ unit: 'm²', conversion: 1 }],
};
type Of<K extends Annotation['subtype']> = Extract<Annotation, { subtype: K }>;
/** Engine read-back supplies every declared field; imported fields lie over it without a write. */
function read<K extends AnnotationDraft['subtype']>(
  n: number,
  draft: Extract<AnnotationDraft, { subtype: K }>,
  over: Partial<Of<K>> = {},
): Of<K> {
  const address: Partial<Pick<Annotation, 'ref' | 'page'>> = over;
  const ref = address.ref ?? objRef(n, address.page?.objectNumber ?? 1);
  // The generic subtype is fixed by the typed draft. This assertion only narrows the engine's union.
  return { ...annotationOfResolvedDraft(draft, { ref }), ...over } as Of<K>;
}
export const square = (n: number, over: Partial<Of<'square'>> = {}) =>
  read<'square'>(
    n,
    {
      subtype: 'square',
      box: BOX,
      color: RED,
      interiorColor: null,
      opacity: 1,
      strokeWidth: 2,
      borderStyle: 'solid',
      print: true,
    },
    over,
  );
export const circle = (n: number, over: Partial<Of<'circle'>> = {}) =>
  read<'circle'>(
    n,
    {
      subtype: 'circle',
      box: BOX,
      color: RED,
      opacity: 1,
      strokeWidth: 2,
      borderStyle: 'solid',
      print: true,
    },
    over,
  );
export const line = (n: number, over: Partial<Of<'line'>> = {}) =>
  read<'line'>(
    n,
    {
      subtype: 'line',
      linePoints: { start: { x: 100, y: 100 }, end: { x: 200, y: 150 } },
      color: RED,
      strokeWidth: 2,
      lineEndings: { start: 'none', end: 'open-arrow' },
      print: true,
    },
    { rect: BOX, ...over },
  );
export const polygon = (n: number, over: Partial<Of<'polygon'>> = {}) =>
  read<'polygon'>(
    n,
    {
      subtype: 'polygon',
      vertices: [
        { x: 100, y: 100 },
        { x: 200, y: 100 },
        { x: 150, y: 180 },
      ],
      color: RED,
      strokeWidth: 2,
      print: true,
    },
    { rect: BOX, ...over },
  );
export const polyline = (n: number, over: Partial<Of<'polyline'>> = {}) =>
  read<'polyline'>(
    n,
    {
      subtype: 'polyline',
      vertices: [
        { x: 100, y: 100 },
        { x: 200, y: 120 },
        { x: 150, y: 180 },
      ],
      color: RED,
      strokeWidth: 2,
      lineEndings: { start: 'circle', end: 'none' },
      print: true,
    },
    { rect: BOX, ...over },
  );
export const ink = (n: number, over: Partial<Of<'ink'>> = {}) =>
  read<'ink'>(
    n,
    {
      subtype: 'ink',
      inkList: [
        [
          { x: 100, y: 100 },
          { x: 120, y: 110 },
          { x: 140, y: 100 },
        ],
      ],
      color: RED,
      strokeWidth: 3,
      print: true,
    },
    { rect: BOX, ...over },
  );
type FreeTextOverrides = Partial<Of<'free-text'>> &
  Pick<Extract<AnnotationDraft, { subtype: 'free-text' }>, 'intent' | 'borderStyle'>;
export const freeText = (n: number, over: FreeTextOverrides = {}): Of<'free-text'> => {
  const fields = {
    subtype: 'free-text',
    box: BOX,
    intent: 'free-text',
    fontFamily: 'helvetica',
    fontSize: 12,
    fontColor: RED,
    textAlign: 'left',
    color: RED,
    strokeWidth: 1,
    print: true,
    ...over,
    contents: over.richText ? over.contents : over.contents === undefined ? 'Hello' : over.contents,
  } satisfies Extract<AnnotationDraft, { subtype: 'free-text' }>;
  const ref = over.ref ?? objRef(n, over.page?.objectNumber ?? 1);
  return { ...annotationOfDraft(fields, { ref }), ...over } as Of<'free-text'>;
};
export const callout = (n: number, over: FreeTextOverrides = {}) =>
  freeText(n, {
    intent: 'free-text-callout',
    rect: { x: 50, y: 100, width: 250, height: 200 },
    box: { x: 150, y: 100, width: 150, height: 150 },
    calloutLine: [
      { x: 60, y: 290 },
      { x: 120, y: 240 },
      { x: 150, y: 200 },
    ],
    lineEnding: 'open-arrow',
    ...over,
  });
export const quad = (x: number, y: number, width: number, height: number): PageQuad => ({
  upperLeft: { x, y },
  upperRight: { x: x + width, y },
  lowerLeft: { x, y: y + height },
  lowerRight: { x: x + width, y: y + height },
});
type MarkupKind = 'highlight' | 'underline' | 'strikeout' | 'squiggly';
export const markup = <K extends MarkupKind>(
  subtype: K,
  n: number,
  over: Partial<Of<K>> = {},
): Of<K> => {
  const draft = {
    subtype,
    quadPoints: [quad(50, 55, 150, 15)],
    color: RED,
    opacity: 1,
    print: true,
  };
  // A generic union of the four markup declarations has identical common draft fields.
  return read(n, draft as Extract<AnnotationDraft, { subtype: K }>, {
    rect: { x: 50, y: 55, width: 150, height: 15 },
    ...over,
  });
};
export const caret = (n: number, over: Partial<Of<'caret'>> = {}) =>
  read<'caret'>(
    n,
    {
      subtype: 'caret',
      box: { x: 85, y: 45, width: 10, height: 10 },
      color: RED,
      opacity: 1,
      print: true,
    },
    over,
  );
export const redact = (n: number, over: Partial<Of<'redact'>> = {}) =>
  read<'redact'>(
    n,
    {
      subtype: 'redact',
      rect: BOX,
      quadPoints: [],
      color: RED,
      interiorColor: '#000000',
      opacity: 1,
      fontFamily: 'helvetica',
      fontSize: 0,
      fontColor: '#ffffff',
      textAlign: 'center',
      print: true,
    },
    over,
  );
export const textNote = (n: number, over: Partial<Of<'text'>> = {}) =>
  read<'text'>(
    n,
    {
      subtype: 'text',
      rect: { x: 100, y: 100, width: 20, height: 20 },
      color: '#facc15',
      opacity: 1,
      icon: 'comment',
      print: true,
    },
    over,
  );
export const fileAttachment = (n: number, over: Partial<Of<'file-attachment'>> = {}) =>
  read<'file-attachment'>(
    n,
    {
      subtype: 'file-attachment',
      rect: { x: 100, y: 100, width: 20, height: 20 },
      color: '#facc15',
      opacity: 1,
      icon: 'paperclip',
      file: { name: 'a.txt', mimeType: 'text/plain' },
      print: true,
    },
    over,
  );
export const stamp = (n: number, over: Partial<Of<'stamp'>> = {}) =>
  read<'stamp'>(n, { subtype: 'stamp', box: BOX, print: true }, over);
export const link = (n: number, over: Partial<Of<'link'>> = {}) =>
  read<'link'>(
    n,
    {
      subtype: 'link',
      rect: BOX,
      target: { kind: 'uri', uri: 'https://example.com' },
      print: true,
    },
    over,
  );
export const widget = (
  n: number,
  fieldFamily: Of<'widget'>['fieldFamily'],
  over: Partial<Of<'widget'>> = {},
) =>
  read<'widget'>(
    n,
    {
      subtype: 'widget',
      box: BOX,
      strokeWidth: 1,
      borderStyle: 'solid',
      textAlign: 'left',
      print: true,
    },
    { fieldFamily, ...over },
  );
export const unsupported = (
  n: number,
  over: Partial<Of<'unsupported'>> = {},
): Of<'unsupported'> => {
  const {
    box: _box,
    rotation: _rotation,
    color: _color,
    interiorColor: _fill,
    opacity: _opacity,
    strokeWidth: _width,
    borderStyle: _border,
    dashArray: _dash,
    cloudyIntensity: _cloud,
    ...base
  } = square(n);
  return { ...base, subtype: 'unsupported', rawSubtypeCode: 16, rawSubtypeName: null, ...over };
};
