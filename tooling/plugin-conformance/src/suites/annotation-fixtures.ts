/**
 * Shared builders and harness shims for the `packages/plugin/annotation`
 * suites (`annotation-*.ts`, not `annotation-core-*`). This module writes no
 * vector file; the generator requires every module to export `suites`.
 *
 * Four harness shims live here, and `crates/plugins/annotation/src/harness.rs`
 * mirrors each:
 *
 *   - `crypto.randomUUID`. `toCreateDraft` seeds `/NM` with it; the vectors
 *     need it deterministic, so it answers `00000000-0000-4000-8000-<n>` with
 *     `n` counting from 1, reset when an `annotation` factory runs (and by the
 *     `toCreateDraft` function wrapper). The Rust port takes it as the host
 *     input `AnnotationHost::random_uuid`.
 *   - `console.error`. Every failure the plugin swallows it reports there;
 *     while an annotation factory is current, a call whose message starts
 *     `[annotation]` logs as the callback `console.error`. Rust:
 *     `AnnotationHost::console_error`.
 *   - `appearances`'s `signal`. A vector cannot carry an `AbortSignal`, so
 *     the factory wraps the capability: `{"$abortSignal": name}` becomes the
 *     signal of a controller named `name`, which the `annotation-signals`
 *     plugin's `abort(name)` aborts.
 *   - `comments` and `links` are object members, which a `call` step cannot
 *     reach, so the `annotation-lens` plugin exposes their verbs flat.
 */
import { createCapabilityToken, definePlugin, type AnyPlugin } from '@embedpdf/core';

import { AnnotationToken, annotationPlugin } from '../../../../packages/plugin/annotation/src/index';
import type { AnnotationConfig } from '../../../../packages/plugin/annotation/src/types';
import type { AnnotationHostCapability } from '../../../../packages/plugin/annotation/src/types';

import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import { registerPlugin, type PluginEnv, type Step } from '../scenario';
import type { Suite } from '../vectors';

export const suites: Suite[] = [];

// ── shims ─────────────────────────────────────────────────────────────────────

let uuidSeq = 0;
export const resetUuid = (): void => {
  uuidSeq = 0;
};
export const uuidAt = (n: number): string => `00000000-0000-4000-8000-${n.toString(16).padStart(12, '0')}`;
Object.defineProperty(globalThis.crypto, 'randomUUID', {
  configurable: true,
  writable: true,
  value: () => uuidAt(++uuidSeq),
});

let currentLog: PluginEnv['log'] | null = null;
const originalError = console.error.bind(console);
console.error = (...args: unknown[]) => {
  if (currentLog && typeof args[0] === 'string' && args[0].startsWith('[annotation]')) {
    currentLog('console.error', args);
    return;
  }
  originalError(...args);
};

const controllers = new Map<string, AbortController>();
const controllerNamed = (name: string): AbortController => {
  let controller = controllers.get(name);
  if (!controller) {
    controller = new AbortController();
    controllers.set(name, controller);
  }
  return controller;
};

const withSignals = (capability: AnnotationHostCapability): AnnotationHostCapability => ({
  ...capability,
  appearances: (pon, scale, signal) => {
    const spec = signal as { $abortSignal?: string } | undefined;
    const real = spec?.$abortSignal !== undefined ? controllerNamed(spec.$abortSignal).signal : undefined;
    return capability.appearances(pon, scale, real);
  },
});

registerPlugin('annotation', {
  factory: (config, env) => {
    resetUuid();
    controllers.clear();
    currentLog = env.log;
    const plugin = annotationPlugin((config ?? {}) as AnnotationConfig) as AnyPlugin;
    return {
      ...plugin,
      capability: (ctx) => withSignals(plugin.capability!(ctx) as AnnotationHostCapability),
    } as AnyPlugin;
  },
  tokens: [AnnotationToken],
});

const SignalsToken = createCapabilityToken<{ abort(name: string): void }>('annotation-test/signals');
registerPlugin('annotation-signals', {
  factory: () =>
    ({
      id: 'annotation-test/signals',
      token: SignalsToken,
      capability: () => ({ abort: (name: string) => controllerNamed(name).abort() }),
    }) as AnyPlugin,
  tokens: [SignalsToken],
});

const LensToken = createCapabilityToken<Record<string, (...args: never[]) => unknown>>('annotation-test/lens');
registerPlugin('annotation-lens', {
  factory: () =>
    definePlugin({
      id: 'annotation-test/lens',
      token: LensToken,
      scope: 'document',
      requires: [AnnotationToken],
      capability: (ctx) => {
        const a = () => ctx.get(AnnotationToken);
        return {
          threads: () => a().comments.threads(),
          thread: (ref: never) => a().comments.thread(ref),
          hydration: () => a().comments.hydration(),
          rehydrate: () => a().comments.rehydrate(),
          reply: (ref: never, text: string) => a().comments.reply(ref, text),
          edit: (ref: never, text: string) => a().comments.edit(ref, text),
          setStatus: (ref: never, state: string) => a().comments.setStatus(ref, state),
          setMarked: (ref: never, marked: boolean) => a().comments.setMarked(ref, marked),
          remove: (ref: never) => a().comments.remove(ref),
          removeThread: (ref: never) => a().comments.removeThread(ref),
          permissionsFor: (ref: never) => a().comments.permissionsFor(ref),
          linksOf: (ref: never) => a().links.of(ref),
          linksSet: (ref: never, target: never) => a().links.set(ref, target),
          linksClear: (ref: never) => a().links.clear(ref),
        };
      },
    }) as AnyPlugin,
  tokens: [LensToken],
});

// ── pages ─────────────────────────────────────────────────────────────────────

export const CROP = { left: 0, bottom: 0, right: 600, top: 800 };
export const CROP2 = { left: 10, bottom: 20, right: 410, top: 520 };
export const layout = (index: number, pon: number, crop = CROP, rotation = 0) => ({
  index,
  pageObjectNumber: pon,
  label: null,
  size: { width: crop.right - crop.left, height: crop.top - crop.bottom },
  rotation,
  userUnit: 1,
  boxes: { media: crop, crop },
});
export const PAGES = [layout(0, 1), layout(1, 2, CROP2)];

// ── DTOs ──────────────────────────────────────────────────────────────────────

export const FLAGS = (over: Record<string, boolean> = {}) => ({
  invisible: false,
  hidden: false,
  print: true,
  noZoom: false,
  noRotate: false,
  noView: false,
  readOnly: false,
  locked: false,
  toggleNoView: false,
  lockedContents: false,
  ...over,
});
export const RED = { r: 239, g: 68, b: 68 };
export const BLUE = { r: 56, g: 88, b: 233 };

export const objRef = (n: number, pon = 1) => ({ kind: 'objectNumber', pageObjectNumber: pon, annotObjectNumber: n });

type Dto = Record<string, unknown>;

export const base = (n: number, subtype: string, over: Dto = {}): Dto => {
  const pon = (over.pageObjectNumber as number | undefined) ?? 1;
  return {
    ref: objRef(n, pon),
    pageObjectNumber: pon,
    index: n,
    identityQuality: 'durable',
    nm: null,
    flags: FLAGS(),
    rect: { left: 100, bottom: 600, right: 200, top: 700 },
    contents: null,
    subject: null,
    author: null,
    created: null,
    modified: null,
    blendMode: 'normal',
    inReplyTo: null,
    replyType: null,
    subtype,
    ...over,
  };
};

const filled = { color: RED, interiorColor: null, opacity: 1, strokeWidth: 2, borderStyle: 'solid' };

export const square = (n: number, over: Dto = {}) =>
  base(n, 'square', { ...filled, cloudyIntensity: null, rectDifferences: null, ...over });
export const circle = (n: number, over: Dto = {}) =>
  base(n, 'circle', { ...filled, cloudyIntensity: null, rectDifferences: null, ...over });
export const line = (n: number, over: Dto = {}) =>
  base(n, 'line', {
    ...filled,
    linePoints: { start: { x: 100, y: 700 }, end: { x: 200, y: 650 } },
    lineEndings: { start: 'none', end: 'open-arrow' },
    ...over,
  });
export const polygon = (n: number, over: Dto = {}) =>
  base(n, 'polygon', {
    ...filled,
    cloudyIntensity: null,
    vertices: [
      { x: 100, y: 700 },
      { x: 200, y: 700 },
      { x: 150, y: 620 },
    ],
    ...over,
  });
export const polyline = (n: number, over: Dto = {}) =>
  base(n, 'polyline', {
    ...filled,
    vertices: [
      { x: 100, y: 700 },
      { x: 200, y: 680 },
      { x: 150, y: 620 },
    ],
    lineEndings: { start: 'circle', end: 'none' },
    ...over,
  });
export const ink = (n: number, over: Dto = {}) =>
  base(n, 'ink', {
    color: RED,
    opacity: 1,
    strokeWidth: 3,
    borderStyle: 'solid',
    interiorColor: null,
    inkList: [
      [
        { x: 100, y: 700 },
        { x: 120, y: 690 },
        { x: 140, y: 700 },
      ],
    ],
    ...over,
  });
export const freeText = (n: number, over: Dto = {}) =>
  base(n, 'free-text', {
    intent: 'free-text',
    fontFamily: 'helvetica',
    fontSize: 12,
    textAlign: 'left',
    color: RED,
    interiorColor: null,
    opacity: 1,
    strokeWidth: 1,
    borderStyle: 'solid',
    rectDifferences: null,
    contents: 'Hello',
    ...over,
  });
export const callout = (n: number, over: Dto = {}) =>
  freeText(n, {
    intent: 'free-text-callout',
    rect: { left: 50, bottom: 500, right: 300, top: 700 },
    rectDifferences: { left: 100, bottom: 50, right: 0, top: 0 },
    calloutLine: [
      { x: 60, y: 510 },
      { x: 120, y: 560 },
      { x: 150, y: 600 },
    ],
    lineEnding: 'open-arrow',
    ...over,
  });
const quad = (x: number, y: number, w: number, h: number) => ({
  p1: { x, y: y + h },
  p2: { x: x + w, y: y + h },
  p3: { x, y },
  p4: { x: x + w, y },
});
export const markup = (subtype: string, n: number, over: Dto = {}) =>
  base(n, subtype, {
    color: RED,
    opacity: 1,
    rect: { left: 50, bottom: 730, right: 200, top: 745 },
    quadPoints: [quad(50, 730, 150, 15)],
    ...over,
  });
export const caret = (n: number, over: Dto = {}) =>
  base(n, 'caret', {
    color: RED,
    opacity: 1,
    rect: { left: 85, bottom: 745, right: 95, top: 755 },
    rectDifferences: { left: 0.5, top: 0.5, right: 0.5, bottom: 0.5 },
    ...over,
  });
export const redact = (n: number, over: Dto = {}) =>
  base(n, 'redact', {
    color: RED,
    interiorColor: { r: 0, g: 0, b: 0 },
    opacity: 1,
    quadPoints: [],
    overlayText: null,
    repeat: false,
    fontFamily: 'helvetica',
    fontSize: 0,
    fontColor: { r: 255, g: 255, b: 255 },
    textAlign: 'center',
    ...over,
  });
export const textNote = (n: number, over: Dto = {}) =>
  base(n, 'text', {
    color: { r: 250, g: 204, b: 21 },
    opacity: 1,
    icon: 'comment',
    rect: { left: 100, bottom: 680, right: 120, top: 700 },
    state: null,
    stateModel: null,
    ...over,
  });
export const fileAttachment = (n: number, over: Dto = {}) =>
  base(n, 'file-attachment', {
    color: { r: 250, g: 204, b: 21 },
    opacity: 1,
    icon: 'paperclip',
    rect: { left: 100, bottom: 680, right: 120, top: 700 },
    file: { name: 'a.txt', size: 3, mimeType: 'text/plain' },
    ...over,
  });
export const stamp = (n: number, over: Dto = {}) => base(n, 'stamp', { ...over });
export const link = (n: number, over: Dto = {}) =>
  base(n, 'link', { target: { kind: 'uri', uri: 'https://example.com' }, ...over });
export const widget = (n: number, family: string, over: Dto = {}) =>
  base(n, 'widget', {
    fieldFamily: family,
    color: null,
    interiorColor: null,
    strokeWidth: 1,
    borderStyle: 'solid',
    textAlign: 'left',
    ...over,
  });
export const unsupported = (n: number, over: Dto = {}) => base(n, 'unsupported', { rawSubtype: 16, ...over });

// ── engine answers ────────────────────────────────────────────────────────────

export const snapshot = (byPage: Record<number, Dto[]>, auditHead?: number) => ({
  pages: Object.entries(byPage).map(([pon, annotations]) => ({
    pageState: { pageObjectNumber: Number(pon) },
    annotations,
  })),
  ...(auditHead !== undefined ? { auditHead } : {}),
});
export const hydrate = (byPage: Record<number, Dto[]> = {}, auditHead?: number): ResponseSpec => ({
  path: 'annotations.listRawAll',
  value: snapshot(byPage, auditHead),
});
export const created = (dto: Dto) => ({ created: dto });
export const updated = (dto: Dto, changed = false) => ({ updated: dto, appearance: { changed } });

export const ALL = { allows: '*' as const };

export const doc = (responses: ResponseSpec[] = [hydrate()], over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec => ({
  pages: PAGES,
  security: ALL,
  responses,
  ...over,
});

// ── steps ─────────────────────────────────────────────────────────────────────

export const start: Step = { op: 'call', target: 'kernel', method: 'start' };
export const open = (id = 'a'): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id, bytes: { $bytes: '' } }],
});
export const boot: Step[] = [start, open()];
export const call = (method: string, args: unknown[] = [], doc = 'a'): Step => ({
  op: 'call',
  target: { token: 'annotation', doc },
  method,
  args,
});
export const lens = (method: string, args: unknown[] = [], doc = 'a'): Step => ({
  op: 'call',
  target: { token: 'annotation-test/lens', doc },
  method,
  args,
});
export const ix = (method: string, args: unknown[] = [], doc = 'a'): Step => ({
  op: 'call',
  target: { token: 'interaction', doc },
  method,
  args,
});
export const sel = (method: string, args: unknown[] = [], doc = 'a'): Step => ({
  op: 'call',
  target: { token: 'selection', doc },
  method,
  args,
});
export const emit = (event: Dto, doc = 'a'): Step => ({ op: 'emit', doc, event });
export const settle = (label: string, value?: unknown): Step =>
  value === undefined ? { op: 'settle', label } : { op: 'settle', label, value };
export const fail = (label: string, message = 'refused', name = 'EngineError'): Step => ({
  op: 'settle',
  label,
  error: { name, message, code: 'Refused' },
});
export const advance = (ms: number): Step => ({ op: 'advance', ms });
export const drain: Step = { op: 'drain' };

/** A pointer sample over page `pon` (content point `x`, `y`). */
export const sample = (phase: string, x: number, y: number, over: Dto = {}, pon = 1) => ({
  phase,
  viewport: { x, y },
  page: { pon, point: { x, y }, scale: 1 },
  modifiers: { shift: false, alt: false, ctrl: false, meta: false },
  ...over,
});
export const down = (x: number, y: number, over: Dto = {}) => ix('dispatch', [sample('down', x, y, over)]);
export const move = (x: number, y: number, over: Dto = {}) => ix('dispatch', [sample('move', x, y, over)]);
export const up = (x: number, y: number, over: Dto = {}) => ix('dispatch', [sample('up', x, y, over)]);
export const tool = (id: string): Step => ix('activateTool', [id]);

export const PLUGINS = [{ factory: 'interaction' }, { factory: 'annotation' }];
export const WITH_LENS = [...PLUGINS, { factory: 'annotation-lens' }];
export const SNAP = ['annotation::a'];

/** The model ids the plugin gives object-number refs. */
export const id = (n: number) => `obj:${n}`;
