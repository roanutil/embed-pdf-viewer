/**
 * Shared builders and harness shims for the `packages/plugin/stamp` suites
 * (`stamp-*.ts`). This module writes no vector file; the generator requires
 * every module to export `suites`.
 *
 * `crates/plugins/stamp/src/harness.rs` mirrors each shim:
 *
 *   - A fresh module instance per case. `capability.ts` numbers its ids from
 *     a module-level counter, so one process running every case would carry
 *     the counter from one case into the next and a case's ids would depend
 *     on which cases ran before it. The `stamp` factory therefore builds the
 *     plugin (`stamp.plugin.ts`'s definition, verbatim) over its own import
 *     of `capability.ts?instance=<n>`, whose counter starts at zero. The Rust
 *     counter is thread-local, and the harness resets it per case. The
 *     `stamp-plugin` factory is `stampPlugin()` itself, over the shared
 *     instance, and exactly one case may use it.
 *   - `console.error`. While a stamp factory is current, a call whose message
 *     starts `[stamp]` logs as the callback `console.error` (the Rust port
 *     takes it as `StampHost::console_error`).
 *   - The asset engine. A vector cannot carry an `Engine`, so
 *     `config.assetEngine` names one built over the kernel's (fake) engine:
 *     `'instance'` passes it as is; `'factory'` and `'async-factory'` are
 *     factories that log `assetEngine` and answer it, synchronously or as a
 *     resolved promise; `{throws}` and `{rejects}` are factories that fail.
 *   - Scripting. `config.scripting` is `StampScriptingOptions` with a
 *     scripted sandbox and scripted observers (`form-fixtures.ts`'s
 *     `StandaloneConfig`, plus `enabled`, `onUiEffect`, `onDiagnostic` and
 *     `onError`). `sandbox: null` is a host with no sandbox: the TypeScript
 *     runs with scripting disabled, and the Rust port has no scripting
 *     options to build. The QuickJS default does not port, so `enabled`
 *     without a sandbox spec is refused.
 */
import { definePlugin, type AnyPlugin, type PluginContext } from '@embedpdf/core';
import type { Engine } from '@embedpdf/engine-core/runtime';

import { initialStampState, stampReducer } from '../../../../packages/plugin/stamp/src/reducer';
import { stampPlugin } from '../../../../packages/plugin/stamp/src/stamp.plugin';
import { StampToken } from '../../../../packages/plugin/stamp/src/types';
import type {
  StampAction,
  StampCapability,
  StampConfig,
  StampScriptingOptions,
  StampState,
} from '../../../../packages/plugin/stamp/src/types';
import type { FormScriptingOptions } from '../../../../packages/plugin/form/src/types';

import { makeError, type ErrorSpec, type FakeDocumentSpec, type ResponseSpec } from '../fake-engine';
import { registerPlugin, type PluginEnv, type Step } from '../scenario';
import type { Suite } from '../vectors';
import { scriptedCallback, scriptedSandboxFactory, type AnswerSpec, type SandboxSpec } from './acrojs';
import { layout as pageLayout } from './annotation-fixtures';

export const suites: Suite[] = [];

// ── fresh capability instances ────────────────────────────────────────────────

type CapabilityModule = typeof import('../../../../packages/plugin/stamp/src/capability');

const CAPABILITY = new URL('../../../../packages/plugin/stamp/src/capability.ts', import.meta.url).href;
const POOL_SIZE = 160;
const pool: CapabilityModule[] = await Promise.all(
  Array.from({ length: POOL_SIZE }, (_, i) => import(`${CAPABILITY}?instance=${i}`) as Promise<CapabilityModule>),
);
let nextInstance = 0;
const freshCapability = (): CapabilityModule => {
  const instance = pool[nextInstance++];
  if (!instance) throw new Error(`[stamp suite] more than ${POOL_SIZE} stamp cases: raise POOL_SIZE`);
  return instance;
};

// ── the console.error shim ────────────────────────────────────────────────────

let stampLog: PluginEnv['log'] | null = null;
const previousError = console.error;
console.error = (...args: unknown[]) => {
  if (stampLog && typeof args[0] === 'string' && args[0].startsWith('[stamp]')) {
    stampLog('console.error', args);
    return;
  }
  previousError(...args);
};

// ── config ────────────────────────────────────────────────────────────────────

type Scripted = AnswerSpec & { readonly $fn: string };

export interface StampScriptingSpec {
  readonly enabled: boolean;
  /** The scripted sandbox; `null` is a host with no sandbox at all. */
  readonly sandbox?: SandboxSpec | null;
  readonly identity?: Record<string, unknown> | Scripted;
  readonly fileName?: Scripted;
  readonly now?: Scripted;
  readonly utcOffsetMinutes?: Scripted;
  readonly randomSeed?: Scripted;
  readonly budget?: FormScriptingOptions['budget'];
  readonly onUiEffect?: Scripted;
  readonly onDiagnostic?: Scripted;
  readonly onError?: Scripted;
}

export type AssetEngineSpec =
  | 'instance'
  | 'factory'
  | 'async-factory'
  | { readonly throws: ErrorSpec }
  | { readonly rejects: ErrorSpec };

export interface StampSpec {
  readonly assetEngine?: AssetEngineSpec;
  readonly previewWidth?: number;
  readonly scripting?: StampScriptingSpec;
}

function assetEngineOf(spec: AssetEngineSpec, engine: Engine, env: PluginEnv): StampConfig['assetEngine'] {
  if (spec === 'instance') return engine;
  return () => {
    env.log('assetEngine', []);
    if (spec === 'factory') return engine;
    if (spec === 'async-factory') return Promise.resolve(engine);
    if ('throws' in spec) throw makeError(spec.throws);
    return Promise.reject(makeError(spec.rejects));
  };
}

function scriptingOf(spec: StampScriptingSpec, env: PluginEnv): StampScriptingOptions {
  if (spec.enabled && spec.sandbox === undefined) {
    throw new Error('[stamp suite] scripting.enabled needs a scripted sandbox: the QuickJS default does not port');
  }
  const fn = <T>(s: Scripted | undefined) =>
    s === undefined ? undefined : (scriptedCallback(s.$fn, s, env) as (...args: unknown[]) => T);
  const identity = spec.identity;
  return {
    enabled: spec.enabled && spec.sandbox !== null,
    ...(spec.sandbox ? { sandboxFactory: scriptedSandboxFactory(spec.sandbox, env) } : {}),
    ...(identity === undefined
      ? {}
      : { identity: '$fn' in identity ? fn<Record<string, string>>(identity as Scripted) : identity }),
    ...(spec.fileName ? { fileName: fn<string>(spec.fileName) } : {}),
    ...(spec.now ? { now: fn<number>(spec.now) } : {}),
    ...(spec.utcOffsetMinutes ? { utcOffsetMinutes: fn<number>(spec.utcOffsetMinutes) } : {}),
    ...(spec.randomSeed ? { randomSeed: fn<number>(spec.randomSeed) } : {}),
    ...(spec.budget ? { budget: spec.budget } : {}),
    ...(spec.onUiEffect ? { onUiEffect: fn<void>(spec.onUiEffect) } : {}),
    ...(spec.onDiagnostic ? { onDiagnostic: fn<void>(spec.onDiagnostic) } : {}),
    ...(spec.onError ? { onError: fn<void>(spec.onError) } : {}),
  } as StampScriptingOptions;
}

function configOf(spec: StampSpec, engine: Engine, env: PluginEnv): StampConfig {
  return {
    ...(spec.assetEngine !== undefined ? { assetEngine: assetEngineOf(spec.assetEngine, engine, env) } : {}),
    ...(spec.previewWidth !== undefined ? { previewWidth: spec.previewWidth } : {}),
    ...(spec.scripting !== undefined ? { scripting: scriptingOf(spec.scripting, env) } : {}),
  };
}

registerPlugin('stamp', {
  factory: (config, env) => {
    stampLog = env.log;
    const instance = freshCapability();
    const spec = (config ?? {}) as StampSpec;
    // `stamp.plugin.ts`, over this case's own capability module.
    return definePlugin<StampState, StampAction, StampCapability>({
      id: 'stamp',
      token: StampToken,
      scope: 'workspace',
      initialState: initialStampState,
      reduce: stampReducer,
      capability: (ctx: PluginContext<StampState, StampAction>) =>
        instance.createStampCapability(ctx, configOf(spec, ctx.engine, env)),
    }) as AnyPlugin;
  },
  tokens: [StampToken],
});

let sharedInstanceUsed = false;
registerPlugin('stamp-plugin', {
  factory: (_config, env) => {
    if (sharedInstanceUsed) throw new Error('[stamp suite] only one case may use the shared stamp-plugin instance');
    sharedInstanceUsed = true;
    stampLog = env.log;
    return stampPlugin() as AnyPlugin;
  },
  tokens: [StampToken],
});

// ── bytes ─────────────────────────────────────────────────────────────────────

export const bytes = (data: Uint8Array | string): { $bytes: string } => ({
  $bytes: Buffer.from(typeof data === 'string' ? new TextEncoder().encode(data) : data).toString('base64'),
});
export const PDF = bytes('%PDF-1.7\n%stamp fixture\n');
export const pdf = (tag: string) => bytes(`%PDF-${tag}`);
/** A PNG signature and IHDR with the given size. */
export const png = (width = 100, height = 50) => {
  const b = new Uint8Array(32);
  b.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], 0);
  const view = new DataView(b.buffer);
  view.setUint32(8, 13);
  b.set([0x49, 0x48, 0x44, 0x52], 12);
  view.setUint32(16, width);
  view.setUint32(20, height);
  return bytes(b);
};
/** SOI, an APP0 segment, then SOF0 with the given size. */
export const jpeg = (width = 64, height = 32) =>
  bytes(
    new Uint8Array([
      0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00, 0xff, 0xc0, 0x00, 0x0b, 0x08,
      (height >> 8) & 0xff, height & 0xff, (width >> 8) & 0xff, width & 0xff, 0x01, 0x01, 0x11, 0x00,
    ]),
  );
export const GARBAGE = bytes('GIF89a-not-supported');

// ── PieceInfo ─────────────────────────────────────────────────────────────────

type Entries = Record<string, unknown>;
export const str = (value: string) => ({ type: 'string', value });
export const nm = (value: string) => ({ type: 'name', value });
export const arr = (...value: string[]) => ({ type: 'string-array', value });
export const numEntry = (value: number) => ({ type: 'number', value });
export const pieceInfo = (entries: Entries) => ({ entries, lastModified: null });

// ── asset documents ───────────────────────────────────────────────────────────

/** A page of an asset PDF: `size` is its crop box. */
export const assetPage = (index: number, pon: number, width = 200 + index, height = 100 + index) =>
  pageLayout(index, pon, { left: 0, bottom: 0, right: width, top: height });

export const image = (pon: number, contentType: string | undefined = 'image/png') => ({
  ...(contentType === undefined ? {} : { contentType }),
  source: { kind: 'bytes', bytes: bytes(`png-${pon}`) },
});

export interface AssetDocOptions {
  /** Catalog `/PieceInfo` for `EMBD_StampLibrary`; absent reads `null`. */
  readonly catalog?: Entries;
  /** Page `/PieceInfo` for `EMBD_Stamp`, by object number. */
  readonly page?: Record<number, Entries>;
  /** Tried before the defaults. */
  readonly responses?: readonly ResponseSpec[];
  readonly services?: readonly string[];
  readonly lacks?: readonly string[];
  /** The page registry, when it differs from the listed pages. */
  readonly registry?: readonly unknown[];
  readonly download?: string;
}

/** A library PDF the asset engine opens: pieceInfo on the catalog and every
 *  page, and answers for every call an import makes. */
export const assetDoc = (pages: readonly ReturnType<typeof assetPage>[], o: AssetDocOptions = {}): FakeDocumentSpec => ({
  services: o.services ?? ['pieceInfo', 'page.pieceInfo'],
  ...(o.lacks ? { lacks: o.lacks } : {}),
  pages: o.registry ?? pages,
  responses: [
    ...(o.responses ?? []),
    { path: 'pages.list', value: { pageCount: pages.length, pages } },
    { path: 'pieceInfo.read', value: o.catalog ? pieceInfo(o.catalog) : null },
    ...pages.map((p) => ({
      path: 'page.pieceInfo.read',
      pon: p.pageObjectNumber,
      value: o.page?.[p.pageObjectNumber] ? pieceInfo(o.page[p.pageObjectNumber]) : null,
    })),
    { path: 'pieceInfo.update' },
    { path: 'page.pieceInfo.update' },
    ...pages.map((p) => ({ path: 'pages.extract', args: [[p.pageObjectNumber]], value: pdf(`page-${p.pageObjectNumber}`) })),
    ...pages.map((p) => ({ path: 'page.render.image', pon: p.pageObjectNumber, value: image(p.pageObjectNumber) })),
    { path: 'download', value: pdf(o.download ?? 'canonical') },
  ],
});

/** Engine-level `open` answers, consumed in order: each hands out that
 *  document. The kernel's own opens take their turn in the same list. */
export const opens = (...keys: string[]): ResponseSpec[] =>
  keys.map((key) => ({ path: 'open', times: 1, value: { $document: key } }));

// ── steps ─────────────────────────────────────────────────────────────────────

export const start: Step = { op: 'call', target: 'kernel', method: 'start' };
export const destroy: Step = { op: 'call', target: 'kernel', method: 'destroy' };
export const openDoc = (id = 'a'): Step => ({
  op: 'call',
  target: 'documents',
  method: 'open',
  args: [{ kind: 'bytes', id, bytes: { $bytes: '' } }],
});
export const stamp = (method: string, args: unknown[] = []): Step => ({
  op: 'call',
  target: { token: 'stamp' },
  method,
  args,
});
export const settle = (label: string, value?: unknown): Step =>
  value === undefined ? { op: 'settle', label } : { op: 'settle', label, value };
export const fail = (label: string, message = 'refused', code = 'Unknown'): Step => ({
  op: 'settle',
  label,
  error: { name: 'EngineError', message, code },
});
export const advance = (ms: number): Step => ({ op: 'advance', ms });

export const engineError = (code: string, message: string): ErrorSpec => ({ name: 'EngineError', message, code });

/** The stamp plugin alone, with a config. */
export const STAMP = (config?: StampSpec) => [config === undefined ? { factory: 'stamp' } : { factory: 'stamp', config }];
/** The placement registry: interaction, annotation and form beside stamp. */
export const PLACEMENT = (config?: StampSpec) => [
  { factory: 'interaction' },
  { factory: 'annotation' },
  { factory: 'form' },
  ...STAMP(config),
];
