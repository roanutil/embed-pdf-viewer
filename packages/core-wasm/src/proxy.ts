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
 *     `{"$fn": "js:<id>"}`, one id per function however often it crosses,
 *     and stays in the table until the kernel's `release_js(id)` says no
 *     callback holds it (the kernel counts those per id, so one release
 *     means all of them). The same function passed after that crosses
 *     under a new id; the kernel calling a released id throws. An
 *     `AbortSignal` argument is taken out and aborts the call's promise
 *     instead. A `Blob` argument (a picked `File`) is read to bytes first
 *     (`readBinarySources`), so its call waits for the read and answers a
 *     promise; a `Blob` that reaches the encoder anyway throws.
 *   - `{"$promise": n}` becomes a promise with an `abort(reason)`,
 *     `{"$host": n}` the engine bridge's live value, `$set` and `$map` the
 *     collections, and the command context's `{"$kernel": ...}` markers
 *     closures over this wire.
 *   - A closed document's capabilities come back from `take_evicted` as
 *     the texts they described to (`evict`): each proxy leaves the cache,
 *     and its refs, no longer pinned, are held weakly and released like
 *     any other.
 *   - Once the kernel is freed (`dispose`), a call through this wire throws
 *     `[core-wasm] kernel destroyed`, and what the engine or a timer runs
 *     later (a finalizer's release, an abort, the kernel's own releases)
 *     does nothing.
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
import { canonical, decode, errorSpec, type DecodeHook, type EngineBridge } from './engine-bridge';
import { hasBlob, prepareArgs, readBinarySources, rehydrate } from './rehydrate';

/** What this package uses of `WasmKernel` (`wasm/epdf_web.d.ts`). */
export interface RustKernel {
  capability(tokenName: string, docId?: string | null): string;
  try_capability(tokenName: string, docId?: string | null): string | undefined;
  documents(): string;
  invoke_ref(id: number, argsJson: string): string;
  release_ref(id: number): void;
  /** The description texts of closed documents' capabilities, as a JSON
   *  array, each handed out once. */
  take_evicted(): string;
  promise(id: number): Promise<unknown>;
  abort_promise(id: number, reasonJson?: string | null): void;
  state_json(): string;
  /** Moves whenever `state_json` could answer differently. */
  state_revision(): number;
  /** Moves whenever core changes: the documents, and so the capabilities
   *  `try_capability` can answer. */
  core_revision(): number;
  /** `{"core": n, "plugins": {"<key>": n}}`: a revision per part of
   *  `state_json`. */
  state_revisions(): string;
  /** `state_json().core` as text. */
  core_text(): string;
  /** `state_json().plugins[key]` as text, or `undefined`. */
  slice_text(key: string): string | undefined;
  subscribe(listener: () => void): number;
  unsubscribe(id: number): void;
  status(): string;
  scope_of(tokenName: string): string;
  start(): Promise<unknown>;
  destroy(): Promise<unknown>;
  /** The harness's live-table counts plus `described`, as JSON text. */
  debug_counts(): string;
  /** wasm-bindgen's: drop the Rust side. Nothing may call it again. */
  free(): void;
}

export type RustKernelClass = new (
  pluginsJson: string,
  engine: EngineBridge,
  callJs: (id: string, argsJson: string) => unknown,
  releaseJs: (id: string) => void,
) => RustKernel;

type Fn = (...args: unknown[]) => unknown;
type Outcome = { returned: unknown } | { threw: unknown } | { promise: number };

/** Members whose result is something new on every call. */
const MAKES_NEW = /^(create|register|on[A-Z]|subscribe)/;

/** Members whose answer carries engine images (`$host`) nothing reads
 *  through the host table again once they are decoded: annotation's
 *  `renderAppearances` passes the engine's
 *  `page.annotations.renderAppearanceImages` images straight through
 *  (`crates/plugins/annotation/src/read/render.rs`), and the viewer reads
 *  each one's `objectUrl()` off the live handle, which keeps its own bytes.
 *  Their entries are released once the answer is decoded (`promise`).
 *  Only outermost `$host` ids are released (`hostIds`), so a member added
 *  here must answer no live value nested inside another: that inner one
 *  would stay in the table. */
const RELEASES_HOSTS = new Set(['renderAppearances']);

/** Every `$host` id in a canonical value. One inside another `$host` is
 *  never looked for: a held image's `$hostBytes` goes with its release. */
function hostIds(value: unknown, out: number[] = []): number[] {
  if (Array.isArray(value)) {
    for (const item of value) hostIds(item, out);
  } else if (value !== null && typeof value === 'object') {
    const object = value as Record<string, unknown>;
    if (typeof object.$host === 'number') out.push(object.$host);
    else for (const member of Object.values(object)) hostIds(member, out);
  }
  return out;
}

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

/** Every pinned ref id in a parsed description. */
function pinnedIds(value: unknown, out: number[] = []): number[] {
  if (isRef(value)) {
    if (value.pinned === true) out.push(value.$ref);
  } else if (value !== null && typeof value === 'object') {
    for (const member of Object.values(value)) pinnedIds(member, out);
  }
  return out;
}

const DESTROYED = '[core-wasm] kernel destroyed';

/** A `Blob` at the encoder: `Wire.call` reads every one it can reach
 *  first, so this one sits somewhere `readBinarySources` does not look. */
function unprepared(member: string | undefined): never {
  const error = new Error(`[core-wasm] a Blob reached the encoder unprepared: ${member ?? ''}`);
  console.error(error);
  throw error;
}

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

/** Members that only read: their calls change no state, so answers cached
 *  before them stay good while the store's revision holds. */
const READS = /^(get|list|is|has|can|resolve)[A-Z]/;

export class Wire {
  private attached!: RustKernel;
  /** Set by `dispose`, once the kernel is freed. */
  private dead = false;
  private version = 0;
  /** Every function the kernel may still call, by id, until `releaseJs`. */
  private readonly fns = new Map<number, Fn>();
  private readonly fnIds = new Map<Fn, number>();
  /** Ids are never reused, so one below this that `fns` lacks was released. */
  private nextFn = 0;
  /** Ids in a promise's settled value, encoded but not yet read by the
   *  kernel, with how many such values carry each: `releaseJs` leaves these
   *  alone (see `encodeInFlight`). */
  private readonly inFlight = new Map<number, number>();
  /** A capability's own refs, kept for good, as the kernel keeps them. */
  private readonly pinned = new Map<number, Fn>();
  /** Every other ref's function, held weakly: `released` hands the id back
   *  to the kernel once the function is collected. */
  private readonly loose = new Map<number, WeakRef<Fn>>();
  private readonly released = new FinalizationRegistry<number>((id) => {
    if (this.dead) return;
    this.loose.delete(id);
    this.attached.release_ref(id);
  });
  private readonly proxies = new Map<string, object>();
  private results = new Map<string, unknown>();
  /** Each (ref, argument JSON)'s last answer, ref ids blanked, and the value
   *  it decoded to, kept across `bump`: an answer whose text has not changed
   *  decodes to the value it did last time. `text` is the answer `value`
   *  was decoded from, whose refs it holds. Cleared whole past
   *  `ANSWERS_KEPT` keys. */
  private answers = new Map<string, { shape: string; text: string; value: unknown }>();
  private state: { revision: number; value: unknown } | undefined;
  private capabilities = new Map<string, object | null>();
  private capabilitiesRevision = -1;
  /** How many times the kernel's `subscribe` listener has fired
   *  (`kernel.ts` counts them). A call during which it moved changed the
   *  store, so its answer is not cached. */
  changes = 0;
  /** Each slice's JSON text and decoded value, by path (`core`,
   *  `plugins.stage`), from the last `getState` that parsed. */
  private slices = new Map<string, { revision: number; text: string; value: unknown }>();

  constructor(private readonly bridge: EngineBridge) {}

  /** The kernel, once it is built (it needs `callJs` to build). */
  attach(rust: RustKernel): void {
    this.attached = rust;
  }

  /** The kernel, for an explicit call: throws once it is freed, rather than
   *  letting wasm-bindgen answer with a null pointer. */
  get rust(): RustKernel {
    if (this.dead) throw new Error(DESTROYED);
    return this.attached;
  }

  /** The kernel is freed: forget everything it handed over or was handed,
   *  and make every later call throw. */
  dispose(): void {
    this.dead = true;
    this.fns.clear();
    this.fnIds.clear();
    this.inFlight.clear();
    this.pinned.clear();
    this.loose.clear();
    this.proxies.clear();
    this.results = new Map();
    this.answers = new Map();
    this.slices = new Map();
    this.state = undefined;
  }

  /** Drop the proxies of closed documents' capabilities (`take_evicted`),
   *  and the answers their members' calls left for reuse. Their refs are
   *  unpinned in the kernel, so each function here is held weakly from now
   *  on and released once collected, as a loose ref's is; one that was
   *  never decoded is released at once. Safe to call with nothing to
   *  evict: `capability`/`tryCapability` also call this, so a text this
   *  call already drained is the common case, not an error. */
  evict(): void {
    if (this.dead) return;
    const texts = JSON.parse(this.attached.take_evicted()) as string[];
    // A closed document's proxies must not outlive it in the capability
    // cache: they hold its refs, and through them its images.
    if (texts.length > 0) this.capabilities = new Map();
    const evicted = new Set<number>();
    for (const text of texts) {
      this.proxies.delete(text);
      for (const id of pinnedIds(JSON.parse(text))) {
        evicted.add(id);
        const fn = this.pinned.get(id);
        this.pinned.delete(id);
        if (!fn) {
          this.attached.release_ref(id);
          continue;
        }
        this.loose.set(id, new WeakRef(fn));
        this.released.register(fn, id);
      }
    }
    if (evicted.size === 0) return;
    // Keyed `${ref}:${argument JSON}` (see `call`).
    const stale = (key: string) => evicted.has(Number(key.slice(0, key.indexOf(':'))));
    for (const key of [...this.answers.keys()]) if (stale(key)) this.answers.delete(key);
    for (const key of [...this.results.keys()]) if (stale(key)) this.results.delete(key);
  }

  /** The store changed: every cached result is stale. */
  bump(): void {
    this.version++;
    this.results = new Map();
  }

  /** The store revision `results` holds answers for. */
  private resultsRevision = -1;

  /** Drop `results` if the store has moved since they were cached. The
   *  revision moves on every change, before listeners hear of it
   *  (`Store::revision`), so this sees a change whose notification the
   *  store deferred. */
  private sync(): void {
    let revision: number;
    try {
      // Not yet attached (a callback while the kernel is being built), or
      // a kernel that can't be read from here (one being destroyed holds
      // itself exclusively): nothing cached can be trusted.
      revision = this.attached.state_revision();
    } catch {
      this.bump();
      this.resultsRevision = -1;
      return;
    }
    if (revision === this.resultsRevision) return;
    this.bump();
    this.resultsRevision = revision;
  }

  /** After a real call: one that may have changed state outside the store
   *  (a registry, a handler list) drops every cached answer; a read keeps
   *  them unless it moved the store. */
  private settle(member: string): void {
    if (!READS.test(member)) this.bump();
    this.sync();
  }

  /** `value` as canonical JSON (an object, not text). `member` names the
   *  call it is encoded for in the error a `Blob` throws. */
  encode(value: unknown, signals?: AbortSignal[], member?: string): unknown {
    return canonical(value, {
      hold: this.bridge.hold,
      fn: (f) => ({ $fn: `js:${this.fnId(f)}` }),
      signal: signals ? (s) => signals.push(s) : undefined,
      blob: () => unprepared(member),
    });
  }

  decode(value: unknown): unknown {
    return decode(value, this.bridge.host, this.hook);
  }

  /** `encode` for a value the kernel reads later, not during the call that
   *  encodes it: a `callJs` promise's settled value, which Rust's `from_js`
   *  takes on a microtask of its own. Until then, a release of one of its
   *  ids (the kernel dropping the last callback on the same function, from
   *  some other call in between) must not forget the function the value is
   *  about to hand over; the kernel says `release_js` again once it drops
   *  the callbacks it makes from this value. The next task runs after every
   *  microtask, so the kernel has read the value by then. */
  private encodeInFlight(value: unknown): unknown {
    const ids: number[] = [];
    const out = canonical(value, {
      hold: this.bridge.hold,
      blob: () => unprepared('a callback value'),
      fn: (f) => {
        const id = this.fnId(f);
        ids.push(id);
        this.inFlight.set(id, (this.inFlight.get(id) ?? 0) + 1);
        return { $fn: `js:${id}` };
      },
    });
    if (ids.length > 0) {
      setTimeout(() => {
        for (const id of ids) {
          const left = (this.inFlight.get(id) ?? 1) - 1;
          if (left > 0) this.inFlight.set(id, left);
          else this.inFlight.delete(id);
        }
      }, 0);
    }
    return out;
  }

  /** `release_js`: no callback in the kernel holds function `id` any more. */
  readonly releaseJs = (id: string): void => {
    if (this.dead) return;
    const n = Number(id);
    if (this.inFlight.has(n)) return;
    const fn = this.fns.get(n);
    if (!fn) {
      if (n < this.nextFn) console.warn(`[core-wasm] release_js(${id}): already released`);
      return;
    }
    this.fns.delete(n);
    this.fnIds.delete(fn);
  };

  /** `call_js`: a `$fn` callback's call from the kernel. */
  readonly callJs = (id: string, argsJson: string): unknown => {
    if (this.dead) throw new Error(DESTROYED);
    // A callback from inside a kernel call: the store may have moved since
    // anything was cached, with its notification still to come.
    this.sync();
    const n = Number(id);
    const fn = this.fns.get(n);
    if (!fn) {
      if (n < this.nextFn) {
        const error = new Error(`[core-wasm] js function ${id} was released`);
        console.error(error);
        throw error;
      }
      throw new Error(`[core-wasm] no JavaScript function ${id}`);
    }
    const args = this.decode(JSON.parse(argsJson)) as unknown[];
    let out: unknown;
    try {
      out = fn(...args);
    } catch (error) {
      throw errorSpec(error, (v) => this.encode(v));
    }
    if (!isThenable(out)) return this.encode(out);
    return Promise.resolve(out).then(
      // `readBinarySources`, not `readBlobs`: the file picker answers
      // `{ data: file }`, and an attachment needs the file's name.
      async (value) => this.encodeInFlight(await readBinarySources(value)),
      (error) => {
        throw errorSpec(error, (v) => this.encode(v));
      },
    );
  };

  /** `kernel.capability(token, doc)` as its proxy. Drains `take_evicted`
   *  first (see `evict`'s doc comment): a document close is picked up here
   *  even if the kernel's own `closed` listener
   *  (`crates/epdf-web/src/exports.rs`, which fills the evicted list) has
   *  not yet run ahead of whatever registered this wire's `onClosed`
   *  eviction. Not done in `bump`: that also runs inside `callJs`, called
   *  reentrantly from a Rust callback already on the stack, where a second
   *  call into the same wasm object throws "recursive use of an object".
   *  `capability`/`tryCapability` are only ever called from outside such a
   *  callback. */
  capability(tokenName: string, doc?: string | null): object {
    const key = this.capabilityKey(tokenName, doc);
    const cached = this.capabilities.get(key);
    if (cached) return cached;
    this.evict();
    // Throws the kernel's own refusal for a capability that isn't there.
    const found = this.proxy(rusty(() => this.rust.capability(tokenName, doc ?? null)));
    this.capabilities.set(key, found);
    return found;
  }

  tryCapability(tokenName: string, doc?: string | null): object | null {
    const key = this.capabilityKey(tokenName, doc);
    if (this.capabilities.has(key)) return this.capabilities.get(key) ?? null;
    this.evict();
    const text = rusty(() => this.rust.try_capability(tokenName, doc ?? null));
    const found = text === undefined || text === null ? null : this.proxy(text);
    this.capabilities.set(key, found);
    return found;
  }

  /** The key into `capabilities`, emptied first if core moved. Which
   *  capabilities exist, and for which documents, only changes when a
   *  document opens, closes or becomes ready, all core changes. Hooks read
   *  capabilities on every render, and each miss is an eviction check, a
   *  lookup in the kernel, and hashing the answer's description text to find
   *  its proxy. */
  private capabilityKey(tokenName: string, doc?: string | null): string {
    const revision = this.rust.core_revision();
    if (revision !== this.capabilitiesRevision) {
      this.capabilities = new Map();
      this.capabilitiesRevision = revision;
    }
    return `${tokenName}\u0000${doc ?? ''}`;
  }

  documents(): object {
    return this.proxy(rusty(() => this.rust.documents()));
  }

  /** The kernel's state, parsed once per store change, and then only the
   *  parts of it that changed. */
  getState(): unknown {
    // Keyed on the store's own revision, not `version`: `version` also moves
    // on every real call (see `call`) and every callback, which does not
    // mean the store changed.
    const revision = this.rust.state_revision();
    if (this.state?.revision === revision) return this.state.value;
    // A revision per part: only a part whose revision moved is fetched and
    // parsed. Rebuilding and parsing the whole state per change (core alone
    // holds every page's layout) was most of a scroll's wire time. The same
    // text keeps the same object, and a changed store keeps every slice whose
    // text did not change, as the TypeScript store keeps an untouched
    // plugin's slice.
    const revisions = JSON.parse(this.rust.state_revisions()) as {
      core: number;
      plugins: Record<string, number>;
    };
    const slices = new Map<string, { revision: number; text: string; value: unknown }>();
    let changed = this.state === undefined;
    const part = (path: string, partRevision: number, read: () => string | undefined) => {
      const last = this.slices.get(path);
      if (last?.revision === partRevision) {
        slices.set(path, last);
        return last.value;
      }
      const text = read() ?? 'undefined';
      if (last?.text === text) {
        const kept = { ...last, revision: partRevision };
        slices.set(path, kept);
        return kept.value;
      }
      changed = true;
      const entry = {
        revision: partRevision,
        text,
        value: text === 'undefined' ? undefined : this.decode(JSON.parse(text)),
      };
      slices.set(path, entry);
      return entry.value;
    };
    const core = part('core', revisions.core, () => this.rust.core_text());
    const plugins: Record<string, unknown> = {};
    for (const [id, pluginRevision] of Object.entries(revisions.plugins)) {
      plugins[id] = part(`plugins.${id}`, pluginRevision, () => this.rust.slice_text(id));
    }
    // A slice that went away changes the state too.
    if (this.slices.size !== slices.size) changed = true;
    this.slices = slices;
    if (!changed && this.state) {
      this.state.revision = revision;
      return this.state.value;
    }
    const value = { core, plugins };
    this.state = { revision, value };
    return value;
  }

  /** A promise id as a promise of its decoded value, with `abort`. A
   *  `member` in `RELEASES_HOSTS` has the value's `$host` entries released
   *  once it is decoded. A promise is never cached or reused (`invoke`), so
   *  nothing decodes the same value twice. */
  promise(id: number, member?: string): Promise<unknown> & { abort(reason?: unknown): void } {
    const out = this.rust.promise(id).then(
      (value) => {
        const live = this.decode(value);
        if (member !== undefined && RELEASES_HOSTS.has(member)) {
          for (const n of hostIds(value)) this.bridge.release(n);
        }
        return live;
      },
      (error) => {
        throw toError(error);
      },
    ) as Promise<unknown> & { abort(reason?: unknown): void };
    // A no-op once the kernel is freed: an `AbortSignal` can fire long after.
    out.abort = (reason?: unknown) => {
      if (this.dead) return;
      this.attached.abort_promise(
        id,
        reason === undefined ? null : JSON.stringify(this.encode(reason)),
      );
    };
    return out;
  }

  /** Live entries in this wire's own tables, for the eval build's memory
   *  soak test: `fns` handed to Rust, `refs` handed back from it, and
   *  cached `proxies`. */
  counts(): { fns: number; refs: number; proxies: number } {
    const refs = this.pinned.size + this.loose.size;
    return { fns: this.fns.size, refs, proxies: this.proxies.size };
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
      id = this.nextFn++;
      this.fns.set(id, f);
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
    const prepared = prepareArgs(member, args);
    return hasBlob(prepared)
      ? this.callWithBlobs(id, member, prepared)
      : this.invoke(id, member, prepared);
  }

  /** A call with a `Blob` among its arguments, made once `readBinarySources`
   *  has read them: a `Blob` has no synchronous read, and holding it as a
   *  `$host` gives Rust nothing it can read. Every member whose signature
   *  takes one answers a promise, so this changes when the call happens,
   *  not what it answers; one that answers anything else throws. */
  private callWithBlobs(
    id: number,
    member: string,
    args: unknown[],
  ): Promise<unknown> & { abort(reason?: unknown): void } {
    let inner: { abort(reason?: unknown): void } | undefined;
    let early: { reason: unknown } | undefined;
    const out = readBinarySources(args).then((read) => {
      const answer = this.invoke(id, member, read as unknown[]);
      if (!isThenable(answer)) {
        const error = new Error(`[core-wasm] ${member} took a Blob but did not answer a promise`);
        console.error(error);
        throw error;
      }
      inner = answer as unknown as { abort(reason?: unknown): void };
      if (early) inner.abort(early.reason);
      return answer;
    }) as Promise<unknown> & { abort(reason?: unknown): void };
    out.abort = (reason?: unknown) => {
      if (inner) inner.abort(reason);
      else early = { reason };
    };
    return out;
  }

  private invoke(id: number, member: string, args: unknown[]): unknown {
    const signals: AbortSignal[] = [];
    const json = JSON.stringify(this.encode(args, signals, member));
    const key = `${id}:${json}`;
    this.sync();
    if (this.results.has(key)) return this.results.get(key);
    const changes = this.changes;
    const revision = this.resultsRevision;
    // Cache an answer only from a call that changed nothing: no change
    // notification, and no store revision moved during it.
    // Only a read's answer is kept: any other call runs every time.
    const unmoved = () =>
      READS.test(member) && this.changes === changes && this.resultsRevision === revision;
    const text = rusty(() => this.rust.invoke_ref(id, json));
    // The same text as the answer kept for these arguments: the same value,
    // with no parse. Only a returned value is kept, and identical text holds
    // the same ref ids, all still live.
    const kept = this.answers.get(key);
    if (kept !== undefined && kept.text === text && !MAKES_NEW.test(member)) {
      this.settle(member);
      if (unmoved()) this.results.set(key, kept.value);
      return kept.value;
    }
    const outcome = JSON.parse(text) as Outcome;
    // A real call, not a cache hit: the store may have deferred its change
    // notification (a dispatch from inside a change listener only sets
    // `pending`, `crates/epdf-kernel/src/store.rs:206-219`), so this can't
    // wait for `subscribe` to fire. It reads the store's revision instead,
    // and drops everything after a call that isn't a read.
    this.settle(member);
    if ('threw' in outcome) throw toError(outcome.threw);
    if ('promise' in outcome) {
      const promise = this.promise(outcome.promise, member);
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
    const shape = text.includes('"$ref"') ? text.replace(REF_ID, '{"$ref":0}') : text;
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
      if (unmoved()) this.results.set(key, value);
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
