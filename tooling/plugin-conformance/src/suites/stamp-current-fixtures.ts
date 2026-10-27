/** Current stamp registration and typed host inputs. Binary resources stay outside session state. */
import { withStampModuleIsolation } from '../stamp-module-host';
import { definePlugin } from '@embedpdf/core';
import {
  AbortablePromise,
  isLocalDocument,
  type Engine,
  type LocalEngine,
  type LocalDocumentHandle,
  type DocumentMetadata,
  type PageLayout,
  type PageImageResult,
  type PieceInfoEntry,
  type PieceInfoSnapshot,
  type MutationMeta,
  type PieceInfoPatch,
} from '@embedpdf/engine-core/runtime';
import { LOCAL_ENGINE_BRAND } from '@embedpdf/core/testing';
import { stampPlugin } from '../../../../packages/plugin/stamp/src/stamp.plugin';
import {
  StampToken,
  type StampCapability,
  type StampConfig,
  type StampAssetEngine,
  type ImportLibraryOptions,
} from '../../../../packages/plugin/stamp/src/contract';
import {
  stampLibraryPieceInfo,
  stampPieceInfo,
} from '../../../../packages/plugin/stamp/src/convention';
import {
  memoryStampStore,
  persistStampLibraries,
  restoreStampLibraries,
  type StampLibraryStore,
} from '../../../../packages/plugin/stamp/src/persistence';
import { canonical } from '../canonical';
import { installCaseEntropy, restoreCaseEntropy } from '../case-entropy';
import {
  makeError,
  type ErrorSpec,
  type FakeDocumentSpec,
  type ResponseSpec,
} from '../fake-engine';
import { registerPlugin, onCaseEnd, type PluginEnv, type Step, type PluginSpec } from '../scenario';
import type { Suite } from '../vectors';
import { layout } from './annotation-live-fixtures';
import './form-script-fixtures';
export const suites: Suite[] = [];

// Each controller owns a private copy of the shipping module-level uid counter.
type ControllerModule = typeof import('../../../../packages/plugin/stamp/src/controller');
const controller = new URL('../../../../packages/plugin/stamp/src/controller.ts', import.meta.url)
  .href;
const controllers: ControllerModule[] = await withStampModuleIsolation(async () => {
  const settled = await Promise.allSettled(
    Array.from(
      { length: 256 },
      (_, index) => import(`${controller}?stamp-instance=current-${index}`),
    ),
  );
  const modules: ControllerModule[] = [];
  for (const result of settled) {
    if (result.status === 'rejected') throw result.reason;
    modules.push(result.value);
  }
  return modules;
});
let nextController = 0;

/** A typed local port over the table engine. Local handles must already carry the engine's brand. */
export function localAssetEngine(engine: Engine): LocalEngine {
  const unused = (): never => {
    throw new Error('[stamp-current] unused local engine port');
  };
  const local: LocalEngine = {
    get canOpenFiles() {
      return engine.canOpenFiles;
    },
    open: (input, options) =>
      AbortablePromise.run(async (signal) => {
        const doc = await engine.open(input, options).abortWith(signal);
        if (!isLocalDocument(doc)) throw new Error('[stamp-current] asset document must be local');
        return doc;
      }),
    destroy: () => engine.destroy(),
    warmup: unused,
    fonts: {
      register: unused,
      registerAll: unused,
      addFallback: unused,
      clearFallbacks: unused,
      clear: unused,
      authorizeEditing: unused,
      list: unused,
    },
  };
  Object.defineProperty(local, LOCAL_ENGINE_BRAND, { value: true });
  return local;
}

export type AssetEngineInput =
  | 'instance'
  | 'factory'
  | 'async-factory'
  | { readonly throws: ErrorSpec }
  | { readonly rejects: ErrorSpec };
export interface StampCurrentInput {
  readonly assetEngine?: AssetEngineInput | null;
  readonly previewWidth?: number;
  readonly dynamic?: boolean;
}
let release: (() => void) | null = null;
export function restoreStampHost(): void {
  const cleanup = release;
  release = null;
  cleanup?.();
}
onCaseEnd(restoreStampHost);
function installStampHost(env: PluginEnv): void {
  if (release) throw new Error('[stamp-current] install once per case');
  const descriptors = {
    random: Object.getOwnPropertyDescriptor(Math, 'random')!,
    error: Object.getOwnPropertyDescriptor(console, 'error')!,
    warn: Object.getOwnPropertyDescriptor(console, 'warn')!,
  };
  const previous = { random: Math.random, error: console.error, warn: console.warn };
  installCaseEntropy();
  let randomIndex = 0;
  const random = () => ((randomIndex++ % 62) + 0.5) / 62;
  const error = (...args: unknown[]) =>
    typeof args[0] === 'string' && args[0].startsWith('[stamp]')
      ? env.log('console.error', args)
      : previous.error(...args);
  const warn = (...args: unknown[]) =>
    typeof args[0] === 'string' && args[0].startsWith('[stamp]')
      ? env.log('console.warn', args)
      : previous.warn(...args);
  release = () => {
    Object.defineProperty(Math, 'random', descriptors.random);
    Object.defineProperty(console, 'error', descriptors.error);
    Object.defineProperty(console, 'warn', descriptors.warn);
    restoreCaseEntropy();
  };
  try {
    Object.defineProperty(Math, 'random', { configurable: true, writable: true, value: random });
    Object.defineProperty(console, 'error', { configurable: true, writable: true, value: error });
    Object.defineProperty(console, 'warn', { configurable: true, writable: true, value: warn });
  } catch (failure) {
    restoreStampHost();
    throw failure;
  }
}
function assetEngine(input: AssetEngineInput, engine: Engine, env: PluginEnv): StampAssetEngine {
  const local = localAssetEngine(engine);
  if (input === 'instance') return local;
  return () => {
    env.log('assetEngine', []);
    if (input === 'factory') return local;
    if (input === 'async-factory') return Promise.resolve(local);
    if ('throws' in input) throw makeError(input.throws);
    return Promise.reject(makeError(input.rejects));
  };
}
registerPlugin('stamp-current', {
  factory: (config, env) => {
    const input = (config ?? {}) as StampCurrentInput;
    const module = controllers[nextController++];
    if (!module)
      throw new Error('[stamp-current] isolated controller pool exhausted (256 cases per process)');
    installStampHost(env);
    let bound: Engine | null = null;
    const kernelEngine = (): Engine => {
      if (!bound) throw new Error('[stamp-current] engine used before create');
      return bound;
    };
    const forwarding: Engine = {
      get canOpenFiles() {
        return kernelEngine().canOpenFiles;
      },
      open: (input, options) => kernelEngine().open(input, options),
      destroy: () => kernelEngine().destroy(),
    };
    const registered: StampConfig = {
      ...(input.assetEngine
        ? { assetEngine: assetEngine(input.assetEngine, forwarding, env) }
        : {}),
      ...(input.previewWidth !== undefined ? { previewWidth: input.previewWidth } : {}),
      ...(input.dynamic !== undefined ? { dynamic: input.dynamic } : {}),
    };
    const shipped = stampPlugin(registered);
    return {
      ...shipped,
      create: (ctx) => {
        bound = ctx.engine;
        return module.createStampController(ctx);
      },
    };
  },
  tokens: [StampToken],
});

/** Stamp installs outermost; form diagnostics then annotation install inside it and reset before it. */
export const STAMP = (
  config: StampCurrentInput = { assetEngine: 'instance' },
  siblings: readonly PluginSpec[] = [],
): PluginSpec[] => [{ factory: 'stamp-current', config }, ...siblings];
export const start: Step = { op: 'call', target: 'kernel', method: 'start' };
export const destroy: Step = { op: 'call', target: 'kernel', method: 'destroy' };
export const advance = (ms: number): Step => ({ op: 'advance', ms });
export const settle = (label: string, value?: unknown): Step =>
  value === undefined ? { op: 'settle', label } : { op: 'settle', label, value: canonical(value) };
export function stamp<Name extends keyof StampCapability>(
  method: Name,
  args: StampCapability[Name] extends (...args: infer Args) => unknown ? Args : never,
): Step;
export function stamp<Name extends keyof StampCapability>(method: Name): Step;
export function stamp(method: keyof StampCapability, args: readonly unknown[] = []): Step {
  return {
    op: 'call',
    target: { token: 'stamp' },
    method,
    args: args.map((arg) => canonical(arg)),
  };
}
/** Deliberately malformed current contract inputs, kept separate from typed successful calls. */
export const malformedStamp = (method: keyof StampCapability, args: readonly unknown[]): Step => ({
  op: 'call',
  target: { token: 'stamp' },
  method,
  args,
});
/** A serializable current callback port; every answer is checked against assetName's return type. */
export function importWithAssetName(
  source: Uint8Array,
  options: Omit<ImportLibraryOptions, 'assetName'> & {
    readonly assetName: {
      readonly $fn: 'assetName';
      readonly seq: readonly {
        readonly returns?: ReturnType<NonNullable<ImportLibraryOptions['assetName']>>;
      }[];
    };
  },
): Step {
  return {
    op: 'call',
    target: { token: 'stamp' },
    method: 'importLibrary',
    args: [canonical(source), canonical(options)],
  };
}
export const bytes = (text: string): Uint8Array => new TextEncoder().encode(text);
export const PDF = bytes('%PDF-1.7\n%stamp fixture\n');
export const pdf = (tag: string): Uint8Array => bytes(`%PDF-${tag}`);
export const GARBAGE = bytes('GIF89a-not-supported');
export const png = (width = 100, height = 50): Uint8Array => {
  const result = new Uint8Array(32);
  result.set([137, 80, 78, 71, 13, 10, 26, 10]);
  const view = new DataView(result.buffer);
  view.setUint32(8, 13);
  result.set([73, 72, 68, 82], 12);
  view.setUint32(16, width);
  view.setUint32(20, height);
  return result;
};
export const jpeg = (): Uint8Array =>
  new Uint8Array([255, 216, 255, 224, 0, 4, 0, 0, 255, 192, 0, 11, 8, 0, 32, 0, 64, 1, 1, 17, 0]);
export const str = (value: string): PieceInfoEntry => ({ type: 'string', value });
export const nm = (value: string): PieceInfoEntry => ({ type: 'name', value });
export const arr = (...value: string[]): PieceInfoEntry => ({ type: 'string-array', value });
export const numEntry = (value: number): PieceInfoEntry => ({ type: 'number', value });
export const pieceInfo = (entries: Record<string, PieceInfoEntry>): PieceInfoSnapshot => ({
  entries,
  modifiedAt: null,
});
export const assetPage = (
  index: number,
  objectNumber: number,
  width = 200 + index,
  height = 100 + index,
): PageLayout => layout(index, objectNumber, { left: 0, bottom: 0, right: width, top: height });
export const image = (objectNumber: number): PageImageResult => ({
  width: 256,
  height: 128,
  format: 'png',
  contentType: 'image/png',
  source: { kind: 'bytes', bytes: bytes(`png-${objectNumber}`) },
});
export const metadata = (title: string | null): DocumentMetadata => ({
  title,
  author: null,
  subject: null,
  keywords: null,
  producer: null,
  creator: null,
  createdAt: null,
  modifiedAt: null,
  trapped: 'unknown',
});
export const mutationMeta = (pages: PageLayout[] = []): MutationMeta => ({
  affectedPages: pages.map((page) => page.ref),
  cacheDelta: null,
  opId: 'asset-engine-write',
  undoable: true,
});
export interface AssetDocumentInput {
  readonly catalog?: Record<string, PieceInfoEntry>;
  readonly page?: Readonly<Record<number, Record<string, PieceInfoEntry>>>;
  readonly title?: string | null;
  readonly titleAfterWrite?: string;
  readonly metadataTitles?: readonly string[];
  readonly catalogWrites?: readonly {
    readonly id: string;
    readonly kind?: string;
    readonly categories?: readonly string[];
    readonly locale?: string;
  }[];
  readonly pageWrites?: Readonly<
    Record<
      number,
      {
        readonly kind: 'stamp' | 'signature' | 'initials';
        readonly subject?: string;
        readonly categories?: readonly string[];
      }
    >
  >;
  readonly pageNames?: readonly string[];
  readonly namedPages?: Awaited<ReturnType<LocalDocumentHandle['pages']['list']>>['namedPages'];
  readonly registry?: PageLayout[];
  readonly lacks?: readonly string[];
  readonly responses?: readonly ResponseSpec[];
  readonly download?: string;
}
/** Prepare the complete merge-write answer from explicitly authored initial entries and a static patch. */
function writtenPieceInfo(
  patch: PieceInfoPatch,
  initial: Record<string, PieceInfoEntry> = {},
): PieceInfoSnapshot {
  const entries: Record<string, PieceInfoEntry> = { ...initial };
  for (const [key, value] of Object.entries(patch)) {
    if (value === null) {
      delete entries[key];
      continue;
    }
    if (typeof value === 'string') entries[key] = { type: 'string', value };
    else if (typeof value === 'number') entries[key] = { type: 'number', value };
    else if (typeof value === 'boolean') entries[key] = { type: 'boolean', value };
    else if ('name' in value) entries[key] = { type: 'name', value: value.name };
    else entries[key] = { type: 'string-array', value: [...value] };
  }
  return pieceInfo(entries);
}
export function assetDoc(pages: PageLayout[], input: AssetDocumentInput = {}): FakeDocumentSpec {
  const info = metadata(input.title ?? null);
  const catalog = input.catalog ? pieceInfo(input.catalog) : null;
  const currentLayout: Awaited<ReturnType<LocalDocumentHandle['pages']['list']>> = {
    pageCount: pages.length,
    pages,
    namedPages: input.namedPages ?? [],
  };
  const fixedNames = input.pageNames ?? [
    'Stamp1=Stamp1',
    'Stamp2=Stamp2',
    'Stamp3=Stamp3',
    'Stamp4=Stamp4',
  ];
  const nameWrites = pages.map((page, index) => {
    const name = fixedNames[index];
    if (name === undefined) throw new Error('[stamp-current] author a page name for each page');
    return { name, page: page.ref } satisfies Parameters<
      LocalDocumentHandle['pages']['setName']
    >[0];
  });
  return {
    local: true,
    pages: input.registry ?? pages,
    ...(input.lacks ? { lacks: input.lacks } : {}),
    responses: [
      ...(input.responses ?? []),
      { path: 'pages.list', value: currentLayout },
      { path: 'metadata.get', value: info },
      ...(input.metadataTitles ?? [input.titleAfterWrite ?? 'Stamps']).map(
        (title): ResponseSpec => ({
          path: 'metadata.update',
          args: [{ title }],
          value: { metadata: metadata(title), meta: mutationMeta() } satisfies Awaited<
            ReturnType<LocalDocumentHandle['metadata']['update']>
          >,
        }),
      ),
      ...nameWrites.map((write, index): ResponseSpec => {
        const registry = new Map(currentLayout.namedPages.map((entry) => [entry.name, entry]));
        for (const entry of nameWrites.slice(0, index + 1))
          registry.set(entry.name, {
            name: entry.name,
            target: { kind: 'page', page: entry.page },
          });
        const namedPages = [...registry.values()].sort((left, right) =>
          left.name < right.name ? -1 : left.name > right.name ? 1 : 0,
        );
        return {
          path: 'pages.setName',
          args: [write],
          value: {
            layout: { ...currentLayout, namedPages },
            meta: mutationMeta([pages[index]]),
          } satisfies Awaited<ReturnType<LocalDocumentHandle['pages']['setName']>>,
        };
      }),
      { path: 'pieceInfo.get', value: catalog },
      ...pages.map(
        (page): ResponseSpec => ({
          path: 'page.pieceInfo.get',
          pon: page.ref.objectNumber,
          value: input.page?.[page.ref.objectNumber]
            ? pieceInfo(input.page[page.ref.objectNumber])
            : null,
        }),
      ),
      ...(input.catalogWrites ?? [{ id: 'stamp-lib-0-1' }]).map((write): ResponseSpec => {
        const patch = stampLibraryPieceInfo(write.id, write);
        return {
          path: 'pieceInfo.update',
          args: ['EMBD_StampLibrary', patch],
          value: {
            pieceInfo: writtenPieceInfo(patch, input.catalog),
            meta: mutationMeta(),
          } satisfies Awaited<ReturnType<LocalDocumentHandle['pieceInfo']['update']>>,
        };
      }),
      ...pages.map((page): ResponseSpec => {
        const write = input.pageWrites?.[page.ref.objectNumber] ?? { kind: 'stamp' };
        const patch = stampPieceInfo(write.kind, write);
        return {
          path: 'page.pieceInfo.update',
          pon: page.ref.objectNumber,
          args: ['EMBD_Stamp', patch],
          value: {
            pieceInfo: writtenPieceInfo(patch, input.page?.[page.ref.objectNumber]),
            meta: mutationMeta([page]),
          } satisfies Awaited<
            ReturnType<ReturnType<LocalDocumentHandle['page']>['pieceInfo']['update']>
          >,
        };
      }),
      ...pages.map(
        (page): ResponseSpec => ({
          path: 'pages.extract',
          args: [[page.ref]],
          value: canonical(pdf(`page-${page.ref.objectNumber}`)),
        }),
      ),
      ...pages.map(
        (page): ResponseSpec => ({
          path: 'page.render.image',
          pon: page.ref.objectNumber,
          value: canonical(image(page.ref.objectNumber)),
        }),
      ),
      { path: 'download', value: canonical(pdf(input.download ?? 'canonical')) },
    ],
  };
}
export const opens = (...keys: string[]): ResponseSpec[] =>
  keys.map((key) => ({ path: 'open', times: 1, value: { $document: key } }));
export const engineError = (code: string, message: string): ErrorSpec => ({
  name: 'EngineError',
  message,
  code,
});

registerPlugin('stamp-current-persistence', {
  factory: (config, env) => {
    const input = (config ?? {}) as { readonly fail?: Readonly<Record<string, string>> };
    return definePlugin({
      id: 'stamp-persistence',
      token: env.token('stamp-persistence'),
      scope: 'workspace',
      requires: [StampToken],
      create: (ctx) => {
        const memory = memoryStampStore();
        const store: StampLibraryStore = {
          list: () =>
            input.fail?.list ? Promise.reject(new Error(input.fail.list)) : memory.list(),
          put: (id, value) =>
            input.fail?.put ? Promise.reject(new Error(input.fail.put)) : memory.put(id, value),
          delete: (id) =>
            input.fail?.delete ? Promise.reject(new Error(input.fail.delete)) : memory.delete(id),
        };
        let unsubscribe: (() => void) | null = null;
        ctx.cleanup(() => unsubscribe?.());
        return {
          api: {
            seed: async (rows: { id: string; bytes: Uint8Array }[]) => {
              for (const row of rows) await memory.put(row.id, row.bytes);
            },
            rows: () => memory.list(),
            restore: () => restoreStampLibraries(ctx.get(StampToken), store),
            persist: (options?: { except?: readonly string[] }) => {
              unsubscribe?.();
              unsubscribe = persistStampLibraries(ctx.get(StampToken), store, options);
            },
            stop: () => {
              unsubscribe?.();
              unsubscribe = null;
            },
          },
        };
      },
    });
  },
});
