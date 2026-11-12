/** Current measurement inputs and static engine answers, constructed before execution. */
import { viewportsWithScale } from '@embedpdf/engine-core/runtime';
import type {
  Annotation,
  AnnotationDraft,
  ChangeItem,
  ChangeResult,
  DocumentEvent,
  MutationMeta,
  PageBox,
  PageLayout,
  PageListSnapshot,
  PageMeasurementViewport,
  PageRef,
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
  confirmation,
  doc,
  hydrate,
  layout,
  objRef,
  operationAt,
  origin,
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
  doc([...responses, hydrate()], {
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
/** A page list as the engine answers it. */
export const listOf = (pages: readonly PageLayout[]): PageListSnapshot => ({
  pageCount: pages.length,
  pages: [...pages],
  labels: [],
  namedPages: [],
});
/** The page with these viewports on its layout; one without any leaves `viewports` out, as the engine does. */
export const withViewports = (
  page: PageLayout,
  viewports: readonly PageMeasurementViewport[],
): PageLayout => {
  const { viewports: _old, ...rest } = page;
  return viewports.length > 0 ? { ...rest, viewports: [...viewports] } : rest;
};
export const meta = (page = pr(1), opId = 'viewport-fact'): MutationMeta => ({
  affectedPages: [page],
  cacheDelta: null,
  opId,
  undoable: true,
});
/** A remote `pages.scaleSet`: another session set these pages' scale; `layout` is the list it left. */
export const scaleFact = (
  pages: readonly PageRef[],
  measure: PdfMeasure | null,
  layout: readonly PageLayout[],
  opId = 'viewport-fact',
): DocumentEvent => ({
  type: 'pages.scaleSet',
  page: pages[0]!,
  pages: [...pages],
  measure,
  layout: listOf(layout),
  meta: { ...meta(pages[0], opId), affectedPages: [...pages] },
  origin: { ...origin('remote'), transaction: { id: opId, index: 0, count: 1 } },
});
/** One page's scale in a change (`null`: back to the default). */
export interface ScaleWrite {
  readonly page: PageRef;
  readonly measure: PdfMeasure | null;
}
type AnnotationEntry = Parameters<typeof confirmation>[1][number];
/**
 * Authored engine answers to a case's scale changes, in the order the case
 * makes them. A change's `pages.setScale` items come first, one per scale
 * (the pages that get the same one go together), then the re-measured
 * annotations; the engine publishes their events before it answers. Each
 * layout follows from the case's starting pages and the answers before it,
 * as the engine holds them: `viewportsWithScale`, the function the engine
 * contract names for it. Nothing here reads a runtime request.
 */
export function scaleEngine(start: readonly PageLayout[]) {
  let pages = [...start];
  const answer = (
    n: number,
    writes: readonly ScaleWrite[],
    entries: readonly AnnotationEntry[] = [],
  ): { value: ChangeResult; events: DocumentEvent[] } => {
    const opId = operationAt(n);
    const groups = new Map<string, { measure: PdfMeasure | null; pages: PageRef[] }>();
    for (const { page, measure } of writes) {
      const key = JSON.stringify(measure);
      const group = groups.get(key) ?? { measure, pages: [] };
      group.pages.push(page);
      groups.set(key, group);
    }
    const annotations = confirmation(n, entries);
    const count = groups.size + annotations.events.length;
    const items: ChangeItem[] = [];
    const events: DocumentEvent[] = [];
    const affected = new Map<number, PageRef>();
    for (const { measure, pages: scaled } of groups.values()) {
      pages = pages.map((page) =>
        scaled.some((ref) => ref.objectNumber === page.ref.objectNumber)
          ? withViewports(page, viewportsWithScale(page.viewports ?? [], page.size, measure))
          : page,
      );
      for (const ref of scaled) affected.set(ref.objectNumber, ref);
      const result = {
        page: scaled[0]!,
        pages: [...scaled],
        measure,
        layout: listOf(pages),
        meta: { affectedPages: [...scaled], cacheDelta: null, opId, undoable: true },
      };
      const from = { ...LOCAL, transaction: { id: opId, index: events.length, count } };
      items.push({ type: 'pages.setScale', ...result });
      events.push({ type: 'pages.scaleSet', ...result, origin: from });
    }
    items.push(...annotations.value.items);
    for (const event of annotations.events) {
      const from = { ...LOCAL, transaction: { id: opId, index: events.length, count } };
      events.push({ ...event, origin: from } as DocumentEvent);
    }
    for (const page of annotations.value.meta.affectedPages) affected.set(page.objectNumber, page);
    return {
      value: {
        items,
        meta: { affectedPages: [...affected.values()], cacheDelta: null, opId, undoable: true },
      },
      events,
    };
  };
  return {
    /** The pages as the engine holds them after the answers so far. */
    get pages(): readonly PageLayout[] {
      return pages;
    },
    /** The answer to the next change: `n` names its operation, as `write` does. */
    change: (
      n: number,
      writes: readonly ScaleWrite[],
      entries: readonly AnnotationEntry[] = [],
      options: { defer?: string } = {},
    ): ResponseSpec => ({
      path: 'apply',
      times: 1,
      ...answer(n, writes, entries),
      ...(options.defer ? { defer: options.defer } : {}),
    }),
  };
}
/** The re-measured dimension line `n` on page `pon`, as one of a change's annotation entries. */
export const remeasuredEntry = (n: number, measure: PdfMeasure, pon = 1): AnnotationEntry => ({
  item: updated(dimensionLine(n, { measure, page: pr(pon), ref: objRef(n, pon) }), true),
});
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
