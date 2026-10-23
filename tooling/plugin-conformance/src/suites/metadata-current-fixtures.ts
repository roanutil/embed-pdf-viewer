/** Typed document and service values for the standard and custom Info-dict mirrors. */
import type {
  DocumentMetadata,
  CustomMetadata,
  DocumentEvent,
  EventOrigin,
  PageLayout,
  MetadataUpdateResult,
  CustomMetadataUpdateResult,
} from '@embedpdf/engine-core/runtime';
import type {
  MetadataCapability,
  CustomMetadataCapability,
} from '../../../../packages/plugin/metadata/src/contract';
import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import type { Step } from '../scenario';
export const meta = (over: Partial<DocumentMetadata> = {}): DocumentMetadata => ({
  title: null,
  author: null,
  subject: null,
  keywords: null,
  producer: null,
  creator: null,
  createdAt: null,
  modifiedAt: null,
  trapped: 'unknown',
  ...over,
});
const box = { x: 0, y: 0, width: 600, height: 800 };
export const pages: PageLayout[] = [
  {
    index: 0,
    ref: { kind: 'objectNumber', objectNumber: 1 },
    label: null,
    size: { width: 600, height: 800 },
    rotation: 0,
    userUnit: 1,
    tabOrder: null,
    boxes: { media: box, crop: box, bleed: box, trim: box, art: box },
    pdfCropBox: { left: 0, bottom: 0, right: 600, top: 800 },
  },
];
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
  sub: 'u-9',
  ts: 0,
  serverId: null,
};
const mutationMeta = {
  affectedPages: [],
  cacheDelta: null,
  opId: 'metadata-write',
  undoable: true,
};
export const result = (metadata: DocumentMetadata): MetadataUpdateResult => ({
  metadata,
  meta: mutationMeta,
});
export const customResult = (custom: CustomMetadata): CustomMetadataUpdateResult => ({
  custom,
  meta: mutationMeta,
});
export const fact = (metadata: DocumentMetadata, origin = remote): DocumentEvent => ({
  type: 'metadata.updated',
  ...result(metadata),
  origin,
});
export const customFact = (custom: CustomMetadata, origin = remote): DocumentEvent => ({
  type: 'metadata.customUpdated',
  ...customResult(custom),
  origin,
});
export const doc = (
  responses: ResponseSpec[] = [],
  over: Partial<FakeDocumentSpec> = {},
): FakeDocumentSpec => ({ pages, security: { allows: '*' }, responses, ...over });
export const reads = (metadata = meta(), custom: CustomMetadata = {}): ResponseSpec[] => [
  { path: 'metadata.get', args: [], value: metadata },
  { path: 'metadata.custom.get', args: [], value: custom },
];
export const start: Step = { op: 'call', target: 'kernel', method: 'start' };
export const open = (id = 'a'): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id, bytes: { $bytes: '' } }],
});
export const close: Step = { op: 'call', target: 'documents', method: 'close', args: ['a'] };
export const call = <
  M extends Exclude<keyof MetadataCapability, 'custom' | 'onUpdated' | 'onResynced'>,
>(
  method: M,
  args: Parameters<MetadataCapability[M]>,
  id = 'a',
): Step => ({ op: 'call', target: { token: 'metadata', doc: id }, method, args });
export const custom = <
  M extends Exclude<keyof CustomMetadataCapability, 'onUpdated' | 'onResynced'>,
>(
  method: M,
  args: Parameters<CustomMetadataCapability[M]>,
): Step => ({
  op: 'call',
  target: { token: 'metadata', doc: 'a' },
  method: 'custom.' + method,
  args,
});
export const hook = (
  method: 'onUpdated' | 'onResynced' | 'custom.onUpdated' | 'custom.onResynced',
  fn: string = method,
): Step => ({
  op: 'call',
  target: { token: 'metadata', doc: 'a' },
  method,
  args: [{ $fn: fn, returns: null }],
});
export const emit = (event: DocumentEvent): Step => ({ op: 'emit', doc: 'a', event });
export const settle = (label: string, value: unknown): Step => ({ op: 'settle', label, value });

export const suites = [];
