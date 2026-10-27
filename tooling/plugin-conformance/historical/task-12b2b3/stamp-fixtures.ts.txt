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
 *   - `console.error` and `console.warn`. While a stamp factory is current,
 *     a call whose message starts `[stamp]` logs as the callback of that name
 *     (the Rust port takes them as `StampHost::console_error` and
 *     `console_warn`).
 *   - `Math.random`, which `customStampName` draws a fresh identifier from.
 *     Once a stamp factory has built, the nth call of a case answers
 *     `((n % 62) + 0.5) / 62`, so the nth character is `BASE62[n % 62]`; the
 *     Rust port takes the source as `StampHost::random`.
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
import { register } from 'node:module';

import { definePlugin, type AnyPlugin } from '@embedpdf/core';
import type { Engine } from '@embedpdf/engine-core/runtime';

import {
  memoryStampStore,
  persistStampLibraries,
  restoreStampLibraries,
} from '../../../../packages/plugin/stamp/src/persistence';
import { stampPlugin } from '../../../../packages/plugin/stamp/src/stamp.plugin';
import { StampToken } from '../../../../packages/plugin/stamp/src/contract';
import type { StampConfig } from '../../../../packages/plugin/stamp/src/contract';
import { ActionsToken as ActionsHostToken } from '../../../../packages/plugin/actions/src/host-contract';

import { makeError, type ErrorSpec, type FakeDocumentSpec, type ResponseSpec } from '../fake-engine';
import { onCaseEnd, registerPlugin, type PluginEnv, type Step } from '../scenario';
import type { Suite } from '../vectors';
import { scriptedCallback, type AnswerSpec, type SandboxSpec } from './acrojs';
import './actions';
import { layout as pageLayout } from './annotation-fixtures';

export const suites: Suite[] = [];

// ── fresh capability instances ────────────────────────────────────────────────

// The id counter lives in `services/asset-engine.ts`, which the controller
// reaches through several imports. A resolve hook carries the instance query
// from the controller down the whole stamp source tree, so each instance is
// a private copy of it, counter included.
register(new URL('../loader/instance-hook.mjs', import.meta.url));

type ControllerModule = typeof import('../../../../packages/plugin/stamp/src/controller');

const CONTROLLER = new URL('../../../../packages/plugin/stamp/src/controller.ts', import.meta.url).href;
const POOL_SIZE = 160;
const pool: ControllerModule[] = await Promise.all(
  Array.from({ length: POOL_SIZE }, (_, i) => import(`${CONTROLLER}?stamp-instance=${i}`) as Promise<ControllerModule>),
);
let nextInstance = 0;
const freshCapability = (): ControllerModule => {
  const instance = pool[nextInstance++];
  if (!instance) throw new Error(`[stamp suite] more than ${POOL_SIZE} stamp cases: raise POOL_SIZE`);
  return instance;
};

// ── the console.error shim ────────────────────────────────────────────────────

let stampLog: PluginEnv['log'] | null = null;
onCaseEnd(() => {
  stampLog = null;
});
const previousError = console.error;
console.error = (...args: unknown[]) => {
  if (stampLog && typeof args[0] === 'string' && args[0].startsWith('[stamp]')) {
    stampLog('console.error', args);
    return;
  }
  previousError(...args);
};
const previousWarn = console.warn;
console.warn = (...args: unknown[]) => {
  if (stampLog && typeof args[0] === 'string' && args[0].startsWith('[stamp]')) {
    stampLog('console.warn', args);
    return;
  }
  previousWarn(...args);
};

// ── the Math.random shim ──────────────────────────────────────────────────────

let stampRandomCalls: number | null = null;
const previousRandom = Math.random;
Math.random = () => {
  if (stampRandomCalls === null) return previousRandom();
  const n = stampRandomCalls++;
  return ((n % 62) + 0.5) / 62;
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
  readonly budget?: unknown;
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
  readonly dynamic?: boolean;
  /** v2's `config.scripting`. v3 evaluates dynamic stamps in a realm the
   *  target document's actions plugin mints, so this now configures that
   *  plugin (see PLACEMENT), and the observers subscribe to its hooks. */
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

function configOf(spec: StampSpec, engine: Engine, env: PluginEnv): StampConfig {
  return {
    ...(spec.assetEngine !== undefined ? { assetEngine: assetEngineOf(spec.assetEngine, engine, env) } : {}),
    ...(spec.previewWidth !== undefined ? { previewWidth: spec.previewWidth } : {}),
    ...(spec.dynamic !== undefined ? { dynamic: spec.dynamic } : {}),
  };
}

/** v2's scripting observers, over the v3 doors a detached realm's results
 *  reach: alerts through the actions UI adapter (the only UI effect a
 *  detached realm surfaces), diagnostics and errors through its script hooks. */
interface ObserverSpec {
  readonly onUiEffect?: Scripted;
  readonly onDiagnostic?: Scripted;
  readonly onError?: Scripted;
}
registerPlugin('stamp-observers', {
  factory: (config, env) => {
    const spec = (config ?? {}) as ObserverSpec;
    const fn = (s: Scripted) => scriptedCallback(s.$fn, s, env) as (...args: unknown[]) => void;
    return definePlugin({
      id: 'stamp-observers',
      scope: 'document',
      requires: [ActionsHostToken],
      effects: (ctx) => {
        const actions = ctx.get(ActionsHostToken);
        if (spec.onUiEffect) {
          const onUiEffect = fn(spec.onUiEffect);
          ctx.cleanup(
            actions.setUiAdapter({
              openUri: () => {},
              print: () => {},
              alert: (message, opts) => onUiEffect({ kind: 'alert', message, ...opts }),
            }),
          );
        }
        if (spec.onDiagnostic) ctx.cleanup(actions.onScriptDiagnostic(fn(spec.onDiagnostic)));
        if (spec.onError) ctx.cleanup(actions.onScriptError(fn(spec.onError)));
      },
    }) as AnyPlugin;
  },
});

/** The actions plugin carrying a v2 scripting spec as its javascript config. */
export const scriptingActions = (spec: StampScriptingSpec) => {
  const { onUiEffect, onDiagnostic, onError, ...javascript } = spec;
  const observers = { onUiEffect, onDiagnostic, onError };
  const any = Object.values(observers).some((v) => v !== undefined);
  return [
    { factory: 'actions', config: { openSequence: 'off', javascript } },
    ...(any ? [{ factory: 'stamp-observers', config: JSON.parse(JSON.stringify(observers)) }] : []),
  ];
};

registerPlugin('stamp', {
  factory: (config, env) => {
    stampLog = env.log;
    stampRandomCalls = 0;
    const instance = freshCapability();
    const spec = (config ?? {}) as StampSpec;
    // `stamp.plugin.ts` itself, with only `create` swapped to run over this
    // case's own controller module: id, token, scope, dependencies and the
    // reducer stay the shipping definition's.
    const shipped = stampPlugin();
    return {
      ...shipped,
      create: (ctx) => ({ api: instance.createStampController(ctx as never, configOf(spec, ctx.engine, env)) }),
    } satisfies typeof shipped as AnyPlugin;
  },
  tokens: [StampToken],
});

let sharedInstanceUsed = false;
registerPlugin('stamp-plugin', {
  factory: (_config, env) => {
    if (sharedInstanceUsed) throw new Error('[stamp suite] only one case may use the shared stamp-plugin instance');
    sharedInstanceUsed = true;
    stampLog = env.log;
    stampRandomCalls = 0;
    return stampPlugin() as AnyPlugin;
  },
  tokens: [StampToken],
});

// ── the persistence driver ────────────────────────────────────────────────────

/** `stamp-persistence`: `persistence.ts` over a memory store. `seed` fills
 *  the store, `rows` lists it, `restore` and `persist` run the helpers
 *  against the stamp capability, `stop` calls what `persist` returned.
 *  `config.fail` names store calls (`list`, `put`, `delete`) that reject. */
registerPlugin('stamp-persistence', {
  factory: (config, env) => {
    const fail = ((config ?? {}) as { fail?: Record<string, string> }).fail ?? {};
    return definePlugin({
      id: 'stamp-persistence',
      token: env.token('stamp-persistence'),
      scope: 'workspace',
      requires: [StampToken],
      capability: (ctx) => {
        const memory = memoryStampStore();
        const reject = (call: string) => Promise.reject(new Error(fail[call]));
        const store = {
          list: () => (fail.list ? reject('list') : memory.list()),
          put: (id: string, bytes: Uint8Array) => (fail.put ? reject('put') : memory.put(id, bytes)),
          delete: (id: string) => (fail.delete ? reject('delete') : memory.delete(id)),
        };
        let stop: (() => void) | null = null;
        return {
          seed: (rows: { id: string; bytes: Uint8Array }[]) =>
            Promise.all(rows.map((row) => memory.put(row.id, row.bytes))).then(() => undefined),
          rows: () => memory.list(),
          restore: () => restoreStampLibraries(ctx.get(StampToken), store),
          persist: (opts?: { except?: string[]; debounceMs?: number }) => {
            stop = persistStampLibraries(ctx.get(StampToken), store, opts);
          },
          stop: () => stop?.(),
        };
      },
    }) as AnyPlugin;
  },
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
  /** The document's `/Title`; absent reads null. */
  readonly title?: string;
  /** The `/Names /Pages` registry `pages.list` reports; absent is none. */
  readonly namedPages?: readonly unknown[];
}

const META = (title: string | null) => ({
  title,
  author: null,
  subject: null,
  keywords: null,
  producer: null,
  creator: null,
  created: null,
  modified: null,
  trapped: 'unknown',
  custom: {},
});

/** A library PDF the asset engine opens: pieceInfo on the catalog and every
 *  page, and answers for every call an import makes. */
export const assetDoc = (pages: readonly ReturnType<typeof assetPage>[], o: AssetDocOptions = {}): FakeDocumentSpec => ({
  services: o.services ?? ['pieceInfo', 'page.pieceInfo'],
  ...(o.lacks ? { lacks: o.lacks } : {}),
  pages: o.registry ?? pages,
  responses: [
    ...(o.responses ?? []),
    { path: 'pages.list', value: { pageCount: pages.length, pages, namedPages: o.namedPages ?? [] } },
    { path: 'metadata.read', value: META(o.title ?? null) },
    { path: 'metadata.update' },
    { path: 'pages.setName' },
    { path: 'pages.removeName' },
    { path: 'pieceInfo.read', value: o.catalog ? pieceInfo(o.catalog) : null },
    ...pages.map((p) => ({
      path: 'page.pieceInfo.read',
      pon: p.ref.pageObjectNumber,
      value: o.page?.[p.ref.pageObjectNumber] ? pieceInfo(o.page[p.ref.pageObjectNumber]) : null,
    })),
    { path: 'pieceInfo.update' },
    { path: 'page.pieceInfo.update' },
    ...pages.map((p) => ({ path: 'pages.extract', args: [[{ kind: 'objectNumber', pageObjectNumber: p.ref.pageObjectNumber }]], value: pdf(`page-${p.ref.pageObjectNumber}`) })),
    ...pages.map((p) => ({ path: 'page.render.image', pon: p.ref.pageObjectNumber, value: image(p.ref.pageObjectNumber) })),
    { path: 'download', value: pdf(o.download ?? 'canonical') },
  ],
});

/** The page a library edit inserts (a raster's blank page, a PDF's page). */
export const WORKSHOP_PAGE = 900;

/**
 * The document every library edit opens when the case does not name one:
 * the blank seed of a new library, and the canonical copy an append reopens.
 * It answers each library-editing call the same way every time: its registry
 * is the seed's unregistered blank (pon 3) and the page an insert adds
 * (WORKSHOP_PAGE, 300 x 120), which is what the insert answers.
 */
export const workshopDoc = (): FakeDocumentSpec => {
  const pages = [assetPage(0, 3, 612, 792), assetPage(1, WORKSHOP_PAGE, 300, 120)];
  const added = { kind: 'objectNumber', pageObjectNumber: WORKSHOP_PAGE };
  const layout = { pageCount: pages.length, pages, namedPages: [] };
  return assetDoc(pages, {
    responses: [
      { path: 'pages.insert', value: { insertedPages: [added], layout, cache: null } },
      { path: 'pages.insertBlank', value: { insertedPages: [added], layout, cache: null } },
      { path: 'page.annotations.create', value: { created: null } },
      { path: 'pages.flatten', value: { results: [{ page: added, status: 'applied' }], pages: [added], cache: null } },
      { path: 'pages.delete', value: { layout, cache: null } },
      { path: 'forms.list', value: { formKind: 'none', needsAppearances: false, fields: [], calculationOrder: [] } },
    ],
    download: 'workshop',
  });
};

/** A case's engine with the workshop document as the open fallback. */
export const withWorkshop = <E extends { documents?: Record<string, FakeDocumentSpec> }>(engine?: E) => ({
  ...(engine ?? {}),
  documents: { ...(engine?.documents ?? {}), workshop: workshopDoc() },
  defaultDocument: 'workshop',
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
/** v2 stamp method → v3 name. `libraries`/`assets` were list reads;
 *  `libraryBytes` is the async `exportLibrary`. */
const RENAMED: Readonly<Record<string, string>> = {
  libraries: 'listLibraries',
  library: 'getLibrary',
  asset: 'getAsset',
  assetPreview: 'getAssetPreview',
  assetBytes: 'readAssetBytes',
  libraryBytes: 'exportLibrary',
  importLibraryPdf: 'importLibrary',
  addAsset: 'createAsset',
  removeAsset: 'deleteAsset',
  removeLibrary: 'deleteLibrary',
};
export const stamp = (method: string, args: unknown[] = []): Step => ({
  op: 'call',
  target: { token: 'stamp' },
  method: method === 'assets' ? 'listAssets' : (RENAMED[method] ?? method),
  args: method === 'assets' && typeof args[0] === 'string' ? [{ libraryId: args[0] }] : args,
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
export const PLACEMENT = (config?: StampSpec) => {
  const { scripting, ...rest } = config ?? {};
  return [
    { factory: 'interaction' },
    { factory: 'annotation' },
    { factory: 'form' },
    ...(scripting ? scriptingActions(scripting) : []),
    ...STAMP(config === undefined ? undefined : rest),
  ];
};
