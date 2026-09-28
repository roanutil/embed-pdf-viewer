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
import { createEngineBridge, type EngineBridge } from './engine-bridge';
import { isRustPlugin, type RustPluginDef } from './plugins';
import { rusty, toError, Wire, type RustKernel, type RustKernelClass } from './proxy';

let RustKernel: RustKernelClass | undefined;

/** The `WasmKernel` class to build kernels with: `wasm/` (after its
 *  `init()`) in a browser, `wasm-node/` under Node. */
export function setWasmKernel(kernel: RustKernelClass): void {
  RustKernel = kernel;
}

let wasmMemoryBytes: (() => number) | undefined;

/** The wasm module's `wasm_memory_bytes` export (`crates/epdf-web/src/exports
 *  .rs`), the whole module's linear memory in bytes. Set alongside
 *  [`setWasmKernel`]: `wasm/` in a browser, `wasm-node/` under Node. */
export function setWasmMemoryBytes(fn: () => number): void {
  wasmMemoryBytes = fn;
}

/** Each kernel this module built, by the object `createKernel` returned, so
 *  `kernelCounts` can reach its Rust handle, bridge and wire without
 *  widening the public `Kernel` type. Dev-only, for the eval build's memory
 *  soak test. */
const tables = new WeakMap<Kernel, { rust: RustKernel; bridge: EngineBridge; wire: Wire }>();

/** Every kernel `createKernel` built and has not yet destroyed, so a debug
 *  hook that has no reference of its own (`examples/rust-kernel-eval`'s
 *  `window.__epdfCounts`, called from a document the framework wrappers
 *  built) can still read counts, and so a destroyed or ambiguous default
 *  fails loudly instead of silently reporting the wrong (or a dead)
 *  kernel's counts. */
const liveKernels = new Set<Kernel>();

/** Live entries in every table this session's memory tasks are chasing, for
 *  `kernel` (built by [`createKernel`]; defaults to the sole live kernel,
 *  throwing if there is none or more than one): the harness's
 *  `refs`/`promises`, `described` capabilities, `aborts`, the engine bridge's
 *  `hosts` (its own entries), `allHosts` (every bridge's: the one table
 *  they share) and `documents`, the wire's `fns`/`wireRefs`/`proxies`, and
 *  `wasmBytes`, the wasm module's own linear memory.
 *
 *  `refs` is the harness's table, not the wire's: the two are not the same
 *  count. The wire holds an unpinned ref's function weakly and drops its
 *  entry when a `FinalizationRegistry` callback releases it in the kernel,
 *  so the two only agree once the engine has run those callbacks; an answer
 *  the wire reuses releases its undecoded fresh refs at once (`proxy.ts`).
 *  Both are reported, under different keys, so a merge can never hide a
 *  gap between them. `aborts` is `WasmKernel`'s table of taken promises
 *  still abortable. */
export function kernelCounts(kernel?: Kernel): Record<string, number> {
  if (kernel === undefined) {
    if (liveKernels.size === 0) throw new Error('[core-wasm] kernelCounts: no live kernel');
    if (liveKernels.size > 1) {
      throw new Error(
        `[core-wasm] kernelCounts: ${liveKernels.size} live kernels; pass one explicitly`,
      );
    }
    [kernel] = liveKernels;
  }
  const entry = tables.get(kernel);
  if (!entry) throw new Error('[core-wasm] kernelCounts: not a kernel createKernel built');
  const harness = JSON.parse(rusty(() => entry.rust.debug_counts())) as Record<string, number>;
  const { refs: wireRefs, ...wireRest } = entry.wire.counts();
  const wasmBytes = wasmMemoryBytes ? wasmMemoryBytes() : NaN;
  return { ...harness, ...entry.bridge.counts(), ...wireRest, wireRefs, wasmBytes };
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

/** Render's live `host.releaseImage`: the render plugin's raster store
 *  reports the `$host` id of each image its last Rust handle let go of, and
 *  this kernel's bridge releases it. Added here, not in `renderPlugin`
 *  (`./plugins`): the factory runs before there is a bridge. */
function withReleaseImage(config: unknown, bridge: EngineBridge): unknown {
  const base = (config ?? {}) as { host?: Record<string, unknown> };
  return { ...base, host: { ...base.host, releaseImage: (n: number) => bridge.release(n) } };
}

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
  const specs = rustPlugins.map((p) => ({
    factory: p.$rust,
    config: wire.encode(p.$rust === 'render' ? withReleaseImage(p.config, bridge) : p.config),
  }));
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
    wire.changes++;
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

  const kernel: Kernel = {
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
        liveKernels.delete(kernel);
        for (const doc of [...cleanups.keys()]) runCleanups(doc);
        try {
          await rust.destroy();
        } catch (error) {
          report(toError(error));
        }
      })()),
  };
  tables.set(kernel, { rust, bridge, wire });
  liveKernels.add(kernel);
  return kernel;
}
