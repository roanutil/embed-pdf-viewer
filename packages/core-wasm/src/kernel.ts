/**
 * `@embedpdf/core`'s `Kernel` over `WasmKernel`: the Rust kernel and the Rust
 * plugins, built from the descriptors `./plugins` makes.
 *
 * A plugin without `$rust` is a TypeScript `PluginDef`. One that only has
 * `id`, `scope`, `requires`, `optional` and `init` (and `token`) runs here.
 * A workspace plugin's `init(ctx)` runs inside `start()`, after the Rust
 * plugins start, awaited, and a failure fails `start()`; a document
 * plugin's runs once per opened document. Its context answers the same
 * capability proxies `capability` does; `doc`, `documentHandle` and
 * `dispatch` throw. Anything with its own state, reducer, capability or
 * effects throws.
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
import { rusty, toError, Wire, type RustKernelClass } from './proxy';

/** One frame at 60 Hz: the most often store listeners hear of changes. */
const FRAME_MS = 16;

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
 *  `kernelCounts` can reach its bridge and wire (and through the wire its
 *  Rust handle) without widening the public `Kernel` type. Dev-only, for the
 *  eval build's memory soak test. */
const tables = new WeakMap<Kernel, { bridge: EngineBridge; wire: Wire }>();

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
 *  `refs`/`promises`/`heldFns` (the `$fn` ids some Rust callback still
 *  holds), `described` capabilities, `aborts`, `tokens` (the token names
 *  the harness knows), the engine bridge's
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
  const harness = JSON.parse(rusty(() => entry.wire.rust.debug_counts())) as Record<string, number>;
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

type Cleanup = () => void | Promise<void>;

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
      return new Rust(JSON.stringify(specs), bridge, wire.callJs, wire.releaseJs);
    } catch (error) {
      throw toError(error);
    }
  })();
  wire.attach(rust);

  const listeners = new Set<() => void>();
  // One notification per burst of store changes, after the Rust call that
  // made them returns. The Rust store notifies synchronously on every
  // reducer change, and each notification ran every subscribed hook from
  // inside that call: a camera move made several passes over hundreds of
  // hooks. The caches still invalidate at once, so a read in between sees
  // the new state.
  //
  // At most one notification a frame, too: a change after a quiet frame
  // flushes at once, and the rest of that frame's changes wait for its end.
  // Each engine answer is a task of its own, and opening a document answers
  // one measurement read per page, so per-task flushing ran every hook once
  // per page and queued every later engine answer behind those passes.
  let flushing = false;
  let flushedAt = -Infinity;
  const flush = () => {
    flushing = false;
    flushedAt = performance.now();
    for (const listener of [...listeners]) listener();
  };
  const subscription = rust.subscribe(() => {
    wire.changes++;
    wire.bump();
    if (flushing) return;
    flushing = true;
    const wait = flushedAt + FRAME_MS - performance.now();
    if (wait > 0) setTimeout(flush, wait);
    else queueMicrotask(flush);
  });

  const documents = wire.documents() as DocumentsCapability;
  // After the kernel's own listener, which evicted the closed document's
  // capabilities from its memo.
  documents.onClosed(() => wire.evict());
  const name = (token: CapabilityToken<unknown>) => token.name;

  // ── init-only JS plugins ──
  /** Teardowns by owner (`null` for the workspace), until the owner goes:
   *  a document's entry leaves with it. */
  const cleanups = new Map<string | null, Cleanup[]>();
  /** Every teardown `fn` returned a promise from, as `destroy` awaits. */
  const teardowns: Promise<void>[] = [];
  const runCleanup = (fn: Cleanup) => {
    try {
      const done = fn();
      if (done && typeof (done as Promise<void>).then === 'function') {
        teardowns.push(Promise.resolve(done).then(undefined, report));
      }
    } catch (error) {
      report(error);
    }
  };
  const unsupported = (plugin: AnyPlugin, member: string) => () => {
    throw new Error(
      `[core-wasm] JS plugin "${plugin.id}": ctx.${member} is not supported; only init-only JS plugins run in the eval build`,
    );
  };
  const makeContext = (plugin: AnyPlugin, doc: string | null) => {
    const list: Cleanup[] = cleanups.get(doc) ?? [];
    cleanups.set(doc, list);
    const core = () => (kernel.getState() as GlobalState).core;
    const ctx = {
      id: plugin.id,
      engine,
      documentId: doc ?? undefined,
      // The engine's handles live behind the Rust kernel's sessions.
      documentHandle: unsupported(plugin, 'documentHandle'),
      // An init-only plugin has no slice: the TS kernel's lease for it
      // reads `undefined`. Its actions could reach no reducer here.
      getState: () => undefined,
      dispatch: unsupported(plugin, 'dispatch'),
      subscribe: (listener: () => void) => kernel.subscribe(listener),
      core,
      document: () => {
        const { documents: metas, activeId } = core() as {
          documents: Record<string, unknown>;
          activeId: string | null;
        };
        const id = doc ?? activeId;
        return id ? (metas[id] ?? null) : null;
      },
      get: (token: CapabilityToken<unknown>) => kernel.capability(token, doc ?? undefined),
      tryGet: (token: CapabilityToken<unknown>) => kernel.tryCapability(token, doc ?? undefined),
      forDocument: (token: CapabilityToken<unknown>, id: string) => kernel.capability(token, id),
      tryForDocument: (token: CapabilityToken<unknown>, id: string) =>
        kernel.tryCapability(token, id),
      // Once its owner is gone, at once, as the TS kernel does: a late
      // registration cannot leak.
      cleanup: (fn: Cleanup) => (cleanups.get(doc) === list ? list.push(fn) : runCleanup(fn)),
    };
    Object.defineProperty(ctx, 'doc', { enumerable: true, get: unsupported(plugin, 'doc') });
    return ctx;
  };
  const runCleanups = (doc: string | null) => {
    const list = cleanups.get(doc);
    cleanups.delete(doc);
    for (const fn of list ?? []) runCleanup(fn);
  };
  /** Set while the workspace plugins' inits run inside `start()`, and for
   *  good if one fails. */
  let jsPhase: 'idle' | 'starting' | 'failed' = 'idle';
  /** The workspace plugins' inits, one at a time and awaited, as the TS
   *  kernel runs them inside `start()`; then the hooks that run each
   *  document plugin's init per opened document. A failed init fails
   *  `start()` and the kernel. */
  const startJsPlugins = async () => {
    jsPhase = 'starting';
    try {
      for (const plugin of jsPlugins) {
        if (plugin.scope === 'document') continue;
        if (wire.lifecycle === 'ending') return; // destroy() raced us
        await plugin.init?.(makeContext(plugin, null) as never);
      }
    } catch (error) {
      jsPhase = 'failed';
      wire.lifecycle = 'ending';
      runCleanups(null);
      throw error;
    }
    jsPhase = 'idle';
    if (wire.lifecycle === 'ending') return;
    wire.lifecycle = 'started';
    const perDocument = jsPlugins.filter((p) => p.scope === 'document');
    if (perDocument.length === 0) return;
    const opened = (doc: string) => {
      for (const plugin of perDocument) {
        try {
          Promise.resolve(plugin.init?.(makeContext(plugin, doc) as never)).catch(report);
        } catch (error) {
          report(error);
        }
      }
    };
    // Deferred, so an init's calls do not re-enter the kernel mid-event.
    documents.onOpened((event) => queueMicrotask(() => opened(event.documentId)));
    documents.onClosed((event) => runCleanups(event.documentId));
    for (const doc of documents.list()) if (doc.status === 'ready') opened(doc.id);
  };

  let started: Promise<void> | undefined;
  let destroyed: Promise<void> | undefined;
  /** The state once the Rust side is freed: what its destroyed store
   *  answered last, as the TS kernel's `getState()` keeps answering. */
  let finalState: GlobalState | undefined;

  const kernel: Kernel = {
    engine,
    documents,
    capability: <T>(token: CapabilityToken<T>, documentId?: string) => {
      if (finalState || jsPhase === 'failed') {
        throw new Error(`[kernel] capability("${name(token)}") on a ${kernel.status()} kernel`);
      }
      return wire.capability(name(token), documentId) as T;
    },
    // A kernel failed by a JS plugin's init: the TS kernel has cleared its
    // workspace capabilities and opens no document, so nothing resolves.
    tryCapability: <T>(token: CapabilityToken<T>, documentId?: string) =>
      finalState || jsPhase === 'failed'
        ? null
        : (wire.tryCapability(name(token), documentId) as T | null),
    scopeOf: (token) => rusty(() => wire.rust.scope_of(name(token))) as PluginScope,
    subscribe: (listener): Unsubscribe => {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    getState: () => finalState ?? (wire.getState() as GlobalState),
    status: () => {
      if (finalState) return 'destroyed';
      const now = rusty(() => wire.rust.status()) as KernelStatus;
      if (now === 'destroying' || now === 'destroyed') return now;
      if (jsPhase === 'failed') return 'failed';
      // Rust is through its own plugins; the JS ones still init.
      if (jsPhase === 'starting' && now === 'started') return 'starting';
      return now;
    },
    start: () => {
      // Before the memo, as the TS kernel checks: a failed kernel's second
      // `start()` rejects with this, not with the first one's error.
      const now = kernel.status();
      if (now === 'destroying' || now === 'destroyed' || now === 'failed') {
        return Promise.reject(new Error(`[kernel] start() on a ${now} kernel`));
      }
      return (started ??= wire.rust.start().then(startJsPlugins, (error) => {
        throw toError(error, (v) => wire.decode(v));
      }));
    },
    // Frees the Rust side once its destroy settles. After that, as on the
    // TS kernel, `status()` answers 'destroyed', `tryCapability` null,
    // `getState()` the destroyed store's state, and `start()` rejects;
    // `capability` throws, and the rest throws `[core-wasm] kernel
    // destroyed` (`Wire.rust`).
    destroy: () =>
      (destroyed ??= (async () => {
        liveKernels.delete(kernel);
        wire.lifecycle = 'ending';
        for (const doc of [...cleanups.keys()]) runCleanups(doc);
        try {
          await rust.destroy();
        } catch (error) {
          report(toError(error));
        }
        // Asynchronous teardowns are awaited, as the TS kernel's scopes
        // await them; after `rust.destroy()` starts, so it still starts at
        // once.
        await Promise.all(teardowns.splice(0));
        // After destroy settles, not before: `rust.destroy()` closes every
        // session, and closing dispatches document-removed actions the
        // shim's subscription (`wire.bump()`) still needs to see, matching
        // the TS kernel's own order (`packages/core/main/src/kernel.ts`,
        // closing sessions before it clears its own listeners).
        rust.unsubscribe(subscription);
        // `wire.dispose()` stays after `free()`, not before: `free()`'s own
        // Drop cascade is what calls back into JS through `wire.callJs` to
        // release the raster store's still-cached images (`withReleaseImage`
        // in this file), through the same `fns` table `dispose()` clears and
        // the same `dead` flag that makes `callJs` refuse to run at all. See
        // the "Review fixes" deviation in `.superpowers/sdd/task-7-report.md`
        // for why moving `wire.dispose()` earlier, as first proposed, is not
        // safe here: it silently drops that release instead of throwing.
        const last = wire.getState() as GlobalState;
        const kept = { core: last.core, plugins: { ...last.plugins } };
        rust.free();
        wire.dispose();
        bridge.dispose();
        finalState = kept;
      })()),
  };
  tables.set(kernel, { bridge, wire });
  liveKernels.add(kernel);
  return kernel;
}
