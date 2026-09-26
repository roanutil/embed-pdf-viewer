/**
 * Shared builders and harness shims for the `packages/plugin/form` suites
 * (`form-*.ts`). This module writes no vector file; the generator requires
 * every module to export `suites`.
 *
 * Two harness shims live here, and `crates/plugins/form/src/harness.rs`
 * mirrors each:
 *
 *   - `console.warn` and `console.error`. The plugin reports a failed
 *     snapshot load to `console.warn` and a failed click placement to
 *     `console.error`; while a `form` factory is current, a call whose
 *     message starts `[form]` logs as the callback `console.warn` or
 *     `console.error`. The Rust port takes both sinks as `FormHost`.
 *   - `form-scripting` is a document-scoped driver that builds a standalone
 *     `createFormScriptingController` (the `config` arm stamp uses) over the
 *     document's handle, with a scripted sandbox from `acrojs.ts`. The
 *     QuickJS default does not port, so the driver requires a sandbox spec.
 *
 * `form-probe` exposes the annotation plugin's `behaviorFor` as the id of the
 * engaged behavior, because a behavior object prints its functions as refs
 * and a Rust behavior has none to print.
 */
import { definePlugin, type AnyPlugin } from '@embedpdf/core';
import type { DocumentHandle } from '@embedpdf/engine-core/runtime';

import { widget as annotationWidget } from './annotation-fixtures';
import { AnnotationToken as AnnotationHostToken } from '../../../../packages/plugin/annotation/src/types';
import { formPlugin, FormToken } from '../../../../packages/plugin/form/src/index';
import { createFormScriptingController } from '../../../../packages/plugin/form/src/scripting';
import type { FormScriptingOptions } from '../../../../packages/plugin/form/src/types';

import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import { registerPlugin, type PluginEnv, type Step } from '../scenario';
import type { Suite } from '../vectors';
import { scriptedCallback, scriptedSandboxFactory, type AnswerSpec, type SandboxSpec } from './acrojs';

export const suites: Suite[] = [];

// ── shims ─────────────────────────────────────────────────────────────────────

let consoleEnv: PluginEnv | null = null;
/** Installed on the first `form` build rather than at import, so it wraps the
 *  shims later suites install (render and i18n route every warning to their
 *  own last case) instead of sitting underneath them. */
let consoleShimmed = false;
function shimConsole(): void {
  if (consoleShimmed) return;
  consoleShimmed = true;
  const originalWarn = console.warn;
  const originalError = console.error;
  console.warn = (...args: unknown[]) => {
    if (consoleEnv && typeof args[0] === 'string' && args[0].startsWith('[form]')) {
      consoleEnv.log('console.warn', args);
      return;
    }
    originalWarn(...args);
  };
  console.error = (...args: unknown[]) => {
    if (consoleEnv && typeof args[0] === 'string' && args[0].startsWith('[form]')) {
      consoleEnv.log('console.error', args);
      return;
    }
    originalError(...args);
  };
}

registerPlugin('form', {
  factory: (_config, env) => {
    shimConsole();
    consoleEnv = env;
    return formPlugin() as AnyPlugin;
  },
  tokens: [FormToken],
});

type Scripted = AnswerSpec & { readonly $fn: string };

/** The standalone controller's config: `FormScriptingOptions` with a scripted
 *  sandbox and scripted clock, identity and seed callbacks. */
export interface StandaloneConfig {
  readonly sandbox: SandboxSpec;
  readonly identity?: Record<string, unknown> | Scripted;
  readonly fileName?: Scripted;
  readonly now?: Scripted;
  readonly utcOffsetMinutes?: Scripted;
  readonly randomSeed?: Scripted;
  readonly budget?: FormScriptingOptions['budget'];
}

registerPlugin('form-scripting', {
  factory: (config, env) => {
    const spec = config as StandaloneConfig;
    const token = env.token('form-scripting');
    const fn = <T>(s: Scripted | undefined) => (s === undefined ? undefined : (scriptedCallback(s.$fn, s, env) as () => T));
    const identity = spec.identity;
    const options: FormScriptingOptions = {
      sandboxFactory: scriptedSandboxFactory(spec.sandbox, env),
      ...(identity === undefined
        ? {}
        : { identity: '$fn' in identity ? fn<Record<string, string>>(identity as Scripted) : identity }),
      ...(spec.fileName ? { fileName: fn<string>(spec.fileName) } : {}),
      ...(spec.now ? { now: fn<number>(spec.now) } : {}),
      ...(spec.utcOffsetMinutes ? { utcOffsetMinutes: fn<number>(spec.utcOffsetMinutes) } : {}),
      ...(spec.randomSeed ? { randomSeed: fn<number>(spec.randomSeed) } : {}),
      ...(spec.budget ? { budget: spec.budget } : {}),
    } as FormScriptingOptions;
    return definePlugin({
      id: 'form-scripting',
      token,
      scope: 'document',
      capability: (ctx) => {
        const controller = createFormScriptingController({
          doc: ctx.doc as DocumentHandle,
          document: () => ctx.document(),
          config: options,
        });
        return {
          commit: (ref: never, value: never) => controller.commit(ref, value),
          activate: (ref: never, action: never) => controller.activate(ref, action),
          recalculate: () => controller.recalculate(),
          dispose: () => controller.dispose(),
        };
      },
    });
  },
});

registerPlugin('form-probe', {
  factory: (_config, env) =>
    definePlugin({
      id: 'form-probe',
      token: env.token('form-probe'),
      scope: 'document',
      requires: [AnnotationHostToken],
      capability: (ctx) => ({
        behaviorFor: (subtype: string) =>
          ctx.get(AnnotationHostToken).behaviorFor({ subtype } as never)?.id ?? null,
      }),
    }),
});

// ── pages ─────────────────────────────────────────────────────────────────────

export const CROP = { left: 0, bottom: 0, right: 600, top: 800 };
export const CROP2 = { left: 10, bottom: 20, right: 410, top: 520 };
export const layout = (index: number, pon: number, crop = CROP) => ({
  index,
  pageObjectNumber: pon,
  label: null,
  size: { width: crop.right - crop.left, height: crop.top - crop.bottom },
  rotation: 0,
  userUnit: 1,
  boxes: { media: crop, crop },
});
export const PAGES = [layout(0, 1), layout(1, 2, CROP2)];

// ── field DTOs ────────────────────────────────────────────────────────────────

type Dto = Record<string, unknown>;

export const ref = (fieldObjectNumber: number) => ({ kind: 'objectNumber', fieldObjectNumber });
export const fqn = (name: string) => ({ kind: 'fqn', name });
export const FLAGS = (over: Record<string, unknown> = {}) => ({ readOnly: false, required: false, noExport: false, raw: 0, ...over });

/** Common members of every family; `widgets` are `[annot, pon]` pairs or
 *  whole widget objects. */
const common = (n: number, name: string, family: string, widgets: unknown[], over: Dto) => ({
  ref: n > 0 ? ref(n) : fqn(name),
  fieldObjectNumber: n,
  name,
  family,
  origin: 'acroform',
  flags: FLAGS(),
  alternateName: null,
  mappingName: null,
  widgets: widgets.map((w) => (Array.isArray(w) ? { annotObjectNumber: w[0], pageObjectNumber: w[1] } : w)),
  ...over,
});

export const text = (n: number, name: string, value = '', over: Dto = {}, widgets: unknown[] = [[n + 100, 1]]) =>
  common(n, name, 'text', widgets, {
    valueEntry: value === '' ? { kind: 'none' } : { kind: 'scalar', value },
    defaultValueEntry: { kind: 'none' },
    value,
    defaultValue: '',
    maxLength: null,
    multiline: false,
    password: false,
    comb: false,
    ...over,
  });

export const checkbox = (n: number, name: string, checked = false, over: Dto = {}, widgets: unknown[] = [{ annotObjectNumber: n + 100, pageObjectNumber: 1, onState: 'Yes' }]) =>
  common(n, name, 'checkbox', widgets, {
    valueEntry: checked ? { kind: 'scalar', value: 'Yes' } : { kind: 'none' },
    defaultValueEntry: { kind: 'none' },
    checked,
    ...over,
  });

export const radio = (n: number, name: string, widgets: unknown[], over: Dto = {}) =>
  common(n, name, 'radio', widgets, {
    valueEntry: { kind: 'none' },
    defaultValueEntry: { kind: 'none' },
    ...over,
  });

export const OPTIONS = [
  { label: 'Red', value: 'r' },
  { label: 'Green', value: 'g' },
];

export const combobox = (n: number, name: string, value = '', over: Dto = {}, widgets: unknown[] = [[n + 100, 1]]) =>
  common(n, name, 'combobox', widgets, {
    valueEntry: value === '' ? { kind: 'none' } : { kind: 'scalar', value },
    defaultValueEntry: { kind: 'none' },
    value,
    edit: false,
    options: OPTIONS,
    ...over,
  });

export const listbox = (n: number, name: string, selected: string[] = [], over: Dto = {}, widgets: unknown[] = [[n + 100, 1]]) =>
  common(n, name, 'listbox', widgets, {
    valueEntry: selected.length ? { kind: 'array', values: selected } : { kind: 'none' },
    defaultValueEntry: { kind: 'none' },
    multiSelect: true,
    options: OPTIONS,
    selectedValues: selected,
    ...over,
  });

export const pushbutton = (n: number, name: string, over: Dto = {}, widgets: unknown[] = [[n + 100, 1]]) =>
  common(n, name, 'pushbutton', widgets, { valueEntry: { kind: 'none' }, defaultValueEntry: { kind: 'none' }, ...over });

export const signature = (n: number, name: string, over: Dto = {}, widgets: unknown[] = [[n + 100, 1]]) =>
  common(n, name, 'signature', widgets, { valueEntry: { kind: 'none' }, defaultValueEntry: { kind: 'none' }, ...over });

export const snapshot = (fields: unknown[], calculationOrder: unknown[] = []) => ({
  formKind: 'acroform',
  needsAppearances: false,
  fields,
  calculationOrder,
});

// ── widget annotations (the widget plane) ─────────────────────────────────────

export const aref = (pon: number, n: number) => ({ kind: 'objectNumber', pageObjectNumber: pon, annotObjectNumber: n });
/** A widget annotation DTO as the engine lists it (the annotation plugin
 *  ingests it whole). */
export const widgetAnnot = (pon: number, n: number, rect = { left: 100, bottom: 600, right: 260, top: 624 }, over: Dto = {}) =>
  annotationWidget(n, 'text', { pageObjectNumber: pon, rect, ...over });

// ── engine answers ────────────────────────────────────────────────────────────

export const list = (fields: unknown[], extra: Partial<ResponseSpec> = {}, calculationOrder: unknown[] = []): ResponseSpec => ({
  path: 'forms.list',
  value: snapshot(fields, calculationOrder),
  ...extra,
});
export const annotations = (pon: number, items: unknown[], extra: Partial<ResponseSpec> = {}): ResponseSpec => ({
  path: 'page.annotations.list',
  pon,
  value: { annotations: items },
  ...extra,
});
export const effectsOk = (count: number, changedWidgets: unknown[] = []) => ({
  results: Array.from({ length: count }, (_, index) => ({ index, status: 'applied', fields: [], changedWidgets: [] })),
  changedWidgets,
  meta: null,
});

export const ALL = { allows: '*' as const };

export const doc = (responses: ResponseSpec[], over: Partial<FakeDocumentSpec> = {}): FakeDocumentSpec => ({
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
export const close = (id = 'a'): Step => ({ op: 'call', target: 'documents', method: 'close', args: [id] });
export const boot: Step[] = [start, open()];
export const call = (method: string, args: unknown[] = [], doc = 'a'): Step => ({
  op: 'call',
  target: { token: 'form', doc },
  method,
  args,
});
export const ix = (method: string, args: unknown[] = [], doc = 'a'): Step => ({
  op: 'call',
  target: { token: 'interaction', doc },
  method,
  args,
});
export const anno = (method: string, args: unknown[] = [], doc = 'a'): Step => ({
  op: 'call',
  target: { token: 'annotation', doc },
  method,
  args,
});
export const act = (method: string, args: unknown[] = [], doc = 'a'): Step => ({
  op: 'call',
  target: { token: 'actions', doc },
  method,
  args,
});
export const standalone = (method: string, args: unknown[] = [], doc = 'a'): Step => ({
  op: 'call',
  target: { token: 'form-scripting', doc },
  method,
  args,
});
export const probe = (method: string, args: unknown[] = [], doc = 'a'): Step => ({
  op: 'call',
  target: { token: 'form-probe', doc },
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
export const fnSpec = (name: string, answer: AnswerSpec = {}) => ({ $fn: name, ...answer });

/** A pointer sample over page `pon` (content point `x`, `y`). */
export const sample = (phase: string, x: number, y: number, over: Dto = {}, pon = 1) => ({
  phase,
  viewport: { x, y },
  page: { pon, point: { x, y }, scale: 1 },
  modifiers: { shift: false, alt: false, ctrl: false, meta: false },
  ...over,
});
export const pointer = (phase: string, x: number, y: number, over: Dto = {}, pon = 1) => ix('dispatch', [sample(phase, x, y, over, pon)]);
export const tool = (id: string): Step => ix('activateTool', [id]);

export const FORM_ONLY = [{ factory: 'interaction' }, { factory: 'form' }];
export const SNAP = ['form::a'];

// ── action trees ──────────────────────────────────────────────────────────────

type Node = Record<string, unknown>;
export const tree = (root: Node | null, incomplete = false) => ({ root, incomplete, warningFlags: 0, warnings: incomplete ? ['incomplete'] : [] });
export const js = (script: string, next: Node[] = []): Node => ({ type: 'javascript', subtype: 'JavaScript', script, next });
export const resetNode = (fields: unknown[] | null, exclude = false, next: Node[] = []): Node => ({
  type: 'reset-form',
  subtype: 'ResetForm',
  fields,
  exclude,
  next,
});
export const byName = (name: string) => ({ kind: 'name', name });
export const byNumber = (objectNumber: number) => ({ kind: 'objectNumber', objectNumber });
