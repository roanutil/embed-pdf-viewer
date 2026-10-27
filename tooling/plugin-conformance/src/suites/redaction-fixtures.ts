/** Current redaction host seams and typed, static engine answers. */
import type {
  Annotation,
  DocumentEvent,
  EventOrigin,
  PageGeometrySnapshot,
  UprightGeometryRun,
  RedactionApplyResult,
  RedactionApplyScope,
  SearchMatch,
  PdfTextSegment,
} from '@embedpdf/engine-core/runtime';
import { redactionPlugin } from '../../../../packages/plugin/redaction/src/redaction.plugin';
import {
  RedactionToken,
  type RedactionCapability,
  type RedactionConfig,
} from '../../../../packages/plugin/redaction/src/contract';
import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import { registerPlugin, type Step } from '../scenario';
import { doc, hydrate, pr, quad, LOCAL } from './annotation-live-fixtures';
import './annotation-live-inputs';
import { searchPlugin } from '../../../../packages/plugin/search/src/search.plugin';
import { SearchToken } from '../../../../packages/plugin/search/src/contract';
registerPlugin('redaction-search', { factory: () => searchPlugin(), tokens: [SearchToken] });
export * from './annotation-live-fixtures';
export { sel, ix, sample } from './annotation-live-inputs';
export const suites = [];
registerPlugin('redaction-current', {
  factory: (config) => redactionPlugin(config as RedactionConfig | undefined),
  tokens: [RedactionToken],
});
type Events = Extract<keyof RedactionCapability, `on${string}`>;
type Method = Exclude<keyof RedactionCapability, Events>;
type Args<K extends Method> = RedactionCapability[K] extends (...args: infer A) => unknown
  ? A
  : never;
export function rd<K extends Method>(method: K, args: Args<K>, d?: string): Step;
export function rd<K extends Method>(method: K, ...args: [] extends Args<K> ? [] : never): Step;
export function rd(
  method: Events,
  args: readonly [{ $fn: string; throws?: { name: string; message: string } }],
  d?: string,
): Step;
export function rd(
  method: keyof RedactionCapability,
  args: readonly unknown[] = [],
  d = 'a',
): Step {
  return { op: 'call', target: { token: 'redaction', doc: d }, method, args };
}
export const PLUGINS = [
  { factory: 'interaction' },
  { factory: 'annotation-live' },
  { factory: 'redaction-current' },
];
export const WITH_SELECTION = [
  { factory: 'interaction' },
  { factory: 'annotation-live-selection' },
  { factory: 'annotation-live' },
  { factory: 'redaction-current' },
];
export const SNAP = ['redaction::a', 'annotation::a'];
export const docOf = (
  responses: ResponseSpec[] = [hydrate()],
  over: Partial<FakeDocumentSpec> = {},
): FakeDocumentSpec => doc(responses, { services: ['redaction'], ...over });
export const APPLY_CAPS = ['doc.redact', 'doc.pages.modify', 'doc.annotate.modify'];
export const OVERLAY = {
  overlay: { fill: '#ff0000', text: { color: '#00ff00', fontFamily: 'courier', fontSize: 0 } },
} satisfies RedactionConfig;
export const result = (
  scope: RedactionApplyScope,
  removedAnnotationCount = 2,
  opId = 'redaction-apply',
): RedactionApplyResult => {
  const pages =
    'pages' in scope
      ? scope.pages
      : [...new Map(scope.annotations.map((ref) => [ref.page.objectNumber, ref.page])).values()];
  return {
    scope,
    results: pages.map((page, index) => ({
      page,
      status: 'applied',
      removedAnnotationCount: index === 0 ? removedAnnotationCount : 0,
    })),
    removedAnnotationCount,
    meta: { affectedPages: pages, cacheDelta: null, opId, undoable: false },
  };
};
export const applied = (
  answer: RedactionApplyResult,
  origin: EventOrigin = LOCAL,
): DocumentEvent => ({
  type: 'redaction.applied',
  ...answer,
  origin: { ...origin, tx: { id: answer.meta.opId, index: 0, count: 1 } },
});
export const applyAnswer = (
  answer: RedactionApplyResult,
  over: Partial<ResponseSpec> = {},
): ResponseSpec => ({
  path: 'redaction.apply',
  args: [answer.scope],
  value: answer,
  events: [applied(answer)],
  times: 1,
  ...over,
});
export const reload = (annotations: Annotation[] = [], pages = [pr(1), pr(2)]): ResponseSpec => ({
  path: 'annotations.list',
  args: [{ pages }],
  value: { annotations },
});
const run = (text: string, x: number, y: number, start = 0): UprightGeometryRun => ({
  start,
  rect: { x, y, width: text.length * 6, height: 12 },
  fontSize: 12,
  glyphs: [...text].map((ch, i) => ({
    loose: { x: x + i * 6, y, width: 6, height: 12 },
    ...(ch === ' '
      ? { space: true as const }
      : { tight: { x: x + i * 6 + 0.5, y: y + 1, width: 5, height: 10 } }),
  })),
});
export const READS: ResponseSpec[] = [
  {
    path: 'page.text.layout',
    pon: 1,
    value: {
      $textLayout: {
        runs: [
          {
            ...run('Hello world', 50, 50),
            glyphs: [
              ...run('Hello world', 50, 50).glyphs,
              { loose: { x: 0, y: 0, width: 0, height: 0 }, empty: true },
              { loose: { x: 0, y: 0, width: 0, height: 0 }, empty: true },
            ],
          },
          run('Second line', 50, 70, 13),
        ],
      } satisfies PageGeometrySnapshot,
    },
  },
  {
    path: 'page.text.layout',
    pon: 2,
    value: { $textLayout: { runs: [run('Page two', 30, 20)] } satisfies PageGeometrySnapshot },
  },
  { path: 'page.text.read', pon: 1, value: { text: 'Hello world\r\nSecond line', charCount: 24 } },
  { path: 'page.text.read', pon: 2, value: { text: 'Page two', charCount: 8 } },
];
export const load = [
  { op: 'call', target: { token: 'selection', doc: 'a' }, method: 'ensureLoaded', args: [pr(1)] },
  { op: 'call', target: { token: 'selection', doc: 'a' }, method: 'ensureLoaded', args: [pr(2)] },
] satisfies Step[];
export const range = (from: [number, number], to: [number, number]) => ({
  start: { page: pr(from[0]), index: from[1] },
  end: { page: pr(to[0]), index: to[1] },
});
export const segment = (x: number, y: number, width: number, height: number): PdfTextSegment => ({
  quad: quad(x, y, width, height),
  rect: { x, y, width, height },
  advance: 1,
});
export const hit = (pon: number, charStart: number, segments: PdfTextSegment[]): SearchMatch => ({
  page: pr(pon),
  start: charStart,
  count: 3,
  segments,
});
