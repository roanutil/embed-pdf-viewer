/** Typed page snapshots and explicit engine answers; no page-edit rules are simulated. */
import type {
  PageLayout,
  PageRef,
  PdfRotation,
  ChangeItem,
  ChangeResult,
  DocumentEvent,
  EventOrigin,
} from '@embedpdf/engine-core/runtime';
import type { PageEditCapability } from '../../../../packages/plugin/page-edit/src/contract';
import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import type { Step, ScenarioCase } from '../scenario';
export const pr = (objectNumber: number): PageRef => ({ kind: 'objectNumber', objectNumber });
export const page = (
  n: number,
  index: number,
  rotation: PdfRotation = 0,
  width = 100,
  height = 200,
): PageLayout => {
  const b = { x: 0, y: 0, width, height };
  return {
    index,
    ref: pr(n),
    label: null,
    size: { width, height },
    rotation,
    userUnit: 1,
    tabOrder: null,
    boxes: { media: b, crop: b, bleed: b, trim: b, art: b },
    pdfCropBox: { left: 0, bottom: 0, right: width, top: height },
  };
};
export const three = [page(10, 0), page(20, 1, 90, 300, 400), page(30, 2, 270, 500, 600)];
export const layout = (pages: readonly PageLayout[]) => ({
  pageCount: pages.length,
  pages: [...pages],
  namedPages: [],
});
export const local: EventOrigin = {
  kind: 'local',
  sessionId: 's1',
  sub: null,
  ts: 0,
  serverId: null,
};
export const remote: EventOrigin = {
  kind: 'remote',
  sessionId: 's2',
  sub: 'other',
  ts: 0,
  serverId: null,
};
export const meta = {
  affectedPages: [],
  cacheDelta: null,
  opId: 'page-edit-write',
  undoable: true,
};
export const rotated = (
  pages: PageRef[],
  rotation: PdfRotation,
  after: PageLayout[],
): ChangeItem => ({ type: 'pages.rotate', pages, rotation, layout: layout(after), meta });
export const reordered = (pages: PageRef[], after: PageLayout[]): ChangeItem => ({
  type: 'pages.reorder',
  pages,
  layout: layout(after),
  meta,
});
export const deleted = (pages: PageRef[], after: PageLayout[]): ChangeItem => ({
  type: 'pages.delete',
  pages,
  layout: layout(after),
  meta,
});
export const inserted = (
  type: 'pages.insert' | 'pages.insertBlank',
  nums: number[],
  after: PageLayout[],
): ChangeItem => ({ type, insertedPages: nums.map(pr), layout: layout(after), meta });
export const result = (...items: ChangeItem[]): ChangeResult => ({ items, meta });
export const fact = (item: ChangeItem, origin = local): DocumentEvent => {
  switch (item.type) {
    case 'pages.rotate':
      return { ...item, type: 'pages.rotated', origin };
    case 'pages.reorder':
      return { ...item, type: 'pages.reordered', origin };
    case 'pages.delete':
      return { ...item, type: 'pages.deleted', origin };
    case 'pages.insert':
    case 'pages.insertBlank':
      return { ...item, type: 'pages.inserted', origin };
    default:
      throw Error('not a page item');
  }
};
export const answer = (...items: ChangeItem[]): ResponseSpec => ({
  path: 'apply',
  times: 1,
  value: result(...items),
  events: items.map((i) => fact(i)),
});
export const doc = (
  responses: ResponseSpec[] = [],
  over: Partial<FakeDocumentSpec> = {},
): FakeDocumentSpec => ({ pages: three, security: { allows: '*' }, responses, ...over });
export const start: Step = { op: 'call', target: 'kernel', method: 'start' };
export const open = (id = 'a'): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id, bytes: { $bytes: '' } }],
});
export const close: Step = { op: 'call', target: 'documents', method: 'close', args: ['a'] };
export const edit = <M extends keyof PageEditCapability>(
  method: M,
  args: Parameters<PageEditCapability[M]>,
  id = 'a',
): Step => ({ op: 'call', target: { token: 'page-edit', doc: id }, method, args });
export const readPages: Step = {
  op: 'call',
  target: 'documents',
  method: 'listPages',
  args: ['a'],
};
export const emit = (event: DocumentEvent): Step => ({ op: 'emit', doc: 'a', event });
export const settle = (label: string, value: unknown): Step => ({ op: 'settle', label, value });
export const bytes = { $bytes: 'AQID' };
/** Only binary wire markers cross this scenario boundary; actual capability arguments are decoded by the shared harness. */
export const copy = (
  method: 'insertFromBytes',
  source = bytes,
  options: Parameters<PageEditCapability['insertFromBytes']>[1] = {},
): Step => ({
  op: 'call',
  target: { token: 'page-edit', doc: 'a' },
  method,
  args: [source, options],
});
export const scenario = (
  name: string,
  covers: string,
  responses: ResponseSpec[],
  steps: Step[],
  over: Partial<FakeDocumentSpec> = {},
): ScenarioCase => ({
  name,
  covers,
  plugins: [{ factory: 'page-edit' }],
  engine: { documents: { a: doc(responses, over) } },
  steps: [start, open(), ...steps],
});
/** Bind explicit fixture confirmations to the deterministic request ids, not to page-edit business rules. */
export function bindTransactions(c: ScenarioCase): ScenarioCase {
  const deferred = new Map<string, string>();
  const isResult = (v: unknown): v is ChangeResult =>
    typeof v === 'object' && v !== null && 'items' in v && Array.isArray(v.items) && 'meta' in v;
  const bound = (v: unknown, id: string): ChangeResult => {
    if (!isResult(v)) throw Error('page fixture expected ChangeResult');
    return {
      ...v,
      meta: { ...v.meta, opId: id },
      items: v.items.map((item) => {
        switch (item.type) {
          case 'pages.rotate':
          case 'pages.reorder':
          case 'pages.delete':
          case 'pages.insert':
          case 'pages.insertBlank':
            return { ...item, meta: { ...item.meta, opId: id } };
          default:
            throw Error('page fixture expected page result');
        }
      }),
    };
  };
  const documents = Object.fromEntries(
    Object.entries(c.engine?.documents ?? {}).map(([key, d]) => {
      let sequence = 0;
      const responses = d.responses?.map((r) => {
        if (r.path !== 'apply') return r;
        const prefix =
          c.name === 'insert-a-subset-names-the-missing-index' ? '00004996-02d2' : '00000000-0000';
        const id = `${prefix}-7000-8000-${(++sequence).toString(16).padStart(12, '0')}`;
        if (r.defer) deferred.set(r.defer, id);
        return {
          ...r,
          ...(r.value === undefined ? {} : { value: bound(r.value, id) }),
          ...(r.events
            ? {
                events: r.events.map((e, index) => {
                  switch (e.type) {
                    case 'pages.rotated':
                    case 'pages.reordered':
                    case 'pages.deleted':
                    case 'pages.inserted':
                      return {
                        ...e,
                        meta: { ...e.meta, opId: id },
                        origin: { ...e.origin, tx: { id, index, count: r.events!.length } },
                      };
                    default:
                      throw Error('page fixture expected page event');
                  }
                }),
              }
            : {}),
        };
      });
      return [key, { ...d, responses }];
    }),
  );
  const steps = c.steps.map((step) =>
    step.op === 'settle' && deferred.has(step.label)
      ? { ...step, value: bound(step.value, deferred.get(step.label)!) }
      : step,
  );
  return { ...c, engine: { ...c.engine, documents }, steps };
}
