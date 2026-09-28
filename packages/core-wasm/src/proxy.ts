/**
 * The wire between JavaScript values and `WasmKernel`'s canonical JSON
 * (`crates/epdf-web/src/exports.rs` has the protocol):
 *
 *   - A capability arrives described, member name to
 *     `{"$ref": n, "pinned": true}`. It becomes one object per description,
 *     so one per (plugin, document): the kernel memoizes descriptions, so
 *     the same capability always describes to the same text, and pins
 *     their refs.
 *   - A `$ref` becomes a function that encodes its arguments, calls
 *     `invoke_ref` and maps the outcome. Once a function made from an
 *     unpinned ref is garbage, a `FinalizationRegistry` asks the kernel to
 *     `release_ref` it; the engine runs that callback when it chooses, so
 *     this bounds the kernel's ref table over time, not at once. A released
 *     id showing up again throws. A function argument becomes
 *     `{"$fn": "js:<id>"}` in a table that only grows; an `AbortSignal`
 *     argument is taken out and aborts the call's promise instead.
 *   - `{"$promise": n}` becomes a promise with an `abort(reason)`,
 *     `{"$host": n}` the engine bridge's live value, `$set` and `$map` the
 *     collections, and the command context's `{"$kernel": ...}` markers
 *     closures over this wire.
 *
 * Reference stability. React selectors compare by reference, so a getter
 * that answers a fresh object on every call loops `useSyncExternalStore`.
 * Every call's decoded result is cached by (ref, argument JSON), but only
 * while no other real call has landed since: any `invoke_ref` that is not
 * itself served from the cache clears every entry first (the store's change
 * notification can lag behind a mutation made from inside a change listener,
 * so caching cannot wait for it). A real call whose answer text matches the
 * last answer for the same (ref, argument JSON), ref ids aside, reuses the
 * value that answer decoded to, so two getters read in turn (`useSurface`
 * reads `isOpen` and `getSurface`) do not cost each other their references.
 * The new answer's own ref ids are then never decoded, so no function made
 * from them can ever be collected to release them: they are released on
 * the spot instead.
 * `getState` does the same per slice. A call whose member makes something
 * new (`create*`, `register*`, `on*`) is not cached or reused; neither is
 * `undefined`, a throw or a promise. A call during which the store changed
 * is not cached, and a callback from the kernel (`callJs`) clears the cache.
 */
import {
  canonical,
  decode,
  errorSpec,
  readBlobs,
  type DecodeHook,
  type EngineBridge,
} from './engine-bridge';
import { prepareArgs, rehydrate } from './rehydrate';

/** What this package uses of `WasmKernel` (`wasm/epdf_web.d.ts`). */
export interface RustKernel {
  capability(tokenName: string, docId?: string | null): string;
  try_capability(tokenName: string, docId?: string | null): string | undefined;
  documents(): string;
  invoke_ref(id: number, argsJson: string): string;
  release_ref(id: number): void;
  promise(id: number): Promise<unknown>;
  abort_promise(id: number, reasonJson?: string | null): void;
  state_json(): string;
  subscribe(listener: () => void): number;
  unsubscribe(id: number): void;
  status(): string;
  scope_of(tokenName: string): string;
  start(): Promise<unknown>;
  destroy(): Promise<unknown>;
  /** The harness's live-table counts plus `described`, as JSON text. */
  debug_counts(): string;
}

export type RustKernelClass = new (
  pluginsJson: string,
  engine: EngineBridge,
  callJs: (id: string, argsJson: string) => unknown,
) => RustKernel;

type Fn = (...args: unknown[]) => unknown;
type Outcome = { returned: unknown } | { threw: unknown } | { promise: number };

/** Members whose result is something new on every call. */
const MAKES_NEW = /^(create|register|on[A-Z]|subscribe)/;

/** How many distinct calls keep their last answer for reuse. */
const ANSWERS_KEPT = 4096;

// Answer reuse (below) assumes a returned ref's behavior is fully determined
// by its answer's text (ref ids blanked) or by live state it reads afresh on
// each call, never by which physical ref id it happens to hold.
const REF_ID = /\{"\$ref":\d+\}/g;
/** The same, capturing the id. A pinned ref (`"pinned":true` after the id)
 *  matches neither, so answer reuse compares it as text and never releases
 *  it. */
const REF_IDS = /\{"\$ref":(\d+)\}/g;

const refIds = (text: string): Set<number> =>
  new Set(Array.from(text.matchAll(REF_IDS), (match) => Number(match[1])));

const isThenable = (value: unknown): value is PromiseLike<unknown> =>
  value !== null &&
  (typeof value === 'object' || typeof value === 'function') &&
  typeof (value as { then?: unknown }).then === 'function';

const isRef = (value: unknown): value is { $ref: number; pinned?: boolean } =>
  value !== null &&
  typeof value === 'object' &&
  typeof (value as { $ref?: unknown }).$ref === 'number';

/** A canonical `{"$error": {...}}` (or anything thrown) as an `Error` with
 *  its `name`, `code` and other members. */
export function toError(thrown: unknown): Error {
  const spec = (thrown as { $error?: Record<string, unknown> } | null)?.$error;
  if (!spec) return thrown instanceof Error ? thrown : new Error(String(thrown));
  const { name, message, ...members } = spec;
  const error = new Error(typeof message === 'string' ? message : String(message));
  if (typeof name === 'string') error.name = name;
  Object.assign(error, members);
  return error;
}

/** Run a kernel export, turning its canonical throw into an `Error`. */
export function rusty<T>(run: () => T): T {
  try {
    return run();
  } catch (error) {
    throw toError(error);
  }
}

export class Wire {
  private rust!: RustKernel;
  private version = 0;
  private readonly fns: Fn[] = [];
  private readonly fnIds = new Map<Fn, number>();
  /** A capability's own refs, kept for good, as the kernel keeps them. */
  private readonly pinned = new Map<number, Fn>();
  /** Every other ref's function, held weakly: `released` hands the id back
   *  to the kernel once the function is collected. */
  private readonly loose = new Map<number, WeakRef<Fn>>();
  private readonly released = new FinalizationRegistry<number>((id) => {
    this.loose.delete(id);
    this.rust.release_ref(id);
  });
  private readonly proxies = new Map<string, object>();
  private results = new Map<string, unknown>();
  /** Each (ref, argument JSON)'s last answer, ref ids blanked, and the value
   *  it decoded to, kept across `bump`: an answer whose text has not changed
   *  decodes to the value it did last time. `text` is the answer `value`
   *  was decoded from, whose refs it holds. Cleared whole past
   *  `ANSWERS_KEPT` keys. */
  private answers = new Map<string, { shape: string; text: string; value: unknown }>();
  private state: { version: number; text: string; value: unknown } | undefined;
  /** How many times the kernel's `subscribe` listener has fired
   *  (`kernel.ts` counts them). A call during which it moved changed the
   *  store, so its answer is not cached. */
  changes = 0;
  /** Each slice's JSON text and decoded value, by path (`core`,
   *  `plugins.stage`), from the last `getState` that parsed. */
  private slices = new Map<string, { text: string; value: unknown }>();

  constructor(private readonly bridge: EngineBridge) {}

  /** The kernel, once it is built (it needs `callJs` to build). */
  attach(rust: RustKernel): void {
    this.rust = rust;
  }

  /** The store changed: every cached result is stale. */
  bump(): void {
    this.version++;
    this.results = new Map();
  }

  /** `value` as canonical JSON (an object, not text). */
  encode(value: unknown, signals?: AbortSignal[]): unknown {
    return canonical(value, {
      hold: this.bridge.hold,
      fn: (f) => ({ $fn: `js:${this.fnId(f)}` }),
      signal: signals ? (s) => signals.push(s) : undefined,
    });
  }

  decode(value: unknown): unknown {
    return decode(value, this.bridge.host, this.hook);
  }

  /** `call_js`: a `$fn` callback's call from the kernel. */
  readonly callJs = (id: string, argsJson: string): unknown => {
    // A callback from inside a kernel call: the store may have moved since
    // anything was cached, with its notification still to come.
    this.bump();
    const fn = this.fns[Number(id)];
    if (!fn) throw new Error(`[core-wasm] no JavaScript function ${id}`);
    const args = this.decode(JSON.parse(argsJson)) as unknown[];
    let out: unknown;
    try {
      out = fn(...args);
    } catch (error) {
      throw errorSpec(error, (v) => this.encode(v));
    }
    if (!isThenable(out)) return this.encode(out);
    return Promise.resolve(out).then(
      async (value) => this.encode(await readBlobs(value)),
      (error) => {
        throw errorSpec(error, (v) => this.encode(v));
      },
    );
  };

  /** `kernel.capability(token, doc)` as its proxy. */
  capability(tokenName: string, doc?: string | null): object {
    return this.proxy(rusty(() => this.rust.capability(tokenName, doc ?? null)));
  }

  tryCapability(tokenName: string, doc?: string | null): object | null {
    const text = rusty(() => this.rust.try_capability(tokenName, doc ?? null));
    return text === undefined || text === null ? null : this.proxy(text);
  }

  documents(): object {
    return this.proxy(rusty(() => this.rust.documents()));
  }

  /** The kernel's state, parsed once per store change. */
  getState(): unknown {
    if (this.state?.version === this.version) return this.state.value;
    // `version` also moves on every real call (see `call`), which does not
    // mean the store changed: the same text keeps the same object, and a
    // changed store keeps every slice whose text did not change, as the
    // TypeScript store keeps an untouched plugin's slice.
    const text = this.rust.state_json();
    if (this.state?.text === text) {
      this.state.version = this.version;
      return this.state.value;
    }
    const raw = JSON.parse(text) as Record<string, unknown>;
    const slices = new Map<string, { text: string; value: unknown }>();
    const share = (path: string, slice: unknown) => {
      const sliceText = JSON.stringify(slice) ?? 'undefined';
      const last = this.slices.get(path);
      const entry =
        last?.text === sliceText ? last : { text: sliceText, value: this.decode(slice) };
      slices.set(path, entry);
      return entry.value;
    };
    const value: Record<string, unknown> = {};
    for (const [key, slice] of Object.entries(raw)) {
      if (key === 'plugins' && slice !== null && typeof slice === 'object') {
        const plugins: Record<string, unknown> = {};
        for (const [id, plugin] of Object.entries(slice)) {
          plugins[id] = share(`plugins.${id}`, plugin);
        }
        value[key] = plugins;
      } else {
        value[key] = share(key, slice);
      }
    }
    this.slices = slices;
    this.state = { version: this.version, text, value };
    return value;
  }

  /** A promise id as a promise of its decoded value, with `abort`. */
  promise(id: number): Promise<unknown> & { abort(reason?: unknown): void } {
    const out = this.rust.promise(id).then(
      (value) => this.decode(value),
      (error) => {
        throw toError(error);
      },
    ) as Promise<unknown> & { abort(reason?: unknown): void };
    out.abort = (reason?: unknown) =>
      this.rust.abort_promise(
        id,
        reason === undefined ? null : JSON.stringify(this.encode(reason)),
      );
    return out;
  }

  /** Live entries in this wire's own tables, for the eval build's memory
   *  soak test: `fns` handed to Rust, `refs` handed back from it, and
   *  cached `proxies`. */
  counts(): { fns: number; refs: number; proxies: number } {
    const refs = this.pinned.size + this.loose.size;
    return { fns: this.fns.length, refs, proxies: this.proxies.size };
  }

  private proxy(text: string): object {
    let proxy = this.proxies.get(text);
    if (!proxy) {
      proxy = this.decode(JSON.parse(text)) as object;
      this.proxies.set(text, proxy);
    }
    return proxy;
  }

  private fnId(f: Fn): number {
    let id = this.fnIds.get(f);
    if (id === undefined) {
      id = this.fns.push(f) - 1;
      this.fnIds.set(f, id);
    }
    return id;
  }

  /** The function behind ref `id`, one per ref. `member` names it for the
   *  cache's rules and the rehydration table. An unpinned ref's function is
   *  released once it is collected; its id decoding again after that is a
   *  bug in whoever kept the text, and throws. */
  private ref(id: number, member: string, pinned: boolean): Fn {
    if (pinned) {
      let fn = this.pinned.get(id);
      if (!fn) {
        fn = (...args: unknown[]) => this.call(id, member, args);
        this.pinned.set(id, fn);
      }
      return fn;
    }
    const weak = this.loose.get(id);
    if (weak) {
      const live = weak.deref();
      if (live) return live;
      const error = new Error(`[core-wasm] ref ${id} decoded again after it was dropped`);
      console.error(error);
      throw error;
    }
    const fn = (...args: unknown[]) => this.call(id, member, args);
    this.loose.set(id, new WeakRef(fn));
    this.released.register(fn, id);
    return fn;
  }

  private call(id: number, member: string, args: unknown[]): unknown {
    const signals: AbortSignal[] = [];
    const json = JSON.stringify(this.encode(prepareArgs(member, args), signals));
    const key = `${id}:${json}`;
    if (this.results.has(key)) return this.results.get(key);
    const changes = this.changes;
    const text = rusty(() => this.rust.invoke_ref(id, json));
    const outcome = JSON.parse(text) as Outcome;
    // A real call, not a cache hit: the store may have deferred its change
    // notification (a dispatch from inside a change listener only sets
    // `pending`, `crates/epdf-kernel/src/store.rs:206-219`), so `bump` can't
    // wait for `subscribe` to fire. Only two calls with no real call between
    // them may share a cache entry.
    this.bump();
    if ('threw' in outcome) throw toError(outcome.threw);
    if ('promise' in outcome) {
      const promise = this.promise(outcome.promise);
      for (const signal of signals) {
        if (signal.aborted) {
          promise.abort(signal.reason);
          continue;
        }
        // `{ once: true }` only drops the listener once the signal actually
        // fires; a call that never aborts would otherwise keep this closure
        // (and the signal) alive on `signal` for as long as the caller holds
        // it. Settling either way is the other half of "once": remove it
        // then too.
        const onAbort = () => promise.abort(signal.reason);
        signal.addEventListener('abort', onAbort, { once: true });
        const detach = () => signal.removeEventListener('abort', onAbort);
        promise.then(detach, detach);
      }
      return promise;
    }
    if (MAKES_NEW.test(member)) return rehydrate(member, this.decode(outcome.returned));
    // Compared with its ref ids blanked: some members describe what they
    // return with newly registered refs on every call (stage's
    // `listVisiblePages` does, for each page transform), and the refs the
    // last answer holds are still live and stand in the same places.
    const shape = text.replace(REF_ID, '{"$ref":0}');
    const last = this.answers.get(key);
    let value: unknown;
    let decoded = text;
    if (last?.shape === shape) {
      value = last.value;
      decoded = last.text;
      // Nothing decodes this answer's fresh refs, so nothing would ever
      // release them. One the kept answer holds too is still live.
      const kept = refIds(last.text);
      for (const id of refIds(text)) if (!kept.has(id)) this.rust.release_ref(id);
    } else {
      value = rehydrate(member, this.decode(outcome.returned));
    }
    if (value !== undefined) {
      // A call that changed the store (`search.nextHit`, `link.activate`)
      // answers anew next time, even with nothing between.
      if (this.changes === changes) this.results.set(key, value);
      if (this.answers.size >= ANSWERS_KEPT) this.answers = new Map();
      this.answers.set(key, { shape, text: decoded, value });
    }
    return value;
  }

  /** The command context's markers (`epdf_plugin_commands::harness`). */
  private marker(kind: unknown, doc: string | null): unknown {
    type Token = { name: string };
    switch (kind) {
      case 'get':
        return (token: Token) => this.capability(token.name, doc);
      case 'tryGet':
        return (token: Token) => {
          try {
            return this.tryCapability(token.name, doc);
          } catch {
            return null;
          }
        };
      case 'core':
        return () => (this.getState() as { core: unknown }).core;
      default:
        throw new Error(`[core-wasm] unknown kernel marker ${String(kind)}`);
    }
  }

  private readonly hook: DecodeHook = (object, recur) => {
    if ('$host' in object || '$undefined' in object || '$bytes' in object || '$number' in object) {
      return undefined;
    }
    if (isRef(object)) return { value: this.ref(object.$ref, '', object.pinned === true) };
    if (typeof object.$promise === 'number') return { value: this.promise(object.$promise) };
    if ('$kernel' in object) {
      return { value: this.marker(object.$kernel, (object.doc as string | null) ?? null) };
    }
    if (Array.isArray(object.$set)) return { value: new Set(object.$set.map(recur)) };
    if (Array.isArray(object.$map)) {
      return {
        value: new Map(
          (object.$map as [unknown, unknown][]).map(([k, v]) => [recur(k), recur(v)] as const),
        ),
      };
    }
    if (object.$signal && typeof object.$signal === 'object') {
      const controller = new AbortController();
      if ((object.$signal as { aborted?: boolean }).aborted) controller.abort();
      return { value: controller.signal };
    }
    // An object with refs among its members: each becomes a function named
    // after its member.
    const members = Object.entries(object);
    if (!members.some(([, member]) => isRef(member))) return undefined;
    const out: Record<string, unknown> = {};
    for (const [key, member] of members) {
      const live = isRef(member)
        ? this.ref(member.$ref, key, member.pinned === true)
        : recur(member);
      if (live !== undefined) out[key] = live;
    }
    return { value: out };
  };
}
