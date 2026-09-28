/**
 * `@embedpdf/core`'s `Kernel` over `WasmKernel`: the Rust kernel and the Rust
 * plugins, built from the descriptors `./plugins` makes.
 *
 * A plugin without `$rust` is a TypeScript `PluginDef`. One that only has
 * `id`, `scope`, `requires`, `optional` and `init` (and `token`) runs here:
 * once `start()` resolves, `init(ctx)` runs once for a workspace plugin, or
 * once per opened document for a document plugin, with a context whose
 * `get` and `tryGet` answer the same capability proxies `capability` does.
 * Anything with its own state, reducer, capability or effects throws.
 *
 * Everything here imports `@embedpdf/core` as types only, so the eval app
 * can point `@embedpdf/core` at `./redirect/core` without a cycle.
 */
import type {
  AnyPlugin,
  CapabilityToken,
  DocumentsCapability,
  Engine,
  GlobalState,
  Kernel,
  KernelStatus,
  PluginScope,
  Unsubscribe,
} from '@embedpdf/core';
import { createEngineBridge } from './engine-bridge';
import { isRustPlugin, type RustPluginDef } from './plugins';
import { rusty, toError, Wire, type RustKernelClass } from './proxy';

let RustKernel: RustKernelClass | undefined;

/** The `WasmKernel` class to build kernels with: `wasm/` (after its
 *  `init()`) in a browser, `wasm-node/` under Node. */
export function setWasmKernel(kernel: RustKernelClass): void {
  RustKernel = kernel;
}

const UNSUPPORTED = ['initialState', 'reduce', 'capability', 'create', 'effects'] as const;

function checkJsPlugin(plugin: AnyPlugin): void {
  for (const member of UNSUPPORTED) {
    if (plugin[member] !== undefined) {
      throw new Error(
        `[core-wasm] JS plugin "${plugin.id}" has ${member}; only init-only JS plugins run in the eval build`,
      );
    }
  }
}

type Cleanup = () => void;

export function createKernel(opts: {
  engine: Engine;
  plugins: AnyPlugin[];
  /** Kept for the signature; the Rust kernel reports to `console.error`. */
  report?: (error: unknown) => void;
}): Kernel {
  const { engine } = opts;
  const report = opts.report ?? ((error: unknown) => console.error('[kernel]', error));
  const rustPlugins: RustPluginDef[] = [];
  const jsPlugins: AnyPlugin[] = [];
  for (const plugin of opts.plugins as (AnyPlugin | RustPluginDef)[]) {
    if (isRustPlugin(plugin)) {
      rustPlugins.push(plugin);
    } else {
      checkJsPlugin(plugin);
      jsPlugins.push(plugin);
    }
  }
  if (!RustKernel) throw new Error('[core-wasm] setWasmKernel(WasmKernel) before createKernel');

  const bridge = createEngineBridge(engine as never);
  const wire = new Wire(bridge);
  const specs = rustPlugins.map((p) => ({ factory: p.$rust, config: wire.encode(p.config) }));
  const Rust = RustKernel;
  const rust = (() => {
    try {
      return new Rust(JSON.stringify(specs), bridge, wire.callJs);
    } catch (error) {
      throw toError(error);
    }
  })();
  wire.attach(rust);

  const listeners = new Set<() => void>();
  rust.subscribe(() => {
    wire.bump();
    for (const listener of [...listeners]) listener();
  });

  const documents = wire.documents() as DocumentsCapability;
  const name = (token: CapabilityToken<unknown>) => token.name;

  // ── init-only JS plugins ──
  const cleanups = new Map<string | null, Cleanup[]>();
  const runInit = (plugin: AnyPlugin, doc: string | null) => {
    const list = cleanups.get(doc) ?? [];
    cleanups.set(doc, list);
    const ctx = {
      id: plugin.id,
      engine,
      documentId: doc ?? undefined,
      get: (token: CapabilityToken<unknown>) => wire.capability(name(token), doc),
      tryGet: (token: CapabilityToken<unknown>) => wire.tryCapability(name(token), doc),
      forDocument: (token: CapabilityToken<unknown>, id: string) =>
        wire.capability(name(token), id),
      tryForDocument: (token: CapabilityToken<unknown>, id: string) =>
        wire.tryCapability(name(token), id),
      core: () => (wire.getState() as GlobalState).core,
      cleanup: (fn: Cleanup) => list.push(fn),
    };
    try {
      Promise.resolve(plugin.init?.(ctx as never)).catch(report);
    } catch (error) {
      report(error);
    }
  };
  const runCleanups = (doc: string | null) => {
    for (const fn of cleanups.get(doc)?.splice(0) ?? []) {
      try {
        fn();
      } catch (error) {
        report(error);
      }
    }
  };
  const startJsPlugins = () => {
    const perDocument = jsPlugins.filter((p) => p.scope === 'document');
    for (const plugin of jsPlugins) if (plugin.scope !== 'document') runInit(plugin, null);
    if (perDocument.length === 0) return;
    const opened = (doc: string) => {
      for (const plugin of perDocument) runInit(plugin, doc);
    };
    // Deferred, so an init's calls do not re-enter the kernel mid-event.
    documents.onOpened((event) => queueMicrotask(() => opened(event.documentId)));
    documents.onClosed((event) => runCleanups(event.documentId));
    for (const doc of documents.list()) if (doc.status === 'ready') opened(doc.id);
  };

  let started: Promise<void> | undefined;
  let destroyed: Promise<void> | undefined;

  return {
    engine,
    documents,
    capability: <T>(token: CapabilityToken<T>, documentId?: string) =>
      wire.capability(name(token), documentId) as T,
    tryCapability: <T>(token: CapabilityToken<T>, documentId?: string) =>
      wire.tryCapability(name(token), documentId) as T | null,
    scopeOf: (token) => rusty(() => rust.scope_of(name(token))) as PluginScope,
    subscribe: (listener): Unsubscribe => {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    getState: () => wire.getState() as GlobalState,
    status: () => rusty(() => rust.status()) as KernelStatus,
    start: () =>
      (started ??= rust.start().then(startJsPlugins, (error) => {
        throw toError(error);
      })),
    destroy: () =>
      (destroyed ??= (async () => {
        for (const doc of [...cleanups.keys()]) runCleanups(doc);
        try {
          await rust.destroy();
        } catch (error) {
          report(toError(error));
        }
      })()),
  };
}
