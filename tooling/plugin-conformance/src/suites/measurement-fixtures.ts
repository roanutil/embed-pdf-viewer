/** Current measurement inputs and static engine answers, constructed before execution. */
import type {
  Annotation,
  AnnotationDraft,
  DocumentEvent,
  MutationMeta,
  PageBox,
  PageMeasurementViewport,
  PageMeasurementViewportList,
  PageScaleResult,
  PdfMeasure,
  PdfMeasurement,
} from '@embedpdf/engine-core/runtime';
import { defaultMeasure } from '../../../../packages/plugin/measurement/src/scale';
import { annotationOfNew } from '../../../../packages/core/annotation/src/record';
import { measurementPlugin } from '../../../../packages/plugin/measurement/src/measurement.plugin';
import {
  MeasurementToken,
  type MeasurementCapability,
  type MeasurementConfig,
} from '../../../../packages/plugin/measurement/src/contract';
import type { AnnotationConfig } from '../../../../packages/plugin/annotation/src/contract';
import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import { registerPlugin, type Step } from '../scenario';
import {
  doc,
  hydrate,
  layout,
  objRef,
  pr,
  LOCAL,
  updated,
  write,
} from './annotation-live-fixtures';
import './interaction';
export {
  open,
  refused,
  boot,
  call as an,
  created,
  hydrate,
  layout,
  objRef,
  pr,
  settle,
  LOCAL,
  updated,
  write,
  confirmation,
} from './annotation-live-fixtures';
export { ix } from './annotation-live-inputs';
export const suites = [];
registerPlugin('measurement-current', {
  factory: (config) => measurementPlugin(config as MeasurementConfig | undefined),
  tokens: [MeasurementToken],
});
export const CROP = { left: 0, bottom: 0, right: 600, top: 800 };
export const CROP2 = { left: 10, bottom: 20, right: 410, top: 520 };
export const METRES: PdfMeasure = {
  subtype: 'rectilinear',
  ratio: '1 in = 1 m',
  x: [{ unit: 'm', conversion: 1 / 72, precision: 100 }],
  distance: [{ unit: 'm', conversion: 1, precision: 100 }],
  area: [{ unit: 'm²', conversion: 1, precision: 100 }],
};
export const FEET: PdfMeasure = {
  ...METRES,
  ratio: '1 in = 1 ft',
  x: [{ unit: 'ft', conversion: 1 / 72, precision: 100 }],
  distance: [{ unit: 'ft', conversion: 1, precision: 100 }],
  area: [{ unit: 'ft²', conversion: 1, precision: 100 }],
};
export const GEO = { subtype: 'geospatial' } satisfies PdfMeasurement;
export const FEET_INCHES: PdfMeasure = {
  ...FEET,
  distance: [
    { unit: 'ft', conversion: 1, precision: 1 },
    { unit: 'in', conversion: 12, fraction: 'fraction', precision: 16 },
  ],
  area: [{ unit: 'ft²', conversion: 1, fraction: 'fraction', precision: 8 }],
};
export const rl = (unit: string, over: Partial<PdfMeasure> = {}): PdfMeasure => ({
  ...METRES,
  ratio: `1 in = 1 ${unit}`,
  x: [{ unit, conversion: 1 / 72, precision: 100 }],
  distance: [{ unit, conversion: 1, precision: 100 }],
  area: [{ unit: `${unit}²`, conversion: 1, precision: 100 }],
  ...over,
});
/** Deliberate out-of-contract write negatives; the exact malformed object is recorded. */
export const malformedMeasure = (value: unknown): PdfMeasure => value as PdfMeasure;
export const dimensionLine = (
  n: number,
  over: Partial<
    Pick<Extract<Annotation, { subtype: 'line' }>, 'measure' | 'locked' | 'page' | 'ref'>
  > = {},
) => {
  const draft = {
    subtype: 'line',
    intent: 'line-dimension',
    measure: METRES,
    captionEnabled: true,
    captionPosition: 'inline',
    leader: { length: 12, extension: 5, offset: 0 },
    linePoints: { start: { x: 100, y: 100 }, end: { x: 244, y: 100 } },
    color: '#e5484d',
    strokeWidth: 2,
    print: true,
    ...over,
  } satisfies AnnotationDraft;
  const record = annotationOfNew(draft, { ref: objRef(n, over.page?.objectNumber ?? 1) });
  if (record.subtype !== 'line') throw new Error('line draft must materialize as line');
  return record;
};
export const PAGES = [layout(0, 1), layout(1, 2, CROP2)];
export const PAGES4 = [
  ...PAGES,
  layout(2, 3, { left: 0, bottom: 0, right: 200, top: 200 }),
  layout(3, 4, { left: 0, bottom: 0, right: 200, top: 200 }),
];
export const docOf = (
  responses: ResponseSpec[] = [],
  over: Partial<FakeDocumentSpec> = {},
): FakeDocumentSpec =>
  doc([...responses, hydrate(), ...PAGES.map((page) => viewports(page.ref.objectNumber, []))], {
    pages: PAGES,
    ...over,
  });
export const PLUGINS = [
  { factory: 'interaction' },
  { factory: 'annotation-live' },
  { factory: 'measurement-current' },
];
export const withConfig = (config: MeasurementConfig, annotation: AnnotationConfig = {}) => [
  { factory: 'interaction' },
  { factory: 'annotation-live', config: annotation },
  { factory: 'measurement-current', config },
];
export const SNAP = ['measurement::a', 'annotation::a'];
type EventMethod = Extract<keyof MeasurementCapability, `on${string}`>;
type Method = Exclude<keyof MeasurementCapability, EventMethod>;
type Args<K extends Method> = MeasurementCapability[K] extends (...args: infer A) => unknown
  ? A
  : never;
export function m<K extends Method>(method: K, args: Args<K>, d?: string): Step;
export function m<K extends Method>(method: K, ...args: Args<K> extends [] ? [] : never): Step;
export function m(
  method: EventMethod,
  args: readonly [{ $fn: string; throws?: { name: string; message: string } }],
  d?: string,
): Step;
export function m(
  method: keyof MeasurementCapability,
  args: readonly unknown[] = [],
  d = 'a',
): Step {
  return { op: 'call', target: { token: 'measurement', doc: d }, method, args };
}
export const emit = (event: DocumentEvent, d = 'a'): Step => ({ op: 'emit', doc: d, event });
export const close = (id = 'a'): Step => ({
  op: 'call',
  target: 'documents',
  method: 'close',
  args: [id],
});
export const fail = (label: string, message = 'refused', code = 'Refused'): Step => ({
  op: 'settle',
  label,
  error: { name: 'EngineError', message, code },
});
export const listen = [
  m('onScaleChanged', [{ $fn: 'scaleChanged' }]),
  m('onCalibrationRequested', [{ $fn: 'calibrationRequested' }]),
  m('onCalibrationCompleted', [{ $fn: 'calibrationCompleted' }]),
  m('onCalibrationDismissed', [{ $fn: 'calibrationDismissed' }]),
];
export const drag = (
  tool: string,
  from: [number, number],
  to: [number, number],
  page = pr(1),
): Step[] => [
  {
    op: 'call',
    target: { token: 'annotation', doc: 'a' },
    method: 'createPointer',
    args: [tool, 'down', page, { x: from[0], y: from[1] }],
  },
  {
    op: 'call',
    target: { token: 'annotation', doc: 'a' },
    method: 'createPointer',
    args: [tool, 'move', page, { x: (from[0] + to[0]) / 2, y: (from[1] + to[1]) / 2 }],
  },
  {
    op: 'call',
    target: { token: 'annotation', doc: 'a' },
    method: 'createPointer',
    args: [tool, 'move', page, { x: to[0], y: to[1] }],
  },
  {
    op: 'call',
    target: { token: 'annotation', doc: 'a' },
    method: 'createPointer',
    args: [tool, 'up', page, { x: to[0], y: to[1] }],
  },
];
export const vp = (
  bbox: PageBox,
  measure: PdfMeasurement | null = null,
  owned = false,
  name: string | null = null,
): PageMeasurementViewport => ({ bbox, measure, owned, name });
export const viewports = (
  pon: number,
  viewports: PageMeasurementViewport[] | undefined,
  over: Partial<ResponseSpec> = {},
): ResponseSpec => ({
  path: 'page.measure.listViewports',
  pon,
  value: { viewports: viewports ?? [] } satisfies PageMeasurementViewportList,
  ...over,
});
export const meta = (page = pr(1), opId = 'viewport-fact'): MutationMeta => ({
  affectedPages: [page],
  cacheDelta: null,
  opId,
  undoable: true,
});
export const scaleFact = (page = pr(1), opId = 'viewport-fact'): DocumentEvent => ({
  type: 'pages.scaleSet',
  page,
  meta: meta(page, opId),
  origin: { ...LOCAL, tx: { id: opId, index: 0, count: 1 } },
});
export const scaleAnswer = (
  pon = 1,
  over: Partial<ResponseSpec> = {},
  opId = `engine-scale-${pon}`,
): ResponseSpec => ({
  path: 'page.measure.setScale',
  pon,
  value: { page: pr(pon), meta: meta(pr(pon), opId) } satisfies PageScaleResult,
  events: [scaleFact(pr(pon), opId)],
  ...over,
});
export const remeasured = (
  operation: number,
  n: number,
  measure: PdfMeasure,
  pon = 1,
): ResponseSpec =>
  write(operation, [
    { item: updated(dimensionLine(n, { measure, page: pr(pon), ref: objRef(n, pon) }), true) },
  ]);

export const creation = (kind: 'distance' | 'perimeter' | 'area', n: number): Annotation => {
  const shared = {
    color: '#e5484d',
    strokeWidth: 1,
    opacity: 1,
    measure: defaultMeasure('metric'),
    captionEnabled: true,
  };
  const points = [
    { x: 1, y: 1 },
    { x: 50, y: 1 },
    { x: 50, y: 50 },
  ];
  const draft: AnnotationDraft =
    kind === 'distance'
      ? {
          ...shared,
          subtype: 'line',
          intent: 'line-dimension',
          linePoints: { start: { x: 100, y: 100 }, end: { x: 244, y: 100 } },
          lineEndings: { start: 'closed-arrow', end: 'closed-arrow' },
          interiorColor: '#e5484d',
          captionPosition: 'inline',
          leader: { length: 12, extension: 5, offset: 0 },
        }
      : kind === 'perimeter'
        ? {
            ...shared,
            subtype: 'polyline',
            intent: 'polyline-dimension',
            vertices: points,
            lineEndings: { start: 'none', end: 'none' },
          }
        : {
            ...shared,
            subtype: 'polygon',
            intent: 'polygon-dimension',
            vertices: points,
            interiorColor: null,
          };
  return annotationOfNew(draft, { ref: objRef(n) });
};

/** Authored sequential engine snapshots; never reads a runtime request. */
export const scaleSequence = (
  pon: number,
  scales: readonly (PdfMeasure | null)[],
): ResponseSpec[] => [
  viewports(pon, [], { times: 1 }),
  ...scales.flatMap((scale, index) => [
    scaleAnswer(pon, { times: 1, args: [scale] }, `engine-scale-${pon}-${index + 1}`),
    viewports(
      pon,
      scale
        ? [
            vp(
              { x: 0, y: 0, width: pon === 1 ? 600 : 400, height: pon === 1 ? 800 : 500 },
              scale,
              true,
              'EmbedPDF',
            ),
          ]
        : [],
      index === scales.length - 1 ? {} : { times: 1 },
    ),
  ]),
];
