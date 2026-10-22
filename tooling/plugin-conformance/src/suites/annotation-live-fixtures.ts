/** Current live annotation boundary. All serialized calls and engine answers are current DTOs. */
import { annotationPlugin } from '../../../../packages/plugin/annotation/src/annotation.plugin';
import { AnnotationToken } from '../../../../packages/plugin/annotation/src/host-contract';
import type { AnnotationConfig } from '../../../../packages/plugin/annotation/src/contract';
import {
  annotationOfDraft,
  type Annotation,
  type AnnotationDraft,
  type AnnotationRef,
  type ChangeItem,
  type ChangeResult,
  type DocumentEvent,
  type EventOrigin,
  type PageLayout,
  type PageRef,
} from '@embedpdf/engine-core/runtime';
import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import { onCaseEnd, registerPlugin, type ScenarioCase, type Step } from '../scenario';
import type { Suite } from '../vectors';
import './interaction';
import { objRef, pr } from './annotation-current-fixtures';
export * from './annotation-current-fixtures';
export const suites: Suite[] = [];
let previousEntropy: PropertyDescriptor | undefined;
let originalEntropy = globalThis.crypto.getRandomValues;
let active = false;
let entropy = 0;
let previousError = console.error;
let previousWarn = console.warn;
onCaseEnd(() => {
  if (active) {
    if (previousEntropy)
      Object.defineProperty(globalThis.crypto, 'getRandomValues', previousEntropy);
    else delete (globalThis.crypto as unknown as Record<string, unknown>).getRandomValues;
  }
  if (active) {
    console.error = previousError;
    console.warn = previousWarn;
  }
  active = false;
});
registerPlugin('annotation-live', {
  factory: (config, env) => {
    if (active) throw new Error('[annotation-live] install once per scenario');
    previousError = console.error;
    previousWarn = console.warn;
    console.error = (...args: unknown[]) => {
      if (typeof args[0] === 'string' && args[0].startsWith('[annotation]'))
        env.log('console.error', args);
      else previousError(...args);
    };
    console.warn = (...args: unknown[]) => env.log('console.warn', args);
    entropy = 0;
    previousEntropy = Object.getOwnPropertyDescriptor(globalThis.crypto, 'getRandomValues');
    originalEntropy = globalThis.crypto.getRandomValues;
    active = true;
    // Host input, scoped to this scenario: the queue's UUIDv7 entropy. Never used for engine reads.
    const deterministic = <T extends ArrayBufferView | null>(array: T): T => {
      if (!(array instanceof Uint8Array) || array.byteLength !== 16)
        return Reflect.apply(originalEntropy, globalThis.crypto, [array]) as T;
      const bytes = new Uint8Array(array.buffer, array.byteOffset, array.byteLength);
      bytes.fill(0);
      new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).setUint32(
        bytes.length - 4,
        ++entropy,
      );
      return array;
    };
    Object.defineProperty(globalThis.crypto, 'getRandomValues', {
      configurable: true,
      writable: true,
      value: deterministic,
    });
    return annotationPlugin(config as AnnotationConfig | undefined);
  },
  tokens: [AnnotationToken],
});
export const operationAt = (n: number, ms = 0): string => {
  const time = Math.max(0, Math.floor(ms)).toString(16).padStart(12, '0');
  return `${time.slice(0, 8)}-${time.slice(8)}-7000-8000-${n.toString(16).padStart(12, '0')}`;
};
export const origin = (
  kind: 'local' | 'remote' = 'remote',
  serverId: number | null = null,
): EventOrigin => ({
  kind,
  sessionId: kind === 'local' ? 's1' : 's2',
  sub: kind === 'local' ? null : 'u-9',
  ts: 0,
  serverId,
});
export const LOCAL = origin('local');
export const REMOTE = origin();
export const layout = (
  index: number,
  n: number,
  crop = { left: 0, bottom: 0, right: 600, top: 800 },
  rotation: PageLayout['rotation'] = 0,
): PageLayout => {
  const size = { width: crop.right - crop.left, height: crop.top - crop.bottom };
  const box = { x: 0, y: 0, ...size };
  return {
    index,
    ref: pr(n),
    label: null,
    size,
    rotation,
    userUnit: 1,
    tabOrder: null,
    pdfCropBox: crop,
    boxes: { media: box, crop: box, bleed: box, trim: box, art: box },
  };
};
export const PAGES = [layout(0, 1), layout(1, 2, { left: 10, bottom: 20, right: 410, top: 520 })];
export const snapshot = (annotations: readonly Annotation[] = [], auditHead?: number) => ({
  annotations,
  ...(auditHead === undefined ? {} : { auditHead }),
});
export const hydrate = (
  annotations: readonly Annotation[] = [],
  auditHead?: number,
): ResponseSpec => ({ path: 'annotations.list', value: snapshot(annotations, auditHead) });
export const doc = (
  responses: ResponseSpec[] = [hydrate()],
  over: Partial<FakeDocumentSpec> = {},
): FakeDocumentSpec => ({
  pages: PAGES,
  security: { allows: '*' },
  objectNumbers: Array.from({ length: 200 }, (_, i) => 10001 + i),
  responses,
  ...over,
});
type AnnotationItem = Extract<
  ChangeItem,
  {
    type:
      | 'annotations.create'
      | 'annotations.update'
      | 'annotations.delete'
      | 'annotations.reorder';
  }
>;
type Item = AnnotationItem extends infer I
  ? I extends AnnotationItem
    ? Omit<I, 'meta'>
    : never
  : never;
export const created = (annotation: Annotation): Item => ({
  type: 'annotations.create',
  page: annotation.page,
  annotation,
});
export const updated = (annotation: Annotation, changed = false): Item => ({
  type: 'annotations.update',
  page: annotation.page,
  annotation,
  appearance: { changed, action: changed ? 'regenerated' : 'preserved' },
});
export const deleted = (ref: AnnotationRef): Item => ({
  type: 'annotations.delete',
  page: ref.page,
});
export const materialize = (n: number, draft: AnnotationDraft, page = 1): Annotation =>
  annotationOfDraft(draft, { ref: objRef(n, page) });
/** Typed confirmed mutation envelopes. This constructs answers, never interprets a call or mutates a mirror. */
export function confirmation(
  n: number,
  entries: readonly { item: Item; changed?: readonly AnnotationRef[] }[],
  ms = 0,
): { value: ChangeResult; events: DocumentEvent[] } {
  const opId = operationAt(n, ms);
  const items: ChangeItem[] = [];
  const events: DocumentEvent[] = [];
  const pages = new Map<number, PageRef>();
  for (const [index, { item, changed }] of entries.entries()) {
    const page = item.page;
    pages.set(page.objectNumber, page);
    const refs = changed ?? ('annotation' in item ? [item.annotation.ref] : []);
    const meta = {
      affectedPages: [page],
      cacheDelta: null,
      opId,
      undoable: true,
      changed: [...refs],
    };
    // The explicit item discriminant fixes its event/result shape; no compatibility projection.
    items.push({ ...item, meta } as ChangeItem);
    const from = { ...LOCAL, tx: { id: opId, index, count: entries.length } };
    if (item.type === 'annotations.create' || item.type === 'annotations.update')
      events.push({
        ...item,
        type: item.type === 'annotations.create' ? 'annotations.created' : 'annotations.updated',
        meta,
        origin: from,
      } as DocumentEvent);
    if (item.type === 'annotations.delete')
      events.push({ type: 'annotations.deleted', page, deleted: [...refs], meta, origin: from });
    if (item.type === 'annotations.reorder')
      events.push({ ...item, type: 'annotations.reordered', meta, origin: from });
  }
  return {
    value: {
      items,
      meta: { affectedPages: [...pages.values()], cacheDelta: null, opId, undoable: true },
    },
    events,
  };
}
export const write = (
  n: number,
  entries: readonly { item: Item; changed?: readonly AnnotationRef[] }[],
  options: { defer?: string; ms?: number } = {},
): ResponseSpec => ({
  path: 'apply',
  times: 1,
  ...confirmation(n, entries, options.ms),
  ...(options.defer ? { defer: options.defer } : {}),
});
export const refused = (message = 'intended refusal', code = 'Refused'): ResponseSpec => ({
  path: 'apply',
  times: 1,
  error: { name: 'EngineError', message, code },
});
export const call = (method: string, args: readonly unknown[] = [], d = 'a'): Step => ({
  op: 'call',
  target: { token: 'annotation', doc: d },
  method,
  args,
});
export const start: Step = { op: 'call', target: 'kernel', method: 'start' };
export const open = (id = 'a'): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id, bytes: { $bytes: '' } }],
});
export const boot: Step[] = [start, open()];
export const emit = (event: DocumentEvent, d = 'a'): Step => ({ op: 'emit', doc: d, event });
export const settle = (label: string, value?: unknown): Step => ({
  op: 'settle',
  label,
  ...(value === undefined ? {} : { value }),
});
export const fail = (label: string, message = 'intended refusal'): Step => ({
  op: 'settle',
  label,
  error: { name: 'EngineError', code: 'Refused', message },
});
export const advance = (ms: number): Step => ({ op: 'advance', ms });
export const drain: Step = { op: 'drain' };
export const PLUGINS = [{ factory: 'interaction' }, { factory: 'annotation-live' }];
export const SNAP = ['annotation::a'];
export function scenario(
  name: string,
  covers: string,
  steps: Step[],
  responses: ResponseSpec[] = [hydrate()],
  over:
    | (Partial<Omit<ScenarioCase, 'snapshot' | 'reason'>> & {
        snapshot?: readonly string[];
        reason?: never;
      })
    | (Partial<Omit<ScenarioCase, 'snapshot' | 'reason'>> & {
        snapshot: 'none';
        reason: string;
      }) = {},
): ScenarioCase {
  const base = {
    name,
    covers,
    plugins: PLUGINS,
    engine: { documents: { a: doc(responses) } },
    steps: [...boot, ...steps],
  };
  return over.snapshot === 'none'
    ? { ...base, ...over, snapshot: 'none', reason: over.reason }
    : { ...base, snapshot: SNAP, ...over };
}
export const createdFact = (annotation: Annotation, from: EventOrigin = REMOTE): DocumentEvent => ({
  type: 'annotations.created',
  page: annotation.page,
  annotation,
  origin: from,
  meta: {
    affectedPages: [annotation.page],
    cacheDelta: null,
    changed: [annotation.ref],
    opId: 'remote-create',
    undoable: true,
  },
});
export const updatedFact = (
  annotation: Annotation,
  changed = false,
  from: EventOrigin = REMOTE,
): DocumentEvent => ({
  type: 'annotations.updated',
  page: annotation.page,
  annotation,
  appearance: { changed, action: changed ? 'regenerated' : 'preserved' },
  origin: from,
  meta: {
    affectedPages: [annotation.page],
    cacheDelta: null,
    changed: [annotation.ref],
    opId: 'remote-update',
    undoable: true,
  },
});
export const deletedFact = (
  refs: readonly AnnotationRef[],
  from: EventOrigin = REMOTE,
): DocumentEvent => ({
  type: 'annotations.deleted',
  page: refs[0]!.page,
  deleted: [...refs],
  origin: from,
  meta: {
    affectedPages: [refs[0]!.page],
    cacheDelta: null,
    changed: [...refs],
    opId: 'remote-delete',
    undoable: true,
  },
});
export const restoredFact = (page: PageRef, annotations: Annotation[] = []): DocumentEvent => ({
  type: 'annotations.restored',
  page,
  annotations,
  origin: REMOTE,
  meta: {
    affectedPages: [page],
    cacheDelta: null,
    changed: annotations.map((a) => a.ref),
    opId: 'remote-restore',
    undoable: true,
  },
});
/** A complete PNG header with a 100×50 intrinsic size (the stamp sniffer's input). */
export const PNG = {
  $bytes: Buffer.from([
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 100, 0, 0, 0, 50,
  ]).toString('base64'),
};
